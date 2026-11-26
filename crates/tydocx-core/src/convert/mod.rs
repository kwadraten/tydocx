//! Tag-walker based Typst -> OOXML conversion.
//!
//! Walks `HtmlDocument`'s `Tag` sequence. Each `Tag::Start` carries a
//! `Location` that maps via the `Introspector` to the full Content AST for
//! that element, giving us direct access to `HeadingElem`, `EquationElem`,
//! `FootnoteElem`, etc. without parsing HTML tags.

mod baseline;
mod bibliography;
mod block;
mod breaks;
mod coalesce;
mod dom;
mod fmt;
mod footnote;
mod frames;
mod headings;
mod highlight;
mod image;
pub mod inline;
mod inline_walk;
mod lists;
pub mod page;
mod postprocess;
pub mod realize;
mod smallcaps;
mod source;
mod stats;
mod table_align;
mod table_width;
mod tables;
mod text_norm;

pub use realize::{DocxDocument, convert_ir, export_ir, export_ir_with_ir};

use std::collections::{HashMap, HashSet};
use std::hash::RandomState;
use std::str::FromStr;

use tydocx_ooxml::document::{
    Alignment, BlockElement, CellContent, Document, HangingIndent, HeaderFooter, ImageData,
    InlineElement, ListInfo, Paragraph, ParagraphStyle, Run, Table, TableCell, TableRow, VMerge,
};
use typst::comemo::Track;
use typst::diag::SourceDiagnostic;
use typst::foundations::{Content, NativeElement, Packed, Selector, Smart, StyleChain};
use typst::introspection::{Introspector, Location, Tag};
use typst::model::Numbering;
use typst_html::{HtmlDocument, HtmlElement, HtmlNode};
use typst_layout::PagedDocument;
use typst_library::math::EquationElem;
use typst_library::model::{
    CiteElem, CiteGroup, EmphElem, HeadingElem, LinkElem, OutlineElem, RefElem, StrongElem,
    TableElem,
};
use typst_library::text::{HighlightElem, Lang, Region, SmartQuoteElem, SmartQuoter, SmartQuotes};

use crate::compat::{CompatMode, ConvertOptions, ConvertOutcome, Degradation, DegradationAction};
use crate::world::TydocxWorld;
use fmt::InlineFmt;

use baseline::apply_baseline_shifts_from_source;
use block::{emit_block_equation, is_inline_equation_at, walk_tags};
use dom::{
    children_are_inline, collect_deep_text, collect_flat_text, collect_li_ids, content_at_location,
    detect_alignment, drain_text_runs, element_at_location, find_body, find_first_element,
    find_img_src, find_tag_end, first_biblioref_href, get_attr_value, has_attr_value,
    is_block_equation, is_doc_endnotes_section, run_with_span, sanitize_anchor,
    subtree_has_element, tag_name, tag_span,
};
use headings::handle_heading;
use highlight::{HighlightStyle, highlight_style};
use inline_walk::{InlineOptions, collect_inlines};
use lists::{
    convert_html_list, convert_html_list_to_paragraphs, enum_location_before, enum_numbering_at,
    handle_list_tag,
};
use postprocess::{apply_paragraph_formatting, extract_document_metadata};
use smallcaps::apply_smallcaps_from_source;
use source::{apply_hanging_indent_from_source, apply_source_overrides, gather_source_overrides};
use tables::{convert_html_table, handle_table_tag};

/// Tracks equation numbering state across the document.
#[derive(Default)]
struct EquationState {
    /// Current chapter number (incremented on h1 headings).
    chapter: u64,
    /// Equation counter within the current chapter.
    eq_in_chapter: u64,
    /// Global equation counter.
    global_eq: u64,
}

/// State threaded through the recursive tag-walker.
///
/// Replaces the `(html_doc, doc, eq_state, image_queue, bookmarks, page_breaks)`
/// parameter tuple that previously appeared on every walk/handle function. The
/// two read-only references are shared (`&`) and the four sinks are mutable
/// (`&mut`); pass the whole thing as `&mut WalkCtx`.
///
/// Borrow note: when a function both reads `ctx.html_doc.introspector` and
/// mutates a `&mut` field, lift `let html = ctx.html_doc;` first — `&HtmlDocument`
/// is `Copy`, so this severs the borrow tie at zero cost.
struct WalkCtx<'a> {
    world: &'a TydocxWorld,
    html_doc: &'a HtmlDocument,
    doc: &'a mut Document,
    eq_state: &'a mut EquationState,
    /// On-page display sizes (EMU) by image-content hash, from the paged
    /// frames. Image CONTENT comes from each `<img>`'s own src data-URL, so
    /// there is no positional queue to desync — this map only answers "how
    /// large did Typst draw these bytes".
    image_sizes: &'a HashMap<u64, image::ImagePlacement>,
    /// Rasterized vector-drawing canvases, keyed by their owning figure's
    /// introspection `Location` (a drawing body is dropped from the HTML
    /// export, so its pixels come from the paged frames). Keyed — not a FIFO —
    /// so a raster can only ever land on its own figure.
    figure_rasters: &'a mut HashMap<typst::introspection::Location, ImageData>,
    bookmarks: &'a mut HashSet<String>,
    /// Citation keys declared by the bibliography. A `<ref>` whose target is one
    /// of these is a citation (rendered as a marker like `[27]`), not a
    /// cross-reference to a bookmarked figure/equation/heading.
    bib_keys: &'a HashSet<String>,
    /// Compat mode of this conversion. C1 gates degrade constructs the
    /// OOXML intersection cannot express; C2/C3 keep the current output.
    compat: CompatMode,
    /// Degradations found so far. C1 gates push here, as do all-mode
    /// literalizations (e.g. a page-counter value kept as literal text).
    /// The walk pushes; `export` returns the list as part of [`ConvertOutcome`].
    degradations: &'a mut Vec<Degradation>,
}

impl WalkCtx<'_> {
    /// Add a labelled bookmark once, allocating its document-wide id here.
    fn add_bookmark(&mut self, para: &mut Paragraph, label: String) -> bool {
        if self.bookmarks.insert(label.clone()) {
            para.add_bookmark(self.doc.next_bookmark_id(), label);
            true
        } else {
            false
        }
    }
}

/// Convert a Typst source file to an OOXML `Document` using the tag-walker approach.
///
/// Compiles both targets, then hands them to [`export`]. The returned
/// outcome pairs the document with the degradations reported during
/// conversion (page/footnote counters kept as literal text, unmappable
/// numbering patterns, ...; Phase 1 adds the per-mode gates).
///
/// # Errors
/// Returns compilation errors, with their spans, if the Typst source cannot
/// be compiled.
pub fn convert(
    world: &TydocxWorld,
    options: &ConvertOptions,
) -> Result<ConvertOutcome, Vec<SourceDiagnostic>> {
    // The realize path (step 2) is the conversion path. The legacy
    // tag-walker `export` stays as the explicitly-injected fallback and
    // comparison baseline (see docs/fallback-registry.md).
    realize::convert_ir(world, options)
}

/// The legacy tag-walker path, as a full pipeline: compile both targets
/// and run `export`. Kept as the explicitly-injected fallback and as the
/// comparison baseline for the realize path's equivalence tests.
///
/// # Errors
///
/// Returns the Typst compile diagnostics when the source fails to compile.
pub fn convert_legacy(
    world: &TydocxWorld,
    options: &ConvertOptions,
) -> Result<ConvertOutcome, Vec<SourceDiagnostic>> {
    let (html_doc, paged_doc) = compile_both_targets(world)?;
    Ok(export(&html_doc, paged_doc.as_ref(), world, options))
}

/// Export already-compiled documents to an OOXML `Document`.
///
/// Runs pipeline phases 2–5 (page settings and document style, HTML walk,
/// headers and footers, post-processing) over the compiled documents. This is
/// the backend-shaped entry point: callers that already hold compiled
/// documents (a host compiler, a test) use this instead of [`convert`].
#[must_use]
pub fn export(
    html_doc: &HtmlDocument,
    paged_doc: Option<&PagedDocument>,
    world: &TydocxWorld,
    options: &ConvertOptions,
) -> ConvertOutcome {
    let mut degradations = Vec::new();

    let mut doc = Document::new();

    // 2. Apply page settings and document style.
    apply_page_and_style(
        world,
        paged_doc,
        &mut doc,
        options.compat,
        &mut degradations,
    );

    // 3. Walk the HTML body.
    walk_body(
        world,
        html_doc,
        paged_doc,
        &mut doc,
        options.compat,
        &mut degradations,
    );

    // 4. Apply headers and footers.
    apply_headers_and_footers(
        paged_doc,
        world,
        &mut doc,
        options.compat,
        &mut degradations,
        &HashSet::new(),
        None,
    );

    // 5. Run post-processing passes.
    run_post_passes(
        world,
        html_doc,
        paged_doc,
        &mut doc,
        options.compat,
        &mut degradations,
    );

    ConvertOutcome::new(doc, degradations)
}

fn compile_both_targets(
    world: &TydocxWorld,
) -> Result<(HtmlDocument, Option<PagedDocument>), Vec<SourceDiagnostic>> {
    let html_result = typst::compile::<HtmlDocument>(world);
    let html_doc = match html_result.output {
        Ok(doc) => doc,
        Err(errors) => return Err(errors.into_iter().collect()),
    };

    let paged_result = typst::compile::<PagedDocument>(world);
    let paged_doc = match paged_result.output {
        Ok(doc) => Some(doc),
        Err(errors) => return Err(errors.into_iter().collect()),
    };

    Ok((html_doc, paged_doc))
}

fn apply_page_and_style(
    world: &TydocxWorld,
    paged_doc: Option<&PagedDocument>,
    doc: &mut Document,
    compat: CompatMode,
    degradations: &mut Vec<Degradation>,
) {
    // Extract page settings and document style from PagedDocument (heuristic).
    if let Some(paged) = paged_doc {
        doc.style = page::extract_document_style(paged);
        page::extract_page_settings(paged, &mut doc.page_settings);
    }

    // Override with authoritative values from source AST.
    let source_overrides = gather_source_overrides(world);
    apply_source_overrides(&source_overrides, doc, paged_doc.is_some(), degradations);
    source::report_mixed_margin_mirroring(&source_overrides, compat, degradations);

    // Footnote numbering restarts per page only when the source resets the
    // footnote counter in a page header; Typst's default is continuous.
    doc.style.footnote_restart_each_page = source::detect_footnote_restart_each_page(world);

    // Note: page column count comes solely from the source AST
    // (`#set page(columns:)` / `#page(columns:)`, parsed above). There is no
    // geometric fallback — left-edge clustering cannot distinguish a real
    // multi-column page from a wide table or aligned equations, and measurement
    // showed it misread ~17 single-column fixtures as multi-column while the
    // genuine three column documents are all covered by the source parse.
}

fn walk_body(
    world: &TydocxWorld,
    html_doc: &HtmlDocument,
    paged_doc: Option<&PagedDocument>,
    doc: &mut Document,
    compat: CompatMode,
    degradations: &mut Vec<Degradation>,
) {
    // From the PagedDocument: on-page image display sizes keyed by content
    // hash (content itself comes from each <img>'s src data-URL during the
    // walk), and drawing-canvas rasters keyed by their figure's Location.
    // Both keyed — no positional queues to desync. Computed before the
    // footnote extraction below so footnote images get real display sizes.
    let (image_sizes, mut figure_rasters) = if let Some(paged) = paged_doc {
        (
            image::collect_image_sizes(paged),
            image::extract_figure_rasters(paged),
        )
    } else {
        (HashMap::new(), HashMap::new())
    };

    // First pass: extract footnote content from <section role="doc-endnotes">,
    // add it to the document, and size the footnote text from the Paged render.
    let body = find_body(html_doc.root()).unwrap_or_else(|| html_doc.root());
    footnote::extract_add_and_size_footnotes(doc, &body.children, paged_doc, &image_sizes);

    // Walk the HTML tree's Tag sequence. Explicit `#pagebreak()` breaks are
    // recovered from the source AST during post-processing; automatic page-flow
    // boundaries deliberately reflow in Word rather than become hard breaks.
    let mut eq_state = EquationState::default();
    let mut bookmarks: HashSet<String> = HashSet::new();
    // Citation keys, so the <ref> handler can tell a citation from a cross-ref.
    let bib_keys: HashSet<String> = typst_library::model::BibliographyElem::keys(
        (&**html_doc.introspector() as &dyn typst_library::introspection::Introspector).track(),
    )
    .into_iter()
    .map(|(label, _)| label.resolve().to_string())
    .collect();
    {
        let mut ctx = WalkCtx {
            world,
            html_doc,
            doc,
            eq_state: &mut eq_state,
            image_sizes: &image_sizes,
            figure_rasters: &mut figure_rasters,
            bookmarks: &mut bookmarks,
            bib_keys: &bib_keys,
            compat,
            degradations,
        };
        walk_tags(&body.children, &mut ctx);
    }

    // Detect footnote format (circled numbers).
    footnote::detect_footnote_format(&body.children, doc);
}

fn apply_headers_and_footers(
    paged_doc: Option<&PagedDocument>,
    world: &TydocxWorld,
    doc: &mut Document,
    compat: CompatMode,
    degradations: &mut Vec<Degradation>,
    body_places: &HashSet<u64, RandomState>,
    section_ranges: Option<&[std::ops::Range<usize>]>,
) {
    // Extract headers and footers first so header/footer text is not
    // misidentified as body content. All margin-zone consumers
    // share the resolved margins (paged default overridden by the source AST),
    // so a small author margin never misfiles body text as header/footer.
    if let Some(paged) = paged_doc {
        let margins = page::MarginsPt::from_settings(&doc.page_settings);
        // C2/C3: measure the page furniture layers (background/foreground,
        // `#place` inside the header/footer zones, `page(fill:)`) so they can
        // ride as `wp:anchor` drawings in header/footer parts. C1 keeps its
        // literalize-furniture contract and skips all of this.
        let furniture_maps = analyze_furniture(paged, world, &margins, compat, body_places);
        let maps = furniture_maps.as_ref();
        // The declared page-numbering format (from `#set page(numbering:)`,
        // consumed in `apply_source_overrides`) wins over glyph guessing.
        let declared = doc.page_numbering;
        let ranges: Vec<std::ops::Range<usize>> = match section_ranges {
            Some(r) if compat != CompatMode::C1 && r.len() > 1 => r.to_vec(),
            _ => std::iter::once(0..paged.pages().len()).collect(),
        };
        let mut tops: Vec<page::ZoneParts> = ranges
            .iter()
            .map(|range| {
                page::extract_zone_parts_in(
                    paged,
                    page::MarginZone::Top,
                    margins,
                    declared,
                    maps,
                    range.clone(),
                )
            })
            .collect();
        let zones = zone_heading_levels(world, compat);
        apply_top_stylerefs(&mut tops, &ranges, &zones, compat, degradations);
        let (top, header_differs, section_differs) = install_header_sets(doc, tops);
        // Detect page numbering before extracting footer. If the footer is just
        // a page number, the writer generates a PAGE field footer instead.
        let numbering = page::detect_page_numbering(paged, margins, maps)
            .or_else(|| page::detect_multi_format_page_numbering(paged, margins, maps));
        let mut footer_first_differs = false;
        let mut footer_even_differs = false;
        let mut bottom: Option<page::ZoneParts> = None;
        // A `#place` in the footer zone can't ride inside the generated
        // PAGE-field footer — its zone has to be extracted instead (a pure
        // page-number token in it still becomes a live `PAGE` field).
        let footer_has_places =
            maps.is_some_and(|m| m.per_page.iter().any(|f| !f.footer.is_empty()));
        // Read before the closure below captures `doc` mutably.
        let footer_absent = doc.footer.is_none();
        let mut extract_bottom = |bottom_slot: &mut Option<page::ZoneParts>| {
            let mut parts =
                page::extract_zone_parts(paged, page::MarginZone::Bottom, margins, declared, maps);
            let footer_styleref = zones.footer.as_ref().is_some_and(|zref| {
                styleref_zone(&mut parts.default, zref, "footer", degradations)
                    | styleref_zone(&mut parts.even, zref, "footer", degradations)
                    | styleref_zone(&mut parts.first, zref, "footer", degradations)
            });
            footer_first_differs = parts.title_page;
            footer_even_differs = parts.even_and_odd;
            doc.even_and_odd_headers |= parts.even_and_odd;
            doc.title_page |= parts.title_page;
            if !footer_styleref {
                report_zone_unrepresentable(&parts, "footer", compat, 1, degradations);
            }
            doc.footer.clone_from(&parts.default);
            doc.footer_even.clone_from(&parts.even);
            doc.footer_first.clone_from(&parts.first);
            *bottom_slot = Some(parts);
        };
        if let Some(fmt) = numbering {
            if declared.is_none() {
                doc.page_numbering = Some(fmt);
            }
            // A heading-query footer is not a page number — it must be
            // extracted for the STYLEREF splice, not replaced by the
            // generated PAGE-field footer.
            if footer_has_places || zones.footer.is_some() {
                if footer_absent {
                    extract_bottom(&mut bottom);
                }
            } else {
                doc.page_number_footers.default = true;
                // Don't set doc.footer — the writer generates a PAGE field footer
            }
        } else if footer_absent {
            extract_bottom(&mut bottom);
        }
        if doc.title_page {
            doc.page_number_footers.first =
                doc.page_number_footers.default && !footer_first_differs;
        }
        if doc.even_and_odd_headers {
            doc.page_number_footers.even = doc.page_number_footers.default && !footer_even_differs;
        }
        let differs = ZoneDiffers {
            header: header_differs,
            footer: ZoneSlotDiffers {
                first: footer_first_differs,
                even: footer_even_differs,
            },
        };
        propagate_slot_parts(doc, &differs);
        propagate_section_headers(doc, &section_differs);
        // Furniture anchors ride in after propagation: parts that the
        // first/even slots just cloned also carry the matching anchors.
        if let Some(maps) = furniture_maps {
            apply_furniture_plan(&maps, doc, &differs, &section_differs, degradations);
        }
        blank_section_headers(doc, &ranges);
        if compat == CompatMode::C1 {
            c1_zone_collapse(paged, margins, doc, &top, bottom.as_ref(), degradations);
        }
    }
}

fn apply_top_stylerefs(
    tops: &mut [page::ZoneParts],
    ranges: &[std::ops::Range<usize>],
    zones: &page::PageZoneHeadings,
    compat: CompatMode,
    degradations: &mut Vec<Degradation>,
) {
    for (top, range) in tops.iter_mut().zip(ranges) {
        let header_styleref = zones.header.as_ref().is_some_and(|zref| {
            styleref_zone(&mut top.default, zref, "header", degradations)
                | styleref_zone(&mut top.even, zref, "header", degradations)
                | styleref_zone(&mut top.first, zref, "header", degradations)
        });
        if !header_styleref {
            report_zone_unrepresentable(top, "header", compat, range.start + 1, degradations);
        }
    }
}

fn install_header_sets(
    doc: &mut Document,
    tops: Vec<page::ZoneParts>,
) -> (page::ZoneParts, ZoneSlotDiffers, Vec<ZoneSlotDiffers>) {
    let mut tops = tops.into_iter();
    let top = tops.next().unwrap_or_default();
    let differs = ZoneSlotDiffers {
        first: top.title_page,
        even: top.even_and_odd,
    };
    doc.even_and_odd_headers |= top.even_and_odd;
    doc.title_page |= top.title_page;
    if doc.header.is_none() {
        doc.header.clone_from(&top.default);
    }
    doc.header_even.clone_from(&top.even);
    doc.header_first.clone_from(&top.first);
    let mut section_differs = Vec::new();
    for part in tops {
        doc.even_and_odd_headers |= part.even_and_odd;
        doc.section_headers
            .push(tydocx_ooxml::document::SectionHeaderSet {
                default: part.default,
                even: part.even,
                first: part.first,
                title_page: part.title_page,
            });
        section_differs.push(ZoneSlotDiffers {
            first: part.title_page,
            even: part.even_and_odd,
        });
    }
    (top, differs, section_differs)
}

/// C1 keeps one header and one footer: variants report, even/first parts
/// drop, and a suppressed first-page zone stays suppressed.
fn c1_zone_collapse(
    paged: &PagedDocument,
    margins: page::MarginsPt,
    doc: &mut Document,
    top: &page::ZoneParts,
    bottom: Option<&page::ZoneParts>,
    degradations: &mut Vec<Degradation>,
) {
    if let Some(degradation) = page::header_footer_variant(paged, margins) {
        degradations.push(degradation);
    }
    let page_one = |parts: &page::ZoneParts, default: Option<HeaderFooter>| {
        if parts.title_page && parts.first.is_none() {
            None
        } else {
            parts.first.clone().or(default)
        }
    };
    doc.header = page_one(top, doc.header.take());
    doc.footer = bottom.map_or(doc.footer.take(), |parts| {
        page_one(parts, doc.footer.take())
    });
    doc.header_even = None;
    doc.header_first = None;
    doc.footer_even = None;
    doc.footer_first = None;
    doc.section_headers.clear();
    doc.title_page = false;
    doc.even_and_odd_headers = false;
    doc.page_number_footers.even = false;
    doc.page_number_footers.first = false;
}

/// Measure the page furniture layers for C2/C3 — `None` at C1, which keeps
/// its literalize-furniture contract.
fn analyze_furniture(
    paged: &PagedDocument,
    world: &TydocxWorld,
    margins: &page::MarginsPt,
    compat: CompatMode,
    body_places: &HashSet<u64, RandomState>,
) -> Option<page::furniture::FurnitureMaps> {
    (compat != CompatMode::C1).then(|| {
        let overrides = gather_source_overrides(world);
        page::furniture::analyze(
            paged,
            margins,
            page::furniture::DeclaredFurniture {
                foreground: overrides.page_foreground,
                background: overrides.page_background,
            },
            body_places,
        )
    })
}

/// Which first/even slots one zone classified as genuinely different —
/// cloning the default part over one of those would fabricate a slot the
/// source deliberately suppressed.
struct ZoneSlotDiffers {
    first: bool,
    even: bool,
}

/// Header and footer `ZoneSlotDiffers` pairs.
struct ZoneDiffers {
    header: ZoneSlotDiffers,
    footer: ZoneSlotDiffers,
}

fn propagate_section_headers(doc: &mut Document, differs: &[ZoneSlotDiffers]) {
    if !doc.even_and_odd_headers {
        return;
    }
    for (set, differ) in doc.section_headers.iter_mut().zip(differs) {
        if !differ.even && set.even.is_none() && set.default.is_some() {
            set.even = set.default.clone();
        }
    }
}

fn blank_section_headers(doc: &mut Document, ranges: &[std::ops::Range<usize>]) {
    let blank = || {
        Some(HeaderFooter {
            paragraphs: vec![Paragraph::new()],
        })
    };
    let mut emit_default = doc.header.is_some();
    let mut emit_even = doc.header_even.is_some();
    let mut emit_first = doc.header_first.is_some();
    for (i, set) in doc.section_headers.iter_mut().enumerate() {
        let (had_default, had_even, had_first) = (
            set.default.is_some(),
            set.even.is_some(),
            set.first.is_some(),
        );
        if ranges.get(i + 1).is_none_or(|r| !r.is_empty()) {
            if !had_default && emit_default {
                set.default = blank();
            }
            if doc.even_and_odd_headers && !had_even && emit_even {
                set.even = blank();
            }
            if set.title_page && !had_first && emit_first {
                set.first = blank();
            }
        }
        emit_default |= had_default;
        emit_even |= had_even;
        emit_first |= had_first;
    }
}

/// Clone the default header/footer parts into the first/even slots that
/// `title_page`/`even_and_odd_headers` just enabled — never over a slot the
/// source left different on purpose.
fn propagate_slot_parts(doc: &mut Document, differs: &ZoneDiffers) {
    if doc.title_page {
        if !differs.header.first && doc.header_first.is_none() && doc.header.is_some() {
            doc.header_first = doc.header.clone();
        }
        if !differs.footer.first && doc.footer_first.is_none() && doc.footer.is_some() {
            doc.footer_first = doc.footer.clone();
        }
    }
    if doc.even_and_odd_headers {
        if !differs.header.even && doc.header_even.is_none() && doc.header.is_some() {
            doc.header_even = doc.header.clone();
        }
        if !differs.footer.even && doc.footer_even.is_none() && doc.footer.is_some() {
            doc.footer_even = doc.footer.clone();
        }
    }
}

/// Plan the furniture layers' header-part slots, OR the plan's flag needs
/// into the document, re-propagate default parts into any slots the plan
/// enabled, then attach the measured anchors.
fn apply_furniture_plan(
    maps: &page::furniture::FurnitureMaps,
    doc: &mut Document,
    differs: &ZoneDiffers,
    section_differs: &[ZoneSlotDiffers],
    degradations: &mut Vec<Degradation>,
) {
    let plan = page::furniture::plan(maps, degradations);
    doc.title_page |= plan.needs_title_page;
    doc.even_and_odd_headers |= plan.needs_even_odd;
    propagate_slot_parts(doc, differs);
    propagate_section_headers(doc, section_differs);
    page::furniture::attach(&plan, doc);
}

/// The heading level each margin zone queries, when C3 is active — read from
/// the source AST (the query renders per page, so nothing else in the IR
/// keeps the expression).
fn zone_heading_levels(world: &TydocxWorld, compat: CompatMode) -> page::PageZoneHeadings {
    if compat != CompatMode::C3 {
        return page::PageZoneHeadings::default();
    }
    let main = world.main_source();
    let template_names = page::extract_show_template_names_from_source(main.text());
    page::page_zone_heading_levels(main.text(), &template_names)
}

/// Wrap each paragraph's heading-query result in a `STYLEREF "Heading N"`
/// field. Pure-query zones wrap the whole paragraph; mixed zones isolate the
/// query-result span by consuming the zone's literal prefix/suffix pieces
/// (`[Report — #q]` renders `Report — Intro` → only `Intro` is wrapped, so a
/// Word field update keeps the literal). Paragraphs the span can't be
/// isolated for — a rendered literal with an empty query result, an
/// ambiguous match, non-text content, or a zone with several queries — stay
/// literal and report `styleref-mixed`. Returns whether any paragraph was
/// wrapped.
fn styleref_zone(
    part: &mut Option<HeaderFooter>,
    zref: &page::ZoneStyleref,
    zone_name: &str,
    degradations: &mut Vec<Degradation>,
) -> bool {
    let Some(part) = part else {
        return false;
    };
    let mut wrapped = false;
    let mut report = |reason: &str| {
        let message =
            format!("{zone_name}: {reason}; paragraph kept literal instead of a STYLEREF field");
        if !degradations
            .iter()
            .any(|d| d.construct == "styleref-mixed" && d.message == message)
        {
            degradations.push(Degradation::new(
                None,
                "styleref-mixed",
                DegradationAction::Approximated,
                message,
            ));
        }
    };
    if zref.multiple_queries {
        report("more than one heading query in the zone");
        return false;
    }
    for para in &mut part.paragraphs {
        if para.inlines.is_empty() {
            continue;
        }
        if !para
            .inlines
            .iter()
            .all(|i| matches!(i, InlineElement::Text(_)))
        {
            if !zref.literals.is_empty() {
                report("non-text content alongside the heading query");
            }
            continue;
        }
        let runs: Vec<Run> = para
            .inlines
            .iter()
            .filter_map(|i| match i {
                InlineElement::Text(run) => Some(run.clone()),
                _ => None,
            })
            .collect();
        let text: String = runs.iter().map(|r| r.text.as_str()).collect();
        let span = if zref.literals.is_empty() {
            Some((0, text.len()))
        } else {
            query_result_span(&text, &zref.literals).or_else(|| {
                report("heading-query result could not be isolated from literal text");
                None
            })
        };
        let Some((start, end)) = span else { continue };
        let mut prefix = splice_runs(&runs, 0, start);
        let mut suffix = splice_runs(&runs, end, text.len());
        prefix.retain(|r| !r.text.is_empty());
        suffix.retain(|r| !r.text.is_empty());
        let field_runs = splice_runs(&runs, start, end);
        para.inlines = prefix
            .into_iter()
            .map(InlineElement::Text)
            .chain(std::iter::once(InlineElement::Field {
                instruction: format!(" STYLEREF \"Heading {}\" ", zref.level),
                result: field_runs,
            }))
            .chain(suffix.into_iter().map(InlineElement::Text))
            .collect();
        wrapped = true;
    }
    wrapped
}

/// Locate the heading-query result span inside a rendered mixed-zone
/// paragraph by consuming the zone's literal pieces as prefix/suffix in
/// source order. `None` when the span can't be isolated — the rendered text
/// is itself one of the literals (empty query result on that page), no
/// literal matches, or the match leaves nothing.
fn query_result_span(text: &str, literals: &[String]) -> Option<(usize, usize)> {
    if literals.iter().any(|lit| text == *lit) {
        return None;
    }
    let (mut start, mut end) = (0usize, text.len());
    for lit in literals.iter().filter(|l| !l.is_empty()) {
        let rest = &text[start..end];
        if rest.starts_with(lit.as_str()) && start + lit.len() < end {
            start += lit.len();
        } else if rest.ends_with(lit.as_str()) && end - lit.len() > start {
            end -= lit.len();
        }
    }
    (start < end && (start > 0 || end < text.len())).then_some((start, end))
}

/// The runs covering byte span `start..end` of the concatenated run texts —
/// a run straddling a boundary is split.
fn splice_runs(runs: &[Run], start: usize, end: usize) -> Vec<Run> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    for run in runs {
        let run_end = pos + run.text.len();
        let s = start.max(pos);
        let e = end.min(run_end);
        if s < e {
            let mut piece = run.clone();
            piece.text = run.text[s - pos..e - pos].to_string();
            out.push(piece);
        }
        pos = run_end;
    }
    out
}

/// In C2/C3 a first-page or odd/even zone variant is representable, so only a
/// shape that covers none of those (an arbitrary mid-document content change)
/// reports `header-footer-variant`.
fn report_zone_unrepresentable(
    parts: &page::ZoneParts,
    zone_name: &str,
    compat: CompatMode,
    first_page: usize,
    degradations: &mut Vec<Degradation>,
) {
    if compat == CompatMode::C1 {
        return;
    }
    if let Some(page_index) = parts.unrepresentable_page {
        let message = if first_page == 1 {
            format!(
                "page {page_index} {zone_name} differs from page 1; only the first page's {zone_name} is kept"
            )
        } else {
            format!(
                "page {page_index} {zone_name} differs from page {first_page} (its section's first page); only that page's {zone_name} is kept"
            )
        };
        degradations.push(Degradation::new(
            None,
            "header-footer-variant",
            DegradationAction::Skipped,
            message,
        ));
    }
}

/// Convert an equation to OMML, forwarding the math converter's
/// dropped/approximated-construct notes as degradations (deduplicated by
/// construct + message — a document may repeat the same wrapper many times).
pub(crate) fn equation_omml(content: &Content, degradations: &mut Vec<Degradation>) -> String {
    let (omml, notes) = tydocx_math::equation_to_omml_report(content);
    let span = content.span();
    let span = span.id().map(|_| span);
    for note in notes {
        if !degradations
            .iter()
            .any(|d| d.construct == note.construct && d.message == note.message)
        {
            degradations.push(Degradation::new(
                span,
                note.construct,
                if note.dropped {
                    DegradationAction::Skipped
                } else {
                    DegradationAction::Approximated
                },
                note.message,
            ));
        }
    }
    omml
}

fn append_transform_bodies(world: &TydocxWorld, doc: &mut Document) {
    let texts = page::transform_bodies_text(world.main_source().text());
    if texts.is_empty() {
        return;
    }
    let mut para = Paragraph::new();
    para.inlines
        .push(InlineElement::Text(Run::new(texts.join(" "))));
    doc.body.elements.push(BlockElement::Paragraph(para));
}

fn c1_source_degradations(world: &TydocxWorld, degradations: &mut Vec<Degradation>) {
    let main = world.main_source();
    let template_names = page::extract_show_template_names_from_source(main.text());
    if let Some(span) = page::detect_page_setting_change(main, &template_names) {
        degradations.push(Degradation::new(
            Some(span),
            "section-break",
            DegradationAction::Skipped,
            "page columns or margins change mid-document; kept as a single section",
        ));
    }
    let overrides = page::extract_source_style_overrides(main.text(), &template_names);
    if overrides.margins_mirrored {
        degradations.push(Degradation::new(
            None,
            "mirrored-margins",
            DegradationAction::Skipped,
            "inside/outside margins are not representable without w:mirrorMargins; \
                 both sides keep the Typst default",
        ));
    }
    if overrides.numbering_changed {
        degradations.push(Degradation::new(
            None,
            "page-numbering-change",
            DegradationAction::Skipped,
            "the page numbering format or counter restart changes mid-document; \
                 kept as a single numbering scheme",
        ));
    }
    if overrides.page_furniture {
        degradations.push(Degradation::new(
            None,
            "page-foreground",
            DegradationAction::Approximated,
            "page foreground/background is not representable as anchored content; \
                 its content is kept in the page header or dropped",
        ));
    }
    if overrides.show_outline_entry_rule {
        degradations.push(Degradation::new(
            None,
            "show-outline-entry-rule",
            DegradationAction::Approximated,
            "a `show outline.entry` rule rewrites outline entries; the table of \
                 contents keeps Word's default entry formatting",
        ));
    }
    let transforms = [
        (overrides.move_call, "moved-content", "a `move` offset"),
        (
            overrides.rotate_call,
            "rotated-content",
            "a `rotate` rotation",
        ),
        (overrides.scale_call, "scaled-content", "a `scale` factor"),
        (overrides.skew_call, "skewed-content", "a `skew` shear"),
    ];
    for (present, construct, what) in transforms {
        if present {
            degradations.push(Degradation::new(
                None,
                construct,
                DegradationAction::Skipped,
                format!("{what} has no OOXML equivalent; the content is kept in place"),
            ));
        }
    }
    if overrides.show_heading_rule {
        degradations.push(Degradation::new(
            None,
            "show-heading-rule",
            DegradationAction::Approximated,
            "a `show heading` rule rewrites heading content; a heading that is not \
                 returned by the rule loses its Heading style and outline entry, while \
                 its text is kept",
        ));
    }
}

fn run_post_passes(
    world: &TydocxWorld,
    html_doc: &HtmlDocument,
    paged_doc: Option<&PagedDocument>,
    doc: &mut Document,
    compat: CompatMode,
    degradations: &mut Vec<Degradation>,
) {
    // Extract title/author from document metadata, falling back to first heading.
    extract_document_metadata(html_doc, doc);

    // Extract bibliography sources for Word citation data store.
    doc.citation_sources =
        bibliography::extract_bibliography_sources(&**html_doc.introspector(), world);

    // Suppress indent after headings and apply bibliography hanging indent.
    apply_paragraph_formatting(doc);

    // Honor `#set par(hanging-indent: …)` from the source AST. A declared
    // hanging indent governs the paragraphs that follow it; this honors an
    // author-stated value, not a genre heuristic.
    apply_hanging_indent_from_source(world, doc);

    // Apply per-run styles and heading alignment from PagedDocument.
    if let Some(paged) = paged_doc {
        page::apply_styles_from_paged(paged, doc);
    }

    // Translate the English CJK family names Typst exposes into the localized
    // name Word shows for the document's declared language.
    page::localize_cjk_fonts(world, doc);

    // Detect small caps from source text.
    apply_smallcaps_from_source(world, doc);
    apply_baseline_shifts_from_source(world, doc);

    // Recover explicit `#pagebreak()`/`#colbreak()` from the source AST.
    breaks::apply_breaks_from_source(world, doc);

    // M6 (C1 only): a mid-document columns/margins change starts a new
    // Typst section; C1 keeps a single section, so the change is dropped.
    // Size-only section breaks stay in all modes (spec-ecma376 §Page-level).
    append_transform_bodies(world, doc);
    if compat == CompatMode::C1 {
        // Report-and-no-flag: `mirrored-margins` is reported instead of
        // emitted; the flag is a C2/C3 setting.
        doc.mirror_margins = false;
        c1_source_degradations(world, degradations);
    }

    // Merge consecutive paragraphs that belong to the same visual line.
    if paged_doc.is_some() {
        coalesce::merge_same_line_paragraphs(doc);
    }

    // Coalesce adjacent equally-formatted text runs last, after every per-run
    // style patch has settled.
    coalesce::coalesce_runs(doc);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn hello_world() -> TydocxWorld {
        TydocxWorld::new(Path::new("../../tests/fixtures/hello.typ")).unwrap()
    }

    /// Byte-equality oracle for the two conversion paths on `hello`: it
    /// only holds because `hello` is in the `ir_path` IDENTICAL set. If
    /// the paths ever diverge on `hello`, this is the canary — see
    /// `crates/tydocx-cli/tests/integration/ir_path.rs`.
    #[test]
    fn export_matches_convert() {
        let world = hello_world();
        let options = ConvertOptions::default();
        let via_convert = convert(&world, &options).unwrap();
        let (html_doc, paged_doc) = compile_both_targets(&world).unwrap();
        let via_export = export(&html_doc, paged_doc.as_ref(), &world, &options);
        assert_eq!(via_convert, via_export);
    }

    #[test]
    fn export_without_paged_document_still_produces_headings() {
        let world = hello_world();
        let (html_doc, _) = compile_both_targets(&world).unwrap();
        let outcome = export(&html_doc, None, &world, &ConvertOptions::default());
        let doc = outcome.document;
        let headings = doc
            .body
            .elements
            .iter()
            .filter(|element| {
                matches!(
                    element,
                    BlockElement::Paragraph(p)
                        if matches!(p.style, Some(ParagraphStyle::Heading(_)))
                )
            })
            .count();
        assert!(
            headings >= 1,
            "expected at least one heading, got {headings}"
        );
    }
}
