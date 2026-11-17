//! Pair -> `IrDocument` collection (the realize collector).
//!
//! The main realize pairs are walked in document order. Block-level
//! elements arrive as `Tag::Start`/`Tag::End` groups; the tag's `Content`
//! is the semantic element itself (a `HeadingElem`, `TableElem`, …), so
//! no HTML projection stands between the collector and the element.
//!
//! Elements that only exist after layout (`TableElem`/`GridElem` inside a
//! figure body, or a top-level table whose grid is not yet synthesized)
//! are resolved by fragment-re-realizing the enclosing body (verification
//! loop route (a)): the fragment's tag carries the element with its `grid`
//! synthesized. Geometry, page numbers and footnote bodies still come
//! from the companion `PagedDocument` (route (b)), in the patch step —
//! never from the pairs.

use std::collections::HashMap;

use codex::numeral_systems::NamedNumeralSystem;
use typort_ir::{
    Degradation, DegradationAction, DocumentStyle, IrBibEntry, IrBlock, IrDocument, IrDocumentInfo,
    IrEnum, IrEnumItem, IrEquation, IrFigureKind, IrGrid, IrInline, IrList, IrListItem,
    IrNumberingPattern, IrNumberingPiece, IrNumeralSystem, IrParagraph, IrRaw, IrSectionBreakType,
    IrShapeKind, IrTable, IrTableCell, IrTableRow, IrVMerge, PageSettings, TableBorders,
};
use typort_ooxml::document::{Alignment, VerticalAlign};
use typst_library::diag::SourceResult;
use typst_library::engine::Engine;
use typst_library::foundations::{
    Content, Element, LocatableSelector, NativeElement, Packed, SequenceElem, Smart, StyleChain,
    StyledElem, Value,
};
use typst_library::introspection::{Location, SplitLocator, Tag, TagElem};
use typst_library::layout::grid::resolve::{Cell, CellGrid, Entry, Line};
use typst_library::layout::{
    Abs, BlockBody, BlockElem, ColbreakElem, GridElem, HAlignment, Length, Margin, PageElem,
    PagebreakElem, Parity, PlaceElem, Rel, VAlignment,
};
use typst_library::math::EquationElem;
use typst_library::model::{
    BibliographyElem, DirectLinkElem, EnumElem, EnumItem, FigureCaption, FigureElem, FigureKind,
    HeadingElem, ListElem, ListItem, ListMarker, Numbering, OutlineElem, ParElem, ParbreakElem,
    QuoteElem, TableCell, TableElem, TermItem, TermsElem,
};
use typst_library::pdf::PdfMarkerTagKind;
use typst_library::routines::Pair;
use typst_library::text::{RawElem, RawLine, SmartQuoter, SpaceElem, TextElem};
use typst_library::visualize::{
    CircleElem, CurveElem, EllipseElem, ImageElem, LineElem, PolygonElem, RectElem, SquareElem,
};
use typst_syntax::Source;
use typst_syntax::Span;

use super::inline::{
    InlineCtx, alignment_from_styles, collect_inlines, collect_paragraph, image_inline,
    smart_quotes_for,
};
use super::style::{StyleDefaults, defaults_from_root, run_from_styles};
use super::with_fragment;
use crate::convert::table_align::paint_to_shd_hex;
use crate::convert::table_width::{TableWidthCtx, track_widths_pct};

/// State returned alongside the collected body.
pub struct CollectorState {
    /// Footnote markers in document order: (marker location, rendered number).
    pub footnote_markers: Vec<(Location, String)>,
    /// Footnote entry bodies collected from content, keyed by marker
    /// location.
    pub footnote_bodies: HashMap<Location, Vec<IrParagraph>>,
    /// Whether any footnote uses circled numbering (①②③).
    pub footnote_circled: bool,
    /// Degradations recorded while collecting.
    pub degradations: Vec<Degradation>,
}

/// The collector: walks realized pairs into owned IR.
///
/// The `Engine` and `SplitLocator` are threaded as parameters (they are the
/// realize context, not walk state); the walk state itself (label maps,
/// counters, degradations) lives here.
pub struct Collector<'a> {
    defaults: StyleDefaults,
    /// Labelled-element locations -> bookmark names (pass 1).
    label_map: HashMap<Location, String>,
    /// Bibliography-entry locations -> `"bib-n"` bookmark names (pass 1).
    bib_map: HashMap<Location, String>,
    /// Bibliography entries, collected in pass 1 (fragment pairs cannot be
    /// stored: their arenas die with the fragment call).
    bib_entries: Option<Vec<IrBibEntry>>,
    /// Footnote markers in document order.
    footnote_markers: Vec<(Location, String)>,
    /// Footnote entry bodies collected from content, keyed by marker
    /// location (see the `FootnoteElem` branch in `handle_tag_start`).
    footnote_bodies: HashMap<Location, Vec<IrParagraph>>,
    /// Whether any footnote uses circled numbering (①②③).
    footnote_circled: bool,
    /// Degradations recorded while collecting.
    degradations: Vec<Degradation>,
    /// Block-equation counter (for equation numbers, mirroring the legacy
    /// `eq_state`): the count of block equations seen so far.
    eq_count: u64,
    /// The document's main source, for source-AST recovery (page-counter
    /// references): a `context` tag carries only the rendered page number,
    /// the label it refers to is read from the expression its span
    /// points at. `None` when the world has no readable main source.
    source: Option<Source>,
    /// Element identity behind layouter blocks, keyed by span. The
    /// built-in show rules rewrite `#rect`, `#line`, transforms and
    /// friends to a `BlockElem` with a private layouter callback; the
    /// result keeps the element's span, so the pre-realization content
    /// tree — indexed here — still names the element (see
    /// `collect_layouter_block`).
    layouter_index: HashMap<Span, Content>,
    /// Lifetime marker: the collector borrows nothing, but the inline
    /// contexts it hands out borrow its maps.
    _marker: std::marker::PhantomData<&'a ()>,
}

/// Collect the body blocks from the main realize pairs.
/// The document's effective body defaults from the realized pairs,
/// falling back to the root chain.
///
/// A `#set text(size: …, font: …)` at the top of a document is not in the
/// compiler root chain — it applies to the document content, so the
/// pairs' `StyleChain`s carry it. The body size/font is the text-length
/// weighted mode over paragraph pairs: body paragraphs dominate by
/// length, so heading/caption sizes (short text) cannot win.
/// Glyph budget for the body-default vote: the pairs-level analogue of
/// the legacy `BODY_SIZE_SAMPLE_PAGES` (3 pages) rule. Pairs carry no page
/// information, so the leading portion of the document is sampled by
/// glyph count instead — about three pages of body text at typical
/// density (~45 lines of ~70 glyphs per page). Back matter (appendices,
/// bibliographies) can hold most of a document's glyphs; sampling all
/// pairs lets it win the vote and misdetect the body size.
const BODY_DEFAULT_SAMPLE_GLYPHS: usize = 9_000;

fn defaults_from_pairs(pairs: &[Pair<'_>], root_styles: StyleChain<'_>) -> StyleDefaults {
    let fallback = defaults_from_root(root_styles);
    let mut sizes: HashMap<u32, usize> = HashMap::new();
    let mut first_fonts: HashMap<String, usize> = HashMap::new();
    let mut last_fonts: HashMap<String, usize> = HashMap::new();
    let mut sampled = 0usize;
    for (content, styles) in pairs {
        let Some(par) = content.to_packed::<ParElem>() else {
            continue;
        };
        let weight = plain_text(&par.body).chars().count();
        if weight == 0 {
            continue;
        }
        let size_pt = styles.resolve(TextElem::size).to_pt();
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "sizes are small and clamped to at least 1.0 first"
        )]
        let size_half_pt = (size_pt * 2.0).round().max(1.0) as u32;
        *sizes.entry(size_half_pt).or_default() += weight;
        let families: Vec<&str> = typst_library::text::families(*styles)
            .map(typst_library::text::FontFamily::as_str)
            .collect();
        if let Some(first) = families.first() {
            *first_fonts.entry((*first).to_string()).or_default() += weight;
        }
        // NOTE: the last-family vote deliberately uses `families()`
        // including Typst's emoji fallback tail (so it is constant
        // "segoe ui emoji" and no CJK run gets a spurious east-asia
        // override): taking the last DECLARED family instead was tried
        // and regressed the CJK fixtures — the legacy path emits no such
        // overrides there. See `defaults_from_root` for the declared-list
        // reasoning, which applies only to the no-pairs fallback.
        if let Some(last) = families.last() {
            *last_fonts.entry((*last).to_string()).or_default() += weight;
        }
        sampled += weight;
        if sampled >= BODY_DEFAULT_SAMPLE_GLYPHS {
            break;
        }
    }
    StyleDefaults {
        size_half_pt: sizes
            .into_iter()
            .max_by_key(|(_, weight)| *weight)
            .map_or(fallback.size_half_pt, |(size, _)| size),
        body_font: first_fonts
            .into_iter()
            .max_by_key(|(_, weight)| *weight)
            .map_or(fallback.body_font, |(font, _)| font),
        font_east_asia: last_fonts
            .into_iter()
            .max_by_key(|(_, weight)| *weight)
            .map_or(fallback.font_east_asia, |(font, _)| font),
    }
}

pub fn collect_document(
    pairs: &[Pair<'_>],
    engine: &mut Engine,
    locator: &mut SplitLocator,
    root_styles: StyleChain<'_>,
    info: &IrDocumentInfo,
    source: Option<Source>,
    original: &Content,
) -> SourceResult<(IrDocument, CollectorState)> {
    let defaults = defaults_from_pairs(pairs, root_styles);
    let mut layouter_index = HashMap::new();
    index_content_tree(original, &mut layouter_index);
    let mut collector = Collector {
        defaults,
        label_map: HashMap::new(),
        bib_map: HashMap::new(),
        bib_entries: None,
        footnote_markers: Vec::new(),
        footnote_bodies: HashMap::new(),
        footnote_circled: false,
        degradations: Vec::new(),
        eq_count: 0,
        source,
        layouter_index,
        _marker: std::marker::PhantomData,
    };
    collector.pass_one(pairs, engine, locator)?;
    let body = collector.collect_blocks(pairs, engine, locator)?;
    let state = CollectorState {
        footnote_markers: std::mem::take(&mut collector.footnote_markers),
        footnote_bodies: std::mem::take(&mut collector.footnote_bodies),
        footnote_circled: collector.footnote_circled,
        degradations: std::mem::take(&mut collector.degradations),
    };
    let doc = IrDocument {
        info: info.clone(),
        page: PageSettings::default(),
        style: DocumentStyle::default(),
        body,
        footnotes: Vec::new(),
        citation_sources: Vec::new(),
    };
    Ok((doc, state))
}

/// Build `IrDocumentInfo` from the realize `DocumentInfo` and the root
/// styles (language/region).
pub fn document_info(
    info: &typst_library::model::DocumentInfo,
    root_styles: StyleChain<'_>,
) -> IrDocumentInfo {
    let author = if info.author.is_empty() {
        None
    } else {
        Some(info.author.join(", "))
    };
    let (lang, region) = smart_quotes_for(root_styles);
    IrDocumentInfo {
        title: info.title.as_ref().map(ToString::to_string),
        author,
        lang: Some(lang.as_str().to_string()),
        region: region.map(|r| r.as_str().to_string()),
    }
}

/// The list context in force while collecting one block body: the depth
/// at which a list found here sits, and the enum numbering pattern
/// inherited from an outer enum (its pieces + the depth at which it was
/// introduced). A fresh block scope (table cell, quote, figure, block)
/// collects at depth 0 with no inherited pattern; a list/enum item body
/// collects one deeper, inheriting its parent's pattern.
#[derive(Debug, Clone, Default)]
struct ListCtx {
    depth: u8,
    inherited: Option<(IrNumberingPattern, u8)>,
}

/// Per-figure metadata carried through body collection: the semantic
/// kind, the label's bookmark name, and the C1 custom-placement flag.
struct FigureMeta {
    kind: IrFigureKind,
    label: Option<String>,
    custom_placement: bool,
}

impl Collector<'_> {
    /// Pass 1: build the label map (every labelled tag's location ->
    /// bookmark name) and collect the bibliography (entries + the
    /// location -> `"bib-n"` map). Runs before the main walk so forward
    /// references resolve.
    fn pass_one(
        &mut self,
        pairs: &[Pair<'_>],
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<()> {
        // Labels: scan the pairs (top-level and nested) for tag contents
        // carrying a label.
        let mut i = 0;
        while i < pairs.len() {
            let (content, _) = pairs[i];
            if let Some(tagged) = content.to_packed::<TagElem>() {
                if let Tag::Start(inner, _) = &tagged.tag {
                    if let Some(location) = inner.location()
                        && let Some(label) = inner.label()
                    {
                        self.label_map
                            .entry(location)
                            .or_insert_with(|| label.resolve().to_string());
                    }
                    collect_nested_labels(inner, &mut self.label_map);
                }
            } else {
                collect_nested_labels(content, &mut self.label_map);
            }
            i += 1;
        }
        // Bibliography: find the BibliographyElem tag group, fragment its
        // body, and collect the entries (bodies included) now — the
        // fragment pairs cannot be stored.
        if let Some((group, styles)) = find_tag_group(pairs, BibliographyElem::ELEM) {
            let entries = self.collect_bibliography_entries(&group, styles, engine, locator)?;
            if entries.is_empty()
                && !self
                    .degradations
                    .iter()
                    .any(|d| d.construct == "bibliography-entries")
            {
                // The fragment of a `BibliographyElem` yields only its
                // heading: the show-rule entry grid is not in the pairs,
                // so the entries (and their "bib-n" bookmarks, which
                // citations link to) are missing. Entry extraction from
                // the companion Paged frames is a follow-up.
                self.degradations.push(Degradation {
                    span: None,
                    construct: "bibliography-entries",
                    action: DegradationAction::Skipped,
                    message: "the bibliography entries could not be read from the fragment realization; the bibliography renders as an empty field"
                        .to_string(),
                });
            }
            self.bib_entries = Some(entries);
        }
        Ok(())
    }

    /// Fresh inline context for one paragraph body.
    fn inline_ctx(&mut self) -> InlineCtx<'_> {
        InlineCtx {
            defaults: &self.defaults,
            quoter: SmartQuoter::new(),
            prev_char: None,
            label_map: &self.label_map,
            bib_map: &self.bib_map,
            footnote_markers: &mut self.footnote_markers,
            degradations: &mut self.degradations,
            source: self.source.as_ref(),
            block_math_as_inline: false,
            footnote_bodies: &mut self.footnote_bodies,
            footnote_circled: &mut self.footnote_circled,
            layouter_index: &self.layouter_index,
        }
    }

    /// Walk realized pairs into blocks.
    fn collect_blocks(
        &mut self,
        pairs: &[Pair<'_>],
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Vec<IrBlock>> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < pairs.len() {
            let (content, styles) = pairs[i];
            if let Some(tagged) = content.to_packed::<TagElem>() {
                match &tagged.tag {
                    Tag::Start(inner, _) => {
                        let end = find_end(pairs, i + 1, inner.location());
                        let group = &pairs[i + 1..end];
                        out.extend(self.collect_tag_group(inner, group, styles, engine, locator)?);
                        i = end + 1;
                    }
                    Tag::End(_, _, _) => {
                        i += 1;
                    }
                }
                continue;
            }
            out.extend(self.collect_standalone(content, styles, engine, locator)?);
            i += 1;
        }
        Ok(out)
    }

    /// Collect the blocks of one tag group (the pairs between a `Tag::Start`
    /// and its matching `Tag::End`).
    fn collect_tag_group(
        &mut self,
        inner: &Content,
        group: &[Pair<'_>],
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Vec<IrBlock>> {
        if let Some(heading) = inner.to_packed::<HeadingElem>() {
            return Ok(vec![self.collect_heading(
                heading, inner, group, styles, engine, locator,
            )?]);
        }
        if let Some(figure) = inner.to_packed::<FigureElem>() {
            return Ok(vec![
                self.collect_figure(figure, inner, group, styles, engine, locator)?,
            ]);
        }
        if inner.to_packed::<TableElem>().is_some() {
            return Ok(vec![self.collect_table(inner, styles, engine, locator)?]);
        }
        if inner.to_packed::<GridElem>().is_some() {
            return Ok(vec![
                self.collect_grid_elem(inner, styles, engine, locator)?,
            ]);
        }
        if let Some(list) = inner.to_packed::<ListElem>() {
            return Ok(vec![self.collect_list(list, 0, styles, engine, locator)?]);
        }
        if let Some(enumer) = inner.to_packed::<EnumElem>() {
            return Ok(vec![
                self.collect_enum(enumer, 0, None, styles, engine, locator)?,
            ]);
        }
        if let Some(terms) = inner.to_packed::<TermsElem>() {
            return self.collect_terms(terms, styles, engine, locator);
        }
        if let Some(equation) = inner.to_packed::<EquationElem>() {
            return Ok(self.collect_equation(equation, inner, styles));
        }
        if inner.to_packed::<BibliographyElem>().is_some() {
            let entries = self.bib_entries.clone().unwrap_or_default();
            return Ok(vec![IrBlock::Bibliography { entries }]);
        }
        if let Some(raw) = inner.to_packed::<RawElem>() {
            return Ok(vec![self.collect_raw(raw, inner, styles, engine, locator)?]);
        }
        if let Some(quote) = inner.to_packed::<QuoteElem>() {
            return self.collect_quote(quote, styles, engine, locator);
        }
        if let Some(outline) = inner.to_packed::<OutlineElem>() {
            return Ok(vec![
                self.collect_outline(outline, styles, engine, locator)?,
            ]);
        }
        if let Some(place) = inner.to_packed::<PlaceElem>() {
            return Ok(vec![self.collect_place(place, styles, engine, locator)?]);
        }
        if let Some(image) = inner.to_packed::<ImageElem>() {
            // A standalone image: its own paragraph holding the image.
            // The tag group's `BlockElem` only carries the image in a
            // `SingleLayouter` capture, so the element comes from the
            // tag inner, not the group.
            return Ok(self.image_paragraph(image, styles, engine));
        }
        if let Some(par) = inner.to_packed::<ParElem>() {
            let mut ctx = self.inline_ctx();
            let para = collect_paragraph(&par.body, styles, engine, locator, &mut ctx)?;
            return Ok(vec![IrBlock::Paragraph(para)]);
        }
        // Any other tag group (a list item, a PDF marker tag, …): its
        // content is either consumed by the enclosing element's handler or
        // is itself block content — recurse into the group's pairs.
        self.collect_blocks(group, engine, locator)
    }

    /// One image element as its own paragraph (or nothing, when the image
    /// cannot be decoded — the degradation, if any, is recorded by
    /// `image_inline`).
    fn image_paragraph(
        &mut self,
        image: &Packed<ImageElem>,
        styles: StyleChain<'_>,
        engine: &mut Engine,
    ) -> Vec<IrBlock> {
        let mut ctx = self.inline_ctx();
        match image_inline(image, styles, engine, &mut ctx) {
            Some(inline) => vec![IrBlock::Paragraph(IrParagraph {
                inlines: vec![inline],
                ..IrParagraph::default()
            })],
            None => Vec::new(),
        }
    }

    /// Collect a pair that is not inside a tag group.
    fn collect_standalone(
        &mut self,
        content: &Content,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Vec<IrBlock>> {
        if let Some(par) = content.to_packed::<ParElem>() {
            let mut ctx = self.inline_ctx();
            let para = collect_paragraph(&par.body, styles, engine, locator, &mut ctx)?;
            return Ok(vec![IrBlock::Paragraph(para)]);
        }
        if let Some(image) = content.to_packed::<ImageElem>() {
            return Ok(self.image_paragraph(image, styles, engine));
        }
        if let Some(block) = content.to_packed::<BlockElem>() {
            if let Some(body) = block_body_content(block, styles) {
                return self.collect_content_blocks(body, styles, engine, locator);
            }
            return self.collect_layouter_block(content, styles, engine, locator);
        }
        if let Some(pagebreak) = content.to_packed::<PagebreakElem>() {
            return Ok(vec![Self::collect_pagebreak(pagebreak, styles)]);
        }
        if content.to_packed::<ColbreakElem>().is_some() {
            // A column break on its own: a paragraph holding just the
            // break (the lowering emits `w:br type="column"` for it).
            // Whether it appears in the pairs at all is unverified;
            // harmless if it never does. `weak` is not carried: the IR
            // has no weak column break, and the legacy path emits the
            // break unconditionally too.
            return Ok(vec![IrBlock::Paragraph(IrParagraph {
                inlines: vec![IrInline::ColumnBreak],
                ..IrParagraph::default()
            })]);
        }
        if let Some(place) = content.to_packed::<PlaceElem>() {
            return Ok(vec![self.collect_place(place, styles, engine, locator)?]);
        }
        if let Some(text) = content.to_packed::<TextElem>() {
            // Bare text at the top level (no enclosing paragraph): wrap it.
            let mut para = IrParagraph::default();
            let run = run_from_styles(&text.text, styles, &self.defaults, Some(content.span()));
            para.inlines.push(IrInline::Text(run));
            return Ok(vec![IrBlock::Paragraph(para)]);
        }
        if let Some((kind, shape_body)) = shape_parts(content) {
            let body = match shape_body {
                Some(shape_content) => {
                    self.collect_content_blocks(&shape_content, styles, engine, locator)?
                }
                None => Vec::new(),
            };
            return Ok(vec![IrBlock::Shape {
                kind,
                span: {
                    let span = content.span();
                    span.id().map(|_| span)
                },
                body,
            }]);
        }
        if content.to_packed::<LineElem>().is_some() {
            return Ok(Vec::new());
        }
        Ok(Vec::new())
    }

    /// A `BlockElem` whose body is a layouter capture (`SingleLayouter`/
    /// `MultiLayouter`): the element it rewrites — a `#rect`, `#line`,
    /// `#pad`, transform, … — is private inside the callback, but the
    /// block keeps the element's span, so the original element comes
    /// back from the pre-realization index. Shapes keep their body text;
    /// everything else drops as before (a `#line` has no IR form, and
    /// layout-only wrappers have no OOXML meaning).
    fn collect_layouter_block(
        &mut self,
        content: &Content,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Vec<IrBlock>> {
        let Some(original) = self.layouter_index.get(&content.span()).cloned() else {
            return Ok(Vec::new());
        };
        if let Some((kind, shape_body)) = shape_parts(&original) {
            let body = match shape_body {
                Some(shape_content) => {
                    self.collect_content_blocks(&shape_content, styles, engine, locator)?
                }
                None => Vec::new(),
            };
            return Ok(vec![IrBlock::Shape {
                kind,
                span: {
                    let span = original.span();
                    span.id().map(|_| span)
                },
                body,
            }]);
        }
        Ok(Vec::new())
    }

    /// Collect blocks from a `Content` body (a block body, a table cell, a
    /// list item, …): walk the children, accumulating consecutive inline
    /// children into one paragraph.
    ///
    /// A fresh block scope: lists found here sit at depth 0 with no
    /// inherited pattern.
    fn collect_content_blocks(
        &mut self,
        content: &Content,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Vec<IrBlock>> {
        self.collect_body_blocks(content, &ListCtx::default(), styles, engine, locator)
    }

    /// Collect blocks from a `Content` body with an explicit list context
    /// (see `ListCtx`).
    ///
    /// Typst parses a nested list as bare `ListItem`/`EnumItem` children
    /// of the outer item's body — there is no enclosing `ListElem`/
    /// `EnumElem` in the source AST (the enclosing element only appears
    /// in the realized fragment's `MultiLayouter` capture). Consecutive
    /// bare items are grouped here into one `IrBlock::List`/`IrBlock::Enum`
    /// at the context depth; without this grouping the items fall into
    /// the pending inline run and are lost when the body is
    /// fragment-realized.
    fn collect_body_blocks(
        &mut self,
        content: &Content,
        list_ctx: &ListCtx,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Vec<IrBlock>> {
        let children = flatten_sequences(content_children(content));
        let mut out = Vec::new();
        let mut pending: Vec<Content> = Vec::new();
        let mut i = 0;
        while i < children.len() {
            let child = children[i];
            if let Some(list) = child.to_packed::<ListElem>() {
                flush_pending(&mut pending, &mut out, self, styles, engine, locator)?;
                out.push(self.collect_list(list, list_ctx.depth, styles, engine, locator)?);
                i += 1;
            } else if let Some(enumer) = child.to_packed::<EnumElem>() {
                flush_pending(&mut pending, &mut out, self, styles, engine, locator)?;
                out.push(self.collect_enum(
                    enumer,
                    list_ctx.depth,
                    list_ctx.inherited.as_ref(),
                    styles,
                    engine,
                    locator,
                )?);
                i += 1;
            } else if child.to_packed::<ListItem>().is_some() {
                flush_pending(&mut pending, &mut out, self, styles, engine, locator)?;
                let (run, next) = take_item_run(&children, i, true);
                out.push(self.collect_bare_list(&run, list_ctx, styles, engine, locator)?);
                i = next;
            } else if child.to_packed::<EnumItem>().is_some() {
                flush_pending(&mut pending, &mut out, self, styles, engine, locator)?;
                let (run, next) = take_item_run(&children, i, false);
                out.push(self.collect_bare_enum(&run, list_ctx, styles, engine, locator)?);
                i = next;
            } else if child.to_packed::<TermItem>().is_some() {
                flush_pending(&mut pending, &mut out, self, styles, engine, locator)?;
                let (run, next, wide) = take_term_item_run(&children, i);
                out.extend(self.collect_bare_terms(&run, !wide, styles, engine, locator)?);
                i = next;
            } else if let Some((kind, shape_body)) = shape_parts(child) {
                flush_pending(&mut pending, &mut out, self, styles, engine, locator)?;
                let body = match shape_body {
                    Some(content) => {
                        self.collect_content_blocks(&content, styles, engine, locator)?
                    }
                    None => Vec::new(),
                };
                out.push(IrBlock::Shape {
                    kind,
                    span: {
                        let span = child.span();
                        span.id().map(|_| span)
                    },
                    body,
                });
                i += 1;
            } else if child.to_packed::<LineElem>().is_some() {
                // A drawn `#line` has no text and no IR form; the
                // horizontal-rule recovery that reads it from Paged
                // geometry is a legacy-only pass (fallback registry).
                flush_pending(&mut pending, &mut out, self, styles, engine, locator)?;
                i += 1;
            } else if is_block_element(child) {
                flush_pending(&mut pending, &mut out, self, styles, engine, locator)?;
                out.extend(self.collect_content_element(child, styles, engine, locator)?);
                i += 1;
            } else if is_inline_element(child) {
                pending.push(child.clone());
                i += 1;
            } else if child.to_packed::<ParbreakElem>().is_some() {
                // A paragraph break in a flat inline sequence is a
                // paragraph boundary: table cell bodies (and similar
                // bodies) arrive as `Sequence [Text, Parbreak, Text]`
                // with no `ParElem` wrappers, so without this the two
                // paragraphs would merge into one.
                flush_pending(&mut pending, &mut out, self, styles, engine, locator)?;
                i += 1;
            } else {
                // Spacing and tag elements at block level are skipped:
                // their text (if any) is already in the fragment's text
                // pairs.
                i += 1;
            }
        }
        flush_pending(&mut pending, &mut out, self, styles, engine, locator)?;
        Ok(out)
    }

    /// A run of bare `ListItem` children as one `IrBlock::List`: the
    /// marker list in force comes from the `StyleChain` (a
    /// `#set list(marker: …)` applies there, as it does to a `ListElem`'s
    /// own marker field).
    fn collect_bare_list(
        &mut self,
        run: &[&Content],
        list_ctx: &ListCtx,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<IrBlock> {
        let depth = list_ctx.depth;
        let marker = list_marker_from_styles(styles, depth);
        let mut items = Vec::new();
        for child in run {
            let Some(item) = child.to_packed::<ListItem>() else {
                continue;
            };
            let body = self.collect_body_blocks(
                &item.body,
                &ListCtx {
                    depth: depth.saturating_add(1),
                    inherited: None,
                },
                styles,
                engine,
                locator,
            )?;
            items.push(IrListItem {
                marker: marker.clone(),
                body,
            });
        }
        Ok(IrBlock::List(IrList {
            items,
            depth,
            custom_marker: styles.get_cloned(ListElem::marker)
                != crate::convert::lists::default_list_marker(),
        }))
    }

    /// A run of bare `EnumItem` children as one `IrBlock::Enum`,
    /// mirroring `collect_enum`: the pattern in force is the inherited
    /// pattern, else the numbering in force from the `StyleChain`
    /// (a closure numbering keeps the inherited-or-empty pattern, as in
    /// `collect_enum`).
    fn collect_bare_enum(
        &mut self,
        run: &[&Content],
        list_ctx: &ListCtx,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<IrBlock> {
        let depth = list_ctx.depth;
        let (pattern, origin) = match &list_ctx.inherited {
            Some((pattern, origin)) => (pattern.clone(), *origin),
            None => match enum_numbering_from_styles(styles) {
                Some(pattern) => (pattern, depth),
                None => (
                    IrNumberingPattern {
                        pieces: Vec::new(),
                        suffix: String::new(),
                        full: styles.get(EnumElem::full),
                    },
                    depth,
                ),
            },
        };
        let start = match styles.get(EnumElem::start) {
            Smart::Custom(n) => u32::try_from(n).unwrap_or(1).max(1),
            Smart::Auto => 1,
        };
        let mut items = Vec::new();
        let mut number = u64::from(start);
        for child in run {
            let Some(item) = child.to_packed::<EnumItem>() else {
                continue;
            };
            let item_number = match item.number.get(styles) {
                Smart::Custom(n) => n,
                Smart::Auto => number,
            };
            number = item_number + 1;
            let number_text = enum_number_text(&pattern, origin, depth, item_number);
            let body = self.collect_body_blocks(
                &item.body,
                &ListCtx {
                    depth: depth.saturating_add(1),
                    inherited: Some((pattern.clone(), origin)),
                },
                styles,
                engine,
                locator,
            )?;
            items.push(IrEnumItem { number_text, body });
        }
        Ok(IrBlock::Enum(IrEnum {
            items,
            numbering: pattern,
            start,
            depth,
            reversed: styles.get(EnumElem::reversed),
        }))
    }

    /// Collect one block-level child `Content` (a heading, table, list, …)
    /// found inside a block body.
    fn collect_content_element(
        &mut self,
        child: &Content,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Vec<IrBlock>> {
        if let Some(heading) = child.to_packed::<HeadingElem>() {
            return Ok(vec![
                self.collect_heading_elem(heading, child, styles, engine, locator)?,
            ]);
        }
        if child.to_packed::<TableElem>().is_some() {
            return Ok(vec![self.collect_table(child, styles, engine, locator)?]);
        }
        if child.to_packed::<GridElem>().is_some() {
            return Ok(vec![
                self.collect_grid_elem(child, styles, engine, locator)?,
            ]);
        }
        if let Some(list) = child.to_packed::<ListElem>() {
            return Ok(vec![self.collect_list(list, 0, styles, engine, locator)?]);
        }
        if let Some(enumer) = child.to_packed::<EnumElem>() {
            return Ok(vec![
                self.collect_enum(enumer, 0, None, styles, engine, locator)?,
            ]);
        }
        if let Some(terms) = child.to_packed::<TermsElem>() {
            return self.collect_terms(terms, styles, engine, locator);
        }
        if let Some(figure) = child.to_packed::<FigureElem>() {
            return Ok(vec![self.collect_figure_from_elem(
                figure, child, styles, engine, locator,
            )?]);
        }
        if let Some(equation) = child.to_packed::<EquationElem>() {
            return self.collect_equation_elem(equation, child, styles, engine, locator);
        }
        if let Some(raw) = child.to_packed::<RawElem>() {
            return Ok(vec![self.collect_raw(raw, child, styles, engine, locator)?]);
        }
        if let Some(quote) = child.to_packed::<QuoteElem>() {
            return self.collect_quote(quote, styles, engine, locator);
        }
        if let Some(outline) = child.to_packed::<OutlineElem>() {
            let title = self.outline_title(outline, styles, engine, locator)?;
            return Ok(vec![IrBlock::Outline {
                title,
                entries: Vec::new(),
                depth: outline_depth(outline),
                custom_target: outline_custom_target(outline, styles),
                custom_indent: outline_custom_indent(outline),
            }]);
        }
        if let Some(place) = child.to_packed::<PlaceElem>() {
            return Ok(vec![self.collect_place(place, styles, engine, locator)?]);
        }
        if let Some(pagebreak) = child.to_packed::<PagebreakElem>() {
            return Ok(vec![Self::collect_pagebreak(pagebreak, styles)]);
        }
        if child.to_packed::<ColbreakElem>().is_some() {
            return Ok(vec![IrBlock::Paragraph(IrParagraph {
                inlines: vec![IrInline::ColumnBreak],
                ..IrParagraph::default()
            })]);
        }
        if let Some((kind, shape_body)) = shape_parts(child) {
            let body = match shape_body {
                Some(content) => self.collect_content_blocks(&content, styles, engine, locator)?,
                None => Vec::new(),
            };
            return Ok(vec![IrBlock::Shape {
                kind,
                span: {
                    let span = child.span();
                    span.id().map(|_| span)
                },
                body,
            }]);
        }
        if child.to_packed::<LineElem>().is_some() {
            return Ok(Vec::new());
        }
        if let Some(block) = child.to_packed::<BlockElem>() {
            if let Some(body) = block_body_content(block, styles) {
                return self.collect_content_blocks(body, styles, engine, locator);
            }
            return self.collect_layouter_block(child, styles, engine, locator);
        }
        if let Some(par) = child.to_packed::<ParElem>() {
            let mut ctx = self.inline_ctx();
            let para = collect_paragraph(&par.body, styles, engine, locator, &mut ctx)?;
            return Ok(vec![IrBlock::Paragraph(para)]);
        }
        Ok(Vec::new())
    }

    /// A bare `HeadingElem` found inside a block body (outside a tag
    /// group — e.g. a heading synthesized by a show rule's body).
    fn collect_heading_elem(
        &mut self,
        heading: &HeadingElem,
        child: &Content,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<IrBlock> {
        let numbering_text = heading_numbers(heading);
        let level = heading.resolve_level(styles).get();
        let level = u8::try_from(level.min(255)).unwrap_or(u8::MAX);
        let mut ctx = self.inline_ctx();
        let mut body = collect_inlines(&heading.body, styles, engine, locator, &mut ctx)?;
        if let Some(number) = &numbering_text {
            strip_leading_text(&mut body, number);
            strip_leading_text(&mut body, " ");
        }
        Ok(IrBlock::Heading {
            level,
            numbering_text,
            body,
            label: child.label().map(|l| l.resolve().to_string()),
            alignment: alignment_from_styles(styles),
        })
    }

    /// A bare `EquationElem` inside a block body: block equations keep
    /// their OMML and number; an inline equation at block level becomes
    /// a paragraph.
    fn collect_equation_elem(
        &mut self,
        equation: &EquationElem,
        child: &Content,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Vec<IrBlock>> {
        if equation.block.get(styles) {
            self.eq_count += 1;
            let omml = typort_math::equation_to_omml(child);
            let number = self.equation_number(equation, styles);
            return Ok(vec![IrBlock::Equation(IrEquation {
                omml,
                number,
                bookmark: child.label().map(|l| l.resolve().to_string()),
            })]);
        }
        let mut ctx = self.inline_ctx();
        let inlines = collect_inlines(child, styles, engine, locator, &mut ctx)?;
        Ok(vec![IrBlock::Paragraph(IrParagraph {
            inlines,
            ..IrParagraph::default()
        })])
    }

    /// A heading tag group: the block pair holds the rendered heading
    /// (number + title text); the `HeadingElem` holds the level, the
    /// numbering and the label.
    fn collect_heading(
        &mut self,
        heading: &HeadingElem,
        inner: &Content,
        group: &[Pair<'_>],
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<IrBlock> {
        let level = heading.resolve_level(styles).get();
        let level = u8::try_from(level.min(255)).unwrap_or(u8::MAX);
        let numbering_text = heading_numbers(heading);
        // The heading body: prefer the block pair's rendered text (it is
        // what Typst shows, including the number); fall back to the
        // element's body.
        let mut body: Vec<IrInline> = Vec::new();
        // Alignment comes from the body pair's chain, not the heading
        // element's own: a `show heading` rule centers the body
        // (`align(center, it.body)` or `set align(center)`), so the
        // `AlignElem` is in scope for the body's pairs only.
        let mut alignment = alignment_from_styles(styles);
        for (content, pair_styles) in group {
            if let Some(block) = content.to_packed::<BlockElem>()
                && let Some(body_content) = block_body_content(block, *pair_styles)
            {
                alignment = alignment_from_styles(*pair_styles);
                let mut ctx = self.inline_ctx();
                body = collect_inlines(body_content, *pair_styles, engine, locator, &mut ctx)?;
                if let Some(number) = &numbering_text {
                    strip_leading_text(&mut body, number);
                    strip_leading_text(&mut body, " ");
                }
                break;
            }
            if let Some(par) = content.to_packed::<ParElem>() {
                alignment = alignment_from_styles(*pair_styles);
                let mut ctx = self.inline_ctx();
                let para = collect_paragraph(&par.body, *pair_styles, engine, locator, &mut ctx)?;
                body = para.inlines;
                if let Some(number) = &numbering_text {
                    strip_leading_text(&mut body, number);
                    strip_leading_text(&mut body, " ");
                }
                break;
            }
        }
        if body.is_empty() {
            let mut ctx = self.inline_ctx();
            body = collect_inlines(&heading.body, styles, engine, locator, &mut ctx)?;
        }
        Ok(IrBlock::Heading {
            level,
            numbering_text,
            body,
            label: inner.label().map(|l| l.resolve().to_string()),
            alignment,
        })
    }

    /// A figure tag group: fragment the figure body (route (a)) so tables
    /// resolve and the caption renders in full, then split the fragment
    /// pairs into body blocks and the caption.
    fn collect_figure(
        &mut self,
        figure: &FigureElem,
        inner: &Content,
        group: &[Pair<'_>],
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<IrBlock> {
        let meta = FigureMeta {
            kind: figure_kind(figure, styles),
            label: inner.label().map(|l| l.resolve().to_string()),
            custom_placement: figure_custom_placement(figure),
        };
        // The figure body is the block pair's `BlockElem` body. A placed
        // figure (`placement: top/bottom`) arrives wrapped in a
        // `PlaceElem` whose body is that block; unwrap it so the caption
        // tag inside is found instead of falling back to the element's
        // own (caption-less) body.
        for (content, pair_styles) in group {
            let rendered = if let Some(block) = content.to_packed::<BlockElem>() {
                block_body_content(block, *pair_styles)
            } else if let Some(place) = content.to_packed::<PlaceElem>()
                && let Some(block) = place.body.to_packed::<BlockElem>()
            {
                block_body_content(block, *pair_styles)
            } else {
                None
            };
            if let Some(body_content) = rendered {
                return self.collect_figure_body(
                    &meta,
                    body_content,
                    *pair_styles,
                    engine,
                    locator,
                );
            }
        }
        // No block pair: fall back to the element's own body.
        self.collect_figure_body(&meta, &figure.body, styles, engine, locator)
    }

    /// A figure found as a `Content` child (not a tag group): fragment
    /// the whole figure element, not just its body — the caption is added
    /// by the figure's show rule, so fragmenting the body alone never
    /// sees the `FigureCaption` tag (placed figures lost their captions
    /// this way). The pair walk below re-enters the tag-group figure
    /// path for the rendered body.
    fn collect_figure_from_elem(
        &mut self,
        figure: &FigureElem,
        inner: &Content,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<IrBlock> {
        let meta = FigureMeta {
            kind: figure_kind(figure, styles),
            label: inner.label().map(|l| l.resolve().to_string()),
            custom_placement: figure_custom_placement(figure),
        };
        with_fragment(engine, locator, inner, styles, |engine, locator, pairs| {
            self.collect_figure_pairs(pairs, &meta, engine, locator)
        })
    }

    /// Fragment a figure body and split the fragment pairs into body blocks
    /// and the caption.
    ///
    /// The caption inlines keep the rendered number (`"Table 1: …"`): the
    /// lowering emits them as-is and never re-adds a number.
    fn collect_figure_body(
        &mut self,
        meta: &FigureMeta,
        body_content: &Content,
        body_styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<IrBlock> {
        with_fragment(
            engine,
            locator,
            body_content,
            body_styles,
            |engine, locator, pairs| self.collect_figure_pairs(pairs, meta, engine, locator),
        )
    }

    /// Walk fragment pairs into figure body blocks and the caption.
    fn collect_figure_pairs(
        &mut self,
        pairs: &[Pair<'_>],
        meta: &FigureMeta,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<IrBlock> {
        let mut caption: Vec<IrInline> = Vec::new();
        let mut caption_alignment: Option<Alignment> = None;
        let mut supplement: Option<String> = None;
        let mut caption_first = false;
        let mut body: Vec<IrBlock> = Vec::new();
        let mut seen_body = false;
        let mut i = 0;
        while i < pairs.len() {
            let (content, pair_styles) = pairs[i];
            if let Some(tagged) = content.to_packed::<TagElem>()
                && let Tag::Start(inner, _) = &tagged.tag
            {
                let end = find_end(pairs, i + 1, inner.location());
                let group = &pairs[i + 1..end];
                if let Some(caption_elem) = inner.to_packed::<FigureCaption>() {
                    if !seen_body {
                        caption_first = true;
                    }
                    // The caption supplement is a synthesized
                    // `Option<Content>`; the elem macro stores it as
                    // `Option<Option<Content>>` (see `table.grid`).
                    supplement = match &caption_elem.supplement {
                        Some(Some(content)) => {
                            let text = plain_text(content);
                            (!text.is_empty()).then_some(text)
                        }
                        _ => None,
                    };
                    for (c, s) in group {
                        if let Some(block) = c.to_packed::<BlockElem>()
                            && let Some(body_c) = block_body_content(block, *s)
                        {
                            if caption_alignment.is_none() {
                                caption_alignment = alignment_from_styles(*s);
                            }
                            let mut ctx = self.inline_ctx();
                            caption = collect_inlines(body_c, *s, engine, locator, &mut ctx)?;
                        }
                    }
                } else {
                    let blocks =
                        self.collect_tag_group(inner, group, pair_styles, engine, locator)?;
                    if !blocks.is_empty() {
                        seen_body = true;
                    }
                    body.extend(blocks);
                }
                i = end + 1;
                continue;
            }
            let blocks = self.collect_standalone(content, pair_styles, engine, locator)?;
            if !blocks.is_empty() {
                seen_body = true;
            }
            body.extend(blocks);
            i += 1;
        }
        Ok(IrBlock::Figure {
            body,
            caption,
            caption_alignment,
            kind: meta.kind,
            supplement,
            label: meta.label.clone(),
            caption_first,
            custom_placement: meta.custom_placement,
        })
    }

    /// A list tag group (or a `ListElem` child): items from the element's
    /// children; markers from the element's marker list at this depth.
    fn collect_list(
        &mut self,
        list: &ListElem,
        depth: u8,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<IrBlock> {
        let marker = list_marker_at(list, depth, styles);
        let mut items = Vec::new();
        for item in &list.children {
            // The item body is a list context one deeper: nested lists
            // and enums found here sit at `depth + 1`.
            let body = self.collect_body_blocks(
                &item.body,
                &ListCtx {
                    depth: depth.saturating_add(1),
                    inherited: None,
                },
                styles,
                engine,
                locator,
            )?;
            items.push(IrListItem {
                marker: marker.clone(),
                body,
            });
        }
        Ok(IrBlock::List(IrList {
            items,
            depth,
            custom_marker: list.marker.get_cloned(styles)
                != crate::convert::lists::default_list_marker(),
        }))
    }

    /// A term-list tag group: items from the element's children. The show
    /// rule renders the term bold (`term.strong()`), so the term runs are
    /// forced bold here; the separator (default a 0.6em space) sits between
    /// term and description. Wide lists (`tight: false`, items separated by
    /// blank lines) put the term on its own paragraph with the description
    /// after it — as the legacy path does; tight lists keep term, separator
    /// and description in one paragraph.
    fn collect_terms(
        &mut self,
        terms: &TermsElem,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Vec<IrBlock>> {
        let tight = terms.tight.get(styles);
        let separator = terms.separator.get_ref(styles);
        let mut out = Vec::new();
        for item in &terms.children {
            out.extend(self.collect_term_item(
                &item.term,
                &item.description,
                tight,
                Some(separator),
                styles,
                engine,
                locator,
            )?);
        }
        Ok(out)
    }

    /// One term item: bold term + separator + description (tight), or a
    /// term paragraph followed by the description blocks (wide). The show
    /// rule renders the term bold (`term.strong()`), so the term runs are
    /// forced bold here.
    #[allow(
        clippy::too_many_arguments,
        reason = "one item's inputs; the caller sites pass them through"
    )]
    fn collect_term_item(
        &mut self,
        term: &Content,
        description: &Content,
        tight: bool,
        separator: Option<&Content>,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Vec<IrBlock>> {
        let mut ctx = self.inline_ctx();
        let mut term_inlines = collect_inlines(term, styles, engine, locator, &mut ctx)?;
        bold_inlines(&mut term_inlines);
        let desc_blocks = self.collect_body_blocks(
            description,
            &ListCtx {
                depth: 0,
                inherited: None,
            },
            styles,
            engine,
            locator,
        )?;
        if tight {
            let mut sep_inlines = match separator {
                Some(separator) => {
                    let mut ctx = self.inline_ctx();
                    collect_inlines(separator, styles, engine, locator, &mut ctx)?
                }
                None => Vec::new(),
            };
            if sep_inlines.is_empty() {
                // The default separator is a bare horizontal space, which
                // the inline walk drops: keep one space so the term and the
                // description do not run together.
                sep_inlines.push(IrInline::Space);
            }
            let mut inlines = term_inlines;
            inlines.extend(sep_inlines);
            let mut desc_blocks = desc_blocks;
            if let Some(IrBlock::Paragraph(para)) = desc_blocks.first_mut() {
                inlines.extend(std::mem::take(&mut para.inlines));
                desc_blocks.remove(0);
            }
            let mut out = vec![IrBlock::Paragraph(IrParagraph {
                inlines,
                ..IrParagraph::default()
            })];
            out.extend(desc_blocks);
            Ok(out)
        } else {
            let mut out = vec![IrBlock::Paragraph(IrParagraph {
                inlines: term_inlines,
                ..IrParagraph::default()
            })];
            out.extend(desc_blocks);
            Ok(out)
        }
    }

    /// A run of bare `TermItem` children as term blocks: nested term lists
    /// arrive this way inside a description (the parser emits the items
    /// directly, not a nested `TermsElem`). Tightness comes from the
    /// separators in the run: a blank line (parbreak) between two items
    /// makes it wide.
    fn collect_bare_terms(
        &mut self,
        run: &[&Content],
        tight: bool,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Vec<IrBlock>> {
        let separator = styles.get_ref(TermsElem::separator);
        let mut out = Vec::new();
        for child in run {
            let Some(item) = child.to_packed::<TermItem>() else {
                continue;
            };
            out.extend(self.collect_term_item(
                &item.term,
                &item.description,
                tight,
                Some(separator),
                styles,
                engine,
                locator,
            )?);
        }
        Ok(out)
    }

    /// An enum tag group (or an `EnumElem` child).
    ///
    /// `inherited` carries the pattern in force from an outer enum (its
    /// pieces + the depth at which it was introduced), mirroring the legacy
    /// `ListWalker`'s `PatternState`.
    fn collect_enum(
        &mut self,
        enumer: &EnumElem,
        depth: u8,
        inherited: Option<&(IrNumberingPattern, u8)>,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<IrBlock> {
        let numbering = enum_numbering(enumer, styles);
        let full = enumer.full.get(styles);
        let (pattern, origin) = match &numbering {
            Some(pattern) => (pattern.clone(), depth),
            None => match inherited {
                Some((pattern, origin)) => (pattern.clone(), *origin),
                None => (
                    IrNumberingPattern {
                        pieces: Vec::new(),
                        suffix: String::new(),
                        full,
                    },
                    depth,
                ),
            },
        };
        let start = match enumer.start.get(styles) {
            Smart::Custom(n) => u32::try_from(n).unwrap_or(1).max(1),
            Smart::Auto => 1,
        };
        let mut items = Vec::new();
        let mut number = u64::from(start);
        for item in &enumer.children {
            let item_number = match item.number.get(styles) {
                Smart::Custom(n) => n,
                Smart::Auto => number,
            };
            number = item_number + 1;
            let number_text = enum_number_text(&pattern, origin, depth, item_number);
            // Nested lists/enums inside the item inherit this enum's
            // pattern (with its origin).
            let body = self.collect_enum_item_body(
                &item.body,
                depth,
                Some(&(pattern.clone(), origin)),
                styles,
                engine,
                locator,
            )?;
            items.push(IrEnumItem { number_text, body });
        }
        Ok(IrBlock::Enum(IrEnum {
            items,
            numbering: pattern,
            start,
            depth,
            reversed: matches!(enumer.reversed.as_option(), Some(true)),
        }))
    }

    /// Collect an enum item's body, threading the inherited pattern into
    /// nested enums.
    fn collect_enum_item_body(
        &mut self,
        content: &Content,
        depth: u8,
        inherited: Option<&(IrNumberingPattern, u8)>,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Vec<IrBlock>> {
        // The item body is a list context one deeper, inheriting this
        // enum's pattern (with its origin).
        self.collect_body_blocks(
            content,
            &ListCtx {
                depth: depth.saturating_add(1),
                inherited: inherited.map(|(pattern, origin)| (pattern.clone(), *origin)),
            },
            styles,
            engine,
            locator,
        )
    }

    /// A table element (from a tag group or a content child): use the
    /// tag's element when its grid is already synthesized, otherwise
    /// fragment-re-realize it (route (a)) for the synthesized grid, and
    /// fall back to the declared children when even that fails.
    fn collect_table(
        &mut self,
        content: &Content,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<IrBlock> {
        let Some(table) = content.to_packed::<TableElem>() else {
            self.degradations.push(Degradation {
                span: None,
                construct: "table-not-a-table",
                action: DegradationAction::Approximated,
                message: "a table collection was given content that is not a table; kept as an empty table"
                    .to_string(),
            });
            return Ok(IrBlock::Table(IrTable {
                rows: Vec::new(),
                column_widths_pct: None,
                borders: None,
                gutter: false,
            }));
        };
        if let Some(grid) = table.grid.as_ref()
            && !grid.entries.is_empty()
        {
            return Ok(IrBlock::Table(
                self.cell_grid_to_table(grid, table, styles, engine, locator)?,
            ));
        }
        let mut found: Option<IrBlock> = None;
        with_fragment(
            engine,
            locator,
            content,
            styles,
            |engine, locator, pairs| {
                for (frag_content, frag_styles) in pairs {
                    if let Some(tagged) = frag_content.to_packed::<TagElem>()
                        && let Tag::Start(inner, _) = &tagged.tag
                        && let Some(frag_table) = inner.to_packed::<TableElem>()
                        && let Some(frag_grid) = frag_table.grid.as_ref()
                        && !frag_grid.entries.is_empty()
                    {
                        found = Some(IrBlock::Table(self.cell_grid_to_table(
                            frag_grid,
                            frag_table,
                            *frag_styles,
                            engine,
                            locator,
                        )?));
                        break;
                    }
                }
                Ok(())
            },
        )?;
        if let Some(block) = found {
            return Ok(block);
        }
        Ok(IrBlock::Table(self.unresolved_table_to_table(
            table, styles, engine, locator,
        )?))
    }

    /// A grid element: same shape as [`Self::collect_table`], via the
    /// grid's own synthesized `CellGrid`.
    fn collect_grid_elem(
        &mut self,
        content: &Content,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<IrBlock> {
        let Some(grid) = content.to_packed::<GridElem>() else {
            self.degradations.push(Degradation {
                span: None,
                construct: "grid-not-a-grid",
                action: DegradationAction::Approximated,
                message:
                    "a grid collection was given content that is not a grid; kept as an empty grid"
                        .to_string(),
            });
            return Ok(IrBlock::Grid(IrGrid {
                rows: Vec::new(),
                column_widths_pct: None,
                borders: Some(TableBorders::default()),
            }));
        };
        if let Some(cell_grid) = grid.grid.as_ref()
            && !cell_grid.entries.is_empty()
        {
            return Ok(IrBlock::Grid(
                self.cell_grid_to_grid(cell_grid, grid, styles, engine, locator)?,
            ));
        }
        let mut found: Option<IrBlock> = None;
        with_fragment(
            engine,
            locator,
            content,
            styles,
            |engine, locator, pairs| {
                for (frag_content, frag_styles) in pairs {
                    if let Some(tagged) = frag_content.to_packed::<TagElem>()
                        && let Tag::Start(inner, _) = &tagged.tag
                        && let Some(frag_grid) = inner.to_packed::<GridElem>()
                        && let Some(frag_cell_grid) = frag_grid.grid.as_ref()
                        && !frag_cell_grid.entries.is_empty()
                    {
                        found = Some(IrBlock::Grid(self.cell_grid_to_grid(
                            frag_cell_grid,
                            frag_grid,
                            *frag_styles,
                            engine,
                            locator,
                        )?));
                        break;
                    }
                }
                Ok(())
            },
        )?;
        if let Some(block) = found {
            return Ok(block);
        }
        self.degradations.push(Degradation {
            span: None,
            construct: "grid-unresolved",
            action: DegradationAction::Approximated,
            message: "a grid arrived with an empty cell grid; kept as an empty table".to_string(),
        });
        Ok(IrBlock::Grid(IrGrid {
            rows: Vec::new(),
            column_widths_pct: None,
            borders: Some(TableBorders::default()),
        }))
    }

    /// Build an `IrTable` from a resolved `CellGrid` (route (a)).
    fn cell_grid_to_table(
        &mut self,
        grid: &CellGrid,
        table: &TableElem,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<IrTable> {
        let (rows, col_count) = self.cell_grid_rows(grid, styles, engine, locator)?;
        let column_widths_pct = self.table_column_widths(table, col_count, styles);
        Ok(IrTable {
            rows,
            column_widths_pct,
            borders: table_borders_from_grid(grid),
            gutter: table_gutter(table, styles),
        })
    }

    /// Build an `IrGrid` from a resolved `CellGrid`.
    fn cell_grid_to_grid(
        &mut self,
        grid: &CellGrid,
        grid_elem: &GridElem,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<IrGrid> {
        let (rows, col_count) = self.cell_grid_rows(grid, styles, engine, locator)?;
        let column_widths_pct = self.grid_column_widths(grid_elem, col_count, styles);
        Ok(IrGrid {
            rows,
            column_widths_pct,
            borders: table_borders_from_grid(grid),
        })
    }

    /// Rows of a resolved `CellGrid`, in physical coordinates: `entry`
    /// takes gutter-inclusive coordinates and returns `None` for gutter
    /// slots, so gutter rows come out empty and are skipped. A spanning
    /// cell's covered slots arrive as `Entry::Merged` and become explicit
    /// `vMerge: Continue` cells — the lowering does not synthesize them.
    fn cell_grid_rows(
        &mut self,
        grid: &CellGrid,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<(Vec<IrTableRow>, usize)> {
        let col_count = grid.non_gutter_column_count();
        let mut rows = Vec::new();
        for y in 0..grid.rows.len() {
            // `rows.len()` is the logical row index: gutter rows push
            // nothing, so only real rows advance it — and the header
            // ranges are logical too.
            let is_header = grid
                .headers
                .iter()
                .any(|header| header.range.contains(&rows.len()));
            let mut cells = Vec::new();
            let mut x = 0;
            while x < grid.cols.len() {
                match grid.entry(x, y) {
                    Some(Entry::Cell(cell)) => {
                        cells.push(
                            self.resolved_cell_to_ir(cell, is_header, styles, engine, locator)?,
                        );
                    }
                    Some(Entry::Merged { parent }) => {
                        // `parent` is a flat index into the logical grid
                        // (`parent_cell_position`); a merged slot whose
                        // parent is in the same logical row is this row's
                        // colspan continuation (already emitted with the
                        // cell) — only a parent in an earlier row is a
                        // rowspan continuation needing an explicit cell.
                        let parent_row = parent.checked_div(col_count).unwrap_or(rows.len());
                        if parent_row < rows.len() {
                            cells.push(IrTableCell {
                                body: Vec::new(),
                                colspan: 1,
                                rowspan: 1,
                                vmerge: IrVMerge::Continue,
                                fill_hex: None,
                                vertical_align: None,
                                inset_twips: None,
                            });
                        }
                    }
                    None => {}
                }
                x += 1;
            }
            if !cells.is_empty() {
                rows.push(IrTableRow { cells });
            }
        }
        Ok((rows, col_count))
    }

    /// One resolved grid cell -> `IrTableCell`.
    ///
    /// `is_header` marks a cell in one of the grid's header ranges: the
    /// table header's show rule renders header cells bold, but that bold
    /// is not in the cell's own styles — it is applied at layout, so the
    /// collector forces it here.
    fn resolved_cell_to_ir(
        &mut self,
        cell: &Cell,
        is_header: bool,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<IrTableCell> {
        let table_cell = cell.body.to_packed::<TableCell>();
        let fill_hex = cell.fill.as_ref().and_then(paint_to_shd_hex).or_else(|| {
            table_cell.and_then(|tc| match tc.fill.get_cloned(styles) {
                Smart::Custom(Some(paint)) => paint_to_shd_hex(&paint),
                _ => None,
            })
        });
        let vertical_align = table_cell
            .and_then(|tc| match tc.align.get_cloned(styles) {
                Smart::Custom(align) => align.y(),
                Smart::Auto => None,
            })
            .map(|v| match v {
                VAlignment::Top => VerticalAlign::Top,
                VAlignment::Horizon => VerticalAlign::Center,
                VAlignment::Bottom => VerticalAlign::Bottom,
            });
        let horizontal_align = table_cell
            .and_then(|tc| match tc.align.get_cloned(styles) {
                Smart::Custom(align) => align.x(),
                Smart::Auto => None,
            })
            .map(|h| match h {
                HAlignment::Left | HAlignment::Start => Alignment::Left,
                HAlignment::Center => Alignment::Center,
                HAlignment::Right | HAlignment::End => Alignment::Right,
            });
        let inset_twips = table_cell
            .and_then(|tc| match tc.inset.get_cloned(styles) {
                Smart::Custom(sides) => sides
                    .left
                    .map(|rel| rel.abs.at(Abs::pt(11.0)).to_pt() * 20.0),
                Smart::Auto => None,
            })
            .map(|pt| {
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "insets are small non-negative lengths"
                )]
                let twips = pt.round().max(0.0) as u32;
                twips
            });
        let body_content = table_cell.map_or_else(|| cell.body.clone(), |tc| tc.body.clone());
        let mut body = self.collect_content_blocks(&body_content, styles, engine, locator)?;
        // The cell's horizontal alignment applies to its paragraphs.
        if let Some(align) = horizontal_align {
            for block in &mut body {
                if let IrBlock::Paragraph(para) = block {
                    para.alignment = Some(align);
                }
            }
        }
        if is_header {
            bold_header_cell_body(&mut body);
        }
        Ok(IrTableCell {
            body,
            colspan: u32::try_from(cell.colspan.get()).unwrap_or(u32::MAX),
            rowspan: u32::try_from(cell.rowspan.get()).unwrap_or(u32::MAX),
            vmerge: if cell.rowspan.get() > 1 {
                IrVMerge::Restart
            } else {
                IrVMerge::None
            },
            fill_hex,
            vertical_align,
            inset_twips,
        })
    }

    /// Column-width percentages for a table, from its declared tracks.
    fn table_column_widths(
        &self,
        table: &TableElem,
        col_count: usize,
        styles: StyleChain<'_>,
    ) -> Option<Vec<u32>> {
        let tracks = table.columns.get_ref(styles);
        let ctx = self.width_ctx(styles);
        let mut widths = track_widths_pct(&tracks.0, ctx)?;
        // The declared tracks may cover fewer columns than the grid (e.g.
        // `columns: 2` with a merged cell spanning a third); pad with the
        // writer's equal distribution by repeating the last share.
        if widths.len() < col_count {
            let last = widths.last().copied().unwrap_or(0);
            widths.resize(col_count, last);
        }
        Some(widths)
    }

    /// Column-width percentages for a grid, from its declared tracks.
    fn grid_column_widths(
        &self,
        grid: &GridElem,
        col_count: usize,
        styles: StyleChain<'_>,
    ) -> Option<Vec<u32>> {
        let tracks = grid.columns.get_ref(styles);
        let ctx = self.width_ctx(styles);
        let mut widths = track_widths_pct(&tracks.0, ctx)?;
        if widths.len() < col_count {
            let last = widths.last().copied().unwrap_or(0);
            widths.resize(col_count, last);
        }
        Some(widths)
    }

    /// The width context for track resolution: page content width (pt) from
    /// the root styles, body font size (pt).
    fn width_ctx(&self, styles: StyleChain<'_>) -> TableWidthCtx {
        TableWidthCtx {
            content_pt: page_content_width_pt(styles),
            body_font_pt: f64::from(self.defaults.size_half_pt) / 2.0,
        }
    }

    /// A table whose `grid` was not synthesized (route (a) did not reach
    /// it): collect from the declared children, with per-cell properties
    /// from the `TableCell` fields when set. Closure-valued fills/aligns
    /// cannot be resolved here; those cells keep no fill/alignment.
    fn unresolved_table_to_table(
        &mut self,
        table: &TableElem,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<IrTable> {
        self.degradations.push(Degradation {
            span: None,
            construct: "table-grid-unresolved",
            action: DegradationAction::Approximated,
            message:
                "a table arrived without a resolved cell grid; collected from its declared cells"
                    .to_string(),
        });
        let mut rows: Vec<IrTableRow> = Vec::new();
        let mut current: Vec<IrTableCell> = Vec::new();
        let mut col_count = 0usize;
        for child in &table.children {
            match child {
                typst_library::model::TableChild::Header(header) => {
                    for item in &header.children {
                        if let typst_library::model::TableItem::Cell(cell) = item {
                            current.push(self.declared_cell_to_ir(cell, styles, engine, locator)?);
                        }
                    }
                    if !current.is_empty() {
                        col_count = col_count.max(current.len());
                        rows.push(IrTableRow {
                            cells: std::mem::take(&mut current),
                        });
                    }
                }
                typst_library::model::TableChild::Footer(_) => {}
                typst_library::model::TableChild::Item(item) => {
                    if let typst_library::model::TableItem::Cell(cell) = item {
                        current.push(self.declared_cell_to_ir(cell, styles, engine, locator)?);
                    }
                }
            }
        }
        if !current.is_empty() {
            col_count = col_count.max(current.len());
            rows.push(IrTableRow { cells: current });
        }
        let column_widths_pct = self.table_column_widths(table, col_count, styles);
        Ok(IrTable {
            rows,
            column_widths_pct,
            borders: None,
            gutter: table_gutter(table, styles),
        })
    }

    /// One declared (unresolved) table cell -> `IrTableCell`.
    fn declared_cell_to_ir(
        &mut self,
        cell: &typst_library::model::TableCell,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<IrTableCell> {
        let fill_hex = match cell.fill.get_cloned(styles) {
            Smart::Custom(Some(paint)) => paint_to_shd_hex(&paint),
            _ => None,
        };
        let body = self.collect_content_blocks(&cell.body, styles, engine, locator)?;
        Ok(IrTableCell {
            body,
            colspan: u32::try_from(cell.colspan.get(styles).get()).unwrap_or(u32::MAX),
            rowspan: u32::try_from(cell.rowspan.get(styles).get()).unwrap_or(u32::MAX),
            vmerge: IrVMerge::None,
            fill_hex,
            vertical_align: None,
            inset_twips: None,
        })
    }

    /// A block equation tag group: the `EquationElem` (block) plus its
    /// number, from the equation counter and the element's numbering.
    fn collect_equation(
        &mut self,
        equation: &EquationElem,
        inner: &Content,
        styles: StyleChain<'_>,
    ) -> Vec<IrBlock> {
        if !equation.block.get(styles) {
            return Vec::new();
        }
        self.eq_count += 1;
        let omml = typort_math::equation_to_omml(inner);
        let number = self.equation_number(equation, styles);
        vec![IrBlock::Equation(IrEquation {
            omml,
            number,
            bookmark: inner.label().map(|l| l.resolve().to_string()),
        })]
    }

    /// The rendered number of a block equation, from its numbering pattern
    /// and the equation counter (mirroring the legacy
    /// `compute_equation_number`).
    fn equation_number(&self, equation: &EquationElem, styles: StyleChain<'_>) -> Option<String> {
        let Some(Numbering::Pattern(pattern)) = equation.numbering.get_cloned(styles) else {
            return None;
        };
        let nums = vec![self.eq_count];
        pattern.apply(None, &nums).ok().map(|s| s.to_string())
    }

    /// A raw (code) block: one `IrRaw`, lines from the element's
    /// synthesized `RawLine`s (plain `text` + syntax-highlighted `body`).
    ///
    /// The fragment realize of a block raw yields only a `BlockElem`
    /// holding the lines — no text pairs — so the lines are read here and
    /// each line's highlighted body is fragment-realized for styled runs
    /// (the syntax-theme `#Set` colours become run attributes). A line
    /// whose body yields nothing keeps its plain text.
    fn collect_raw(
        &mut self,
        raw: &RawElem,
        content: &Content,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<IrBlock> {
        let mut lines: Vec<Vec<typort_ooxml::document::Run>> = Vec::new();
        if let Some(synthesized) = raw.lines.as_ref() {
            lines = self.collect_raw_lines(synthesized, styles, engine, locator)?;
        }
        if lines.is_empty() {
            // No synthesized lines on the element itself (an unrealized
            // element, e.g. a code block inside a resolved table cell):
            // the fragment realization synthesizes them — the fragment
            // of a block raw is a single `BlockElem` whose body holds
            // the realized `RawElem` with its lines.
            let frag_lines = with_fragment(
                engine,
                locator,
                content,
                styles,
                |engine, locator, pairs| {
                    for (pair_content, _) in pairs {
                        if let Some(synthesized) = find_synthesized_raw_lines(pair_content) {
                            return self.collect_raw_lines(&synthesized, styles, engine, locator);
                        }
                    }
                    Ok(Vec::new())
                },
            )?;
            lines = frag_lines;
        }
        if lines.is_empty() {
            // Still no lines (an inline raw, or an unrealized element
            // whose fragment yielded no `RawElem`): fall back to
            // fragment-realizing the element and splitting at line
            // breaks.
            let mut ctx = self.inline_ctx();
            let inlines = collect_inlines(content, styles, engine, locator, &mut ctx)?;
            let mut split: Vec<Vec<typort_ooxml::document::Run>> = vec![Vec::new()];
            for inline in inlines {
                match inline {
                    IrInline::Text(run) => {
                        if let Some(line) = split.last_mut() {
                            line.push(run);
                        }
                    }
                    IrInline::LineBreak => split.push(Vec::new()),
                    _ => {}
                }
            }
            // Drop a trailing empty line from a final line break.
            if split.len() > 1 && split.last().is_some_and(Vec::is_empty) {
                split.pop();
            }
            lines = split;
        }
        Ok(IrBlock::Raw(IrRaw {
            lines,
            language: raw.lang.get_cloned(styles).map(|l| l.to_string()),
        }))
    }

    /// Collect runs for synthesized `RawLine`s (plain `text` +
    /// syntax-highlighted `body`): each line's highlighted body is
    /// fragment-realized for styled runs, falling back to the plain
    /// text when a line yields no runs.
    fn collect_raw_lines(
        &mut self,
        synthesized: &[Packed<RawLine>],
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Vec<Vec<typort_ooxml::document::Run>>> {
        let mut lines: Vec<Vec<typort_ooxml::document::Run>> = Vec::new();
        for line in synthesized {
            let mut ctx = self.inline_ctx();
            let inlines = collect_inlines(&line.body, styles, engine, locator, &mut ctx)?;
            let mut runs = Vec::new();
            for inline in inlines {
                if let IrInline::Text(run) = inline {
                    runs.push(run);
                }
            }
            if runs.is_empty() && !line.text.is_empty() {
                runs.push(run_from_styles(&line.text, styles, &self.defaults, None));
            }
            lines.push(runs);
        }
        Ok(lines)
    }

    /// A quote: the `QuoteElem` body collected as blocks, plus its
    /// attribution (if any) as a separate plain paragraph after the
    /// quote — the legacy path emits `— Author` unindented, and Typst's
    /// own `Attribution::realize` renders it as an em dash plus the
    /// attribution content (or a citation, for a label attribution).
    fn collect_quote(
        &mut self,
        quote: &QuoteElem,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Vec<IrBlock>> {
        let body = self.collect_content_blocks(&quote.body, styles, engine, locator)?;
        let mut out = vec![IrBlock::Quote { body }];
        if let Some(Some(attribution)) = quote.attribution.as_option() {
            let realized = attribution.realize(Span::detached());
            let mut blocks = self.collect_content_blocks(&realized, styles, engine, locator)?;
            out.append(&mut blocks);
        }
        Ok(out)
    }

    /// An outline tag group: title from the element, entries from the
    /// block pair's rendered outline (best effort — the lowering emits a
    /// TOC field and ignores the entries for now).
    fn collect_outline(
        &mut self,
        outline: &OutlineElem,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<IrBlock> {
        let title = self.outline_title(outline, styles, engine, locator)?;
        Ok(IrBlock::Outline {
            title,
            entries: Vec::new(),
            depth: outline_depth(outline),
            custom_target: outline_custom_target(outline, styles),
            custom_indent: outline_custom_indent(outline),
        })
    }

    /// An outline's title, as inlines.
    fn outline_title(
        &mut self,
        outline: &OutlineElem,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Vec<IrInline>> {
        match outline.title.get_cloned(styles) {
            Smart::Custom(Some(title)) => {
                let mut ctx = self.inline_ctx();
                collect_inlines(&title, styles, engine, locator, &mut ctx)
            }
            _ => Ok(Vec::new()),
        }
    }

    /// A place: body collected as blocks. The lowering records the
    /// degradation (position is not preserved); the collector does not
    /// double-report it.
    fn collect_place(
        &mut self,
        place: &PlaceElem,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<IrBlock> {
        let body = self.collect_content_blocks(&place.body, styles, engine, locator)?;
        Ok(IrBlock::Place {
            float: place.float.get(styles),
            body,
        })
    }

    /// A page break: weak breaks are flow boundaries (the lowering emits
    /// nothing for them); a `to:` break becomes a section break.
    fn collect_pagebreak(pagebreak: &PagebreakElem, styles: StyleChain<'_>) -> IrBlock {
        let to = pagebreak.to.get_cloned(styles).map(|to| match to {
            Parity::Odd => IrSectionBreakType::Odd,
            Parity::Even => IrSectionBreakType::Even,
        });
        IrBlock::PageBreak {
            weak: pagebreak.weak.get(styles),
            to,
        }
    }

    /// Bibliography entries from a fragment of the `BibliographyElem`
    /// body: the fragment's grid has one row per entry; the marker cell
    /// holds a `DirectLinkElem` (whose `loc` is the entry's location) and
    /// the text cell holds a `PdfMarkerTag` (`BibEntry`) with the rendered
    /// entry text.
    fn collect_bibliography_entries(
        &mut self,
        group: &[Pair<'_>],
        _styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Vec<IrBibEntry>> {
        // The bibliography block pair is a `BlockElem` whose body is the
        // `BibliographyElem`; fragment it to get the rendered grid.
        let mut entries = Vec::new();
        for (content, pair_styles) in group {
            let Some(block) = content.to_packed::<BlockElem>() else {
                continue;
            };
            let Some(body_content) = block_body_content(block, *pair_styles) else {
                continue;
            };
            with_fragment(
                engine,
                locator,
                body_content,
                *pair_styles,
                |engine, locator, pairs| {
                    for (frag_content, frag_styles) in pairs {
                        if let Some(grid) = frag_content.to_packed::<GridElem>() {
                            entries =
                                self.bib_entries_from_grid(grid, *frag_styles, engine, locator)?;
                            break;
                        }
                        if let Some(block) = frag_content.to_packed::<BlockElem>()
                            && let Some(body_c) = block_body_content(block, *frag_styles)
                            && let Some(grid) = find_grid_in_content(body_c)
                        {
                            entries =
                                self.bib_entries_from_grid(grid, *frag_styles, engine, locator)?;
                            break;
                        }
                    }
                    Ok(())
                },
            )?;
            break;
        }
        Ok(entries)
    }

    /// Bibliography entries from a resolved bibliography grid.
    fn bib_entries_from_grid(
        &mut self,
        grid: &GridElem,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Vec<IrBibEntry>> {
        let Some(cell_grid) = grid.grid.as_ref() else {
            return Ok(Vec::new());
        };
        let mut entries = Vec::new();
        for py in 0..cell_grid.rows.len() {
            let mut marker: Option<String> = None;
            let mut marker_loc: Option<Location> = None;
            let mut body: Vec<IrInline> = Vec::new();
            let mut any = false;
            for px in 0..cell_grid.cols.len() {
                let Some(Entry::Cell(cell)) = cell_grid.entry(px, py) else {
                    continue;
                };
                any = true;
                let Some(table_cell) = cell.body.to_packed::<TableCell>() else {
                    continue;
                };
                if let Some(direct) = find_direct_link(&table_cell.body) {
                    marker = Some(plain_text(&direct.body));
                    marker_loc = Some(direct.loc);
                } else if let Some(tagged) =
                    find_pdf_marker(&table_cell.body, &PdfMarkerTagKind::BibEntry)
                {
                    let mut ctx = self.inline_ctx();
                    body = collect_inlines(&tagged.body, styles, engine, locator, &mut ctx)?;
                }
            }
            if !any {
                continue;
            }
            let bookmark = format!("bib-{}", entries.len());
            if let Some(loc) = marker_loc {
                self.bib_map.insert(loc, bookmark.clone());
            }
            entries.push(IrBibEntry {
                bookmark,
                marker,
                body,
            });
        }
        Ok(entries)
    }
}

/// Flush pending inline children as one paragraph.
fn flush_pending(
    pending: &mut Vec<Content>,
    out: &mut Vec<IrBlock>,
    collector: &mut Collector<'_>,
    styles: StyleChain<'_>,
    engine: &mut Engine,
    locator: &mut SplitLocator,
) -> SourceResult<()> {
    if pending.is_empty() {
        return Ok(());
    }
    let sequence = Content::sequence(std::mem::take(pending));
    let mut ctx = collector.inline_ctx();
    let inlines = collect_inlines(&sequence, styles, engine, locator, &mut ctx)?;
    if !inlines.is_empty() {
        out.push(IrBlock::Paragraph(IrParagraph {
            inlines,
            ..IrParagraph::default()
        }));
    }
    Ok(())
}

/// Find synthesized `RawLine`s in a content tree: either a realized
/// `RawElem` whose `lines` are present, or bare `RawLine` elements — the
/// fragment of a block raw is a `BlockElem` whose body holds the lines
/// directly, not wrapped in a `RawElem`. Used by the `collect_raw`
/// fallback.
fn find_synthesized_raw_lines(content: &Content) -> Option<Vec<Packed<RawLine>>> {
    let mut found = Vec::new();
    collect_raw_lines_in(content, &mut found);
    if found.is_empty() { None } else { Some(found) }
}

fn collect_raw_lines_in(content: &Content, out: &mut Vec<Packed<RawLine>>) {
    if let Some(line) = content.to_packed::<RawLine>() {
        out.push(line.clone());
        return;
    }
    if let Some(raw) = content.to_packed::<RawElem>()
        && let Some(lines) = raw.lines.as_ref()
    {
        out.extend(lines.iter().cloned());
        return;
    }
    // `content_children` returns the content itself for leaf elements,
    // so skip that case or the recursion never terminates.
    for child in content_children(content) {
        if std::ptr::eq(child, content) {
            continue;
        }
        collect_raw_lines_in(child, out);
    }
}

/// Recursively flatten `SequenceElem` children: a sequence is transparent
/// grouping, so its children are siblings. Without this, a block element
/// nested in a sequence child (e.g. a display equation inside a `#body`
/// substitution) is swallowed into the pending inline run and lost — the
/// fragment collector drops block equations.
fn flatten_sequences(children: Vec<&Content>) -> Vec<&Content> {
    let mut out = Vec::new();
    for child in children {
        if let Some(sequence) = child.to_packed::<SequenceElem>() {
            out.extend(flatten_sequences(sequence.children.iter().collect()));
        } else {
            out.push(child);
        }
    }
    out
}

/// The children of a `Content` body: a sequence's children, a styled
/// element's child, a paragraph/block body, or the content itself.
fn content_children(content: &Content) -> Vec<&Content> {
    if let Some(sequence) = content.to_packed::<SequenceElem>() {
        return sequence.children.iter().collect();
    }
    if let Some(styled) = content.to_packed::<StyledElem>() {
        return vec![&styled.child];
    }
    if let Some(par) = content.to_packed::<ParElem>() {
        return content_children(&par.body);
    }
    if let Some(block) = content.to_packed::<BlockElem>() {
        // No styles here (this is a pure structural helper): read the
        // explicitly-set body. A block body is never set through styles,
        // so the explicit value is the value.
        if let Some(Some(BlockBody::Content(body))) = block.body.as_option() {
            return content_children(body);
        }
        return Vec::new();
    }
    vec![content]
}

/// Index the pre-realization content tree by span. The built-in show
/// rules erase element identity (a `RectElem` becomes a `BlockElem` with
/// a private layouter capture) but copy the element's span onto the
/// result, so a span lookup restores it. Fields are walked generically
/// via `Content::fields()`, so nested bodies (`rect.body`, figure
/// bodies, styled children, …) are covered without per-element cases.
fn index_content_tree(content: &Content, out: &mut HashMap<Span, Content>) {
    let span = content.span();
    if span.id().is_some() {
        out.entry(span).or_insert_with(|| content.clone());
    }
    for (_, value) in content.fields() {
        index_value(&value, out);
    }
}

/// Recurse into `Content`, `Array`, and `Dict` field values.
fn index_value(value: &Value, out: &mut HashMap<Span, Content>) {
    match value {
        Value::Content(inner) => index_content_tree(inner, out),
        Value::Array(items) => {
            for item in items {
                index_value(item, out);
            }
        }
        Value::Dict(dict) => {
            for (_, item) in dict {
                index_value(item, out);
            }
        }
        _ => {}
    }
}

/// A pure shape element (`#rect`, `#circle`, `#curve`, …): its geometry
/// kind and inner content, when it carries one (`rect`/`square`/
/// `ellipse`/`circle` have `body`; `path`/`polygon`/`curve` do not).
/// `#line` is deliberately not here — it is dropped, not gated.
pub(super) fn shape_parts(content: &Content) -> Option<(IrShapeKind, Option<Content>)> {
    if let Some(rect) = content.to_packed::<RectElem>() {
        return Some((
            IrShapeKind::Rectangle,
            rect.body.as_option().clone().flatten(),
        ));
    }
    if let Some(square) = content.to_packed::<SquareElem>() {
        return Some((
            IrShapeKind::Rectangle,
            square.body.as_option().clone().flatten(),
        ));
    }
    if let Some(ellipse) = content.to_packed::<EllipseElem>() {
        return Some((
            IrShapeKind::Curve,
            ellipse.body.as_option().clone().flatten(),
        ));
    }
    if let Some(circle) = content.to_packed::<CircleElem>() {
        return Some((
            IrShapeKind::Curve,
            circle.body.as_option().clone().flatten(),
        ));
    }
    if content.to_packed::<PolygonElem>().is_some() || content.to_packed::<CurveElem>().is_some() {
        return Some((IrShapeKind::Curve, None));
    }
    None
}

/// Whether a `Content` child is a block-level element (headed for its own
/// `IrBlock`) rather than inline content for the pending paragraph.
fn is_block_element(content: &Content) -> bool {
    content.to_packed::<HeadingElem>().is_some()
        || content.to_packed::<TableElem>().is_some()
        || content.to_packed::<GridElem>().is_some()
        || content.to_packed::<ListElem>().is_some()
        || content.to_packed::<ListItem>().is_some()
        || content.to_packed::<EnumElem>().is_some()
        || content.to_packed::<EnumItem>().is_some()
        || content.to_packed::<TermsElem>().is_some()
        || content.to_packed::<FigureElem>().is_some()
        || content.to_packed::<EquationElem>().is_some_and(|eq| {
            // No styles here: read the explicitly-set flag. The parser sets
            // `block` explicitly for display equations; unset means inline.
            matches!(eq.block.as_option(), Some(true))
        })
        || content.to_packed::<RawElem>().is_some()
        || content.to_packed::<QuoteElem>().is_some()
        || content.to_packed::<OutlineElem>().is_some()
        || content.to_packed::<BibliographyElem>().is_some()
        || content.to_packed::<PlaceElem>().is_some()
        || content.to_packed::<PagebreakElem>().is_some()
        || content.to_packed::<ColbreakElem>().is_some()
        || content.to_packed::<BlockElem>().is_some()
        || content.to_packed::<ParElem>().is_some()
        || shape_parts(content).is_some()
        || content.to_packed::<LineElem>().is_some()
}

/// Whether a `Content` child is inline content for the pending paragraph.
fn is_inline_element(content: &Content) -> bool {
    // Everything that is not a block element and not a tag/spacing element.
    !is_block_element(content)
        && content.to_packed::<TagElem>().is_none()
        && content
            .to_packed::<typst_library::layout::VElem>()
            .is_none()
        && content.to_packed::<ParbreakElem>().is_none()
}

/// The maximal run of bare list/enum items starting at `start`, skipping
/// `SpaceElem`/`ParbreakElem` separators between items. Returns the run
/// and the index just past it.
/// A run of bare `TermItem` children, skipping space/parbreak
/// separators; also reports whether a blank line separated two items (a
/// wide list). A parbreak only counts when another item follows it.
fn take_term_item_run<'a>(
    children: &[&'a Content],
    start: usize,
) -> (Vec<&'a Content>, usize, bool) {
    let mut run = Vec::new();
    let mut wide = false;
    let mut i = start;
    while i < children.len() {
        let child = children[i];
        if child.to_packed::<TermItem>().is_some() {
            run.push(child);
            i += 1;
        } else if child.to_packed::<SpaceElem>().is_some() {
            i += 1;
        } else if child.to_packed::<ParbreakElem>().is_some() {
            // Wide only when another item follows this break.
            let mut j = i + 1;
            while j < children.len()
                && (children[j].to_packed::<SpaceElem>().is_some()
                    || children[j].to_packed::<ParbreakElem>().is_some())
            {
                j += 1;
            }
            if j < children.len() && children[j].to_packed::<TermItem>().is_some() {
                wide = true;
            }
            i += 1;
        } else {
            break;
        }
    }
    (run, i, wide)
}

fn take_item_run<'a>(
    children: &[&'a Content],
    start: usize,
    is_list: bool,
) -> (Vec<&'a Content>, usize) {
    let mut run = Vec::new();
    let mut i = start;
    while i < children.len() {
        let child = children[i];
        let matches = if is_list {
            child.to_packed::<ListItem>().is_some()
        } else {
            child.to_packed::<EnumItem>().is_some()
        };
        if matches {
            run.push(child);
            i += 1;
        } else if child.to_packed::<SpaceElem>().is_some()
            || child.to_packed::<ParbreakElem>().is_some()
        {
            // A separator between items: skip it; the run ends at the
            // next non-item, non-separator child.
            i += 1;
        } else {
            break;
        }
    }
    (run, i)
}

/// The body `Content` of a `BlockElem`, when it is plain content.
pub(super) fn block_body_content<'a>(
    block: &'a BlockElem,
    styles: StyleChain<'a>,
) -> Option<&'a Content> {
    match block.body.get_ref(styles) {
        Some(BlockBody::Content(content)) => Some(content),
        _ => None,
    }
}

/// Find a `GridElem` inside a `Content` (a bibliography block body).
fn find_grid_in_content(content: &Content) -> Option<&GridElem> {
    content.to_packed::<GridElem>().map(|packed| &**packed)
}

/// The index of the `Tag::End` matching the `Tag::Start` at `start - 1`
/// (same location), or `pairs.len()` when there is none. A `Start`
/// without a location yields an empty group (`start`): the location is
/// the only thing that identifies the matching `End`, and swallowing the
/// rest of the document for one unlocated tag would be worse than
/// dropping it. (Tag contents must have a location by construction, so
/// this is a defensive fallback, not an expected path.)
fn find_end(pairs: &[Pair<'_>], start: usize, location: Option<Location>) -> usize {
    let Some(location) = location else {
        return start;
    };
    let mut i = start;
    while i < pairs.len() {
        if let Some(tagged) = pairs[i].0.to_packed::<TagElem>()
            && let Tag::End(loc, _, _) = tagged.tag
            && loc == location
        {
            return i;
        }
        i += 1;
    }
    pairs.len()
}

/// Find the tag group (start index, styles) for an element function in the
/// pairs, if any.
fn find_tag_group<'a>(
    pairs: &[Pair<'a>],
    func: Element,
) -> Option<(Vec<Pair<'a>>, StyleChain<'a>)> {
    for (i, (content, styles)) in pairs.iter().enumerate() {
        if let Some(tagged) = content.to_packed::<TagElem>()
            && let Tag::Start(inner, _) = &tagged.tag
            && inner.func() == func
        {
            let end = find_end(pairs, i + 1, inner.location());
            return Some((pairs[i + 1..end].to_vec(), *styles));
        }
    }
    None
}

/// Collect labels nested inside a `Content` (par/block bodies, styled
/// elements, sequences) into the label map.
fn collect_nested_labels(content: &Content, map: &mut HashMap<Location, String>) {
    if let Some(location) = content.location()
        && let Some(label) = content.label()
    {
        map.entry(location)
            .or_insert_with(|| label.resolve().to_string());
    }
    for child in content_children(content) {
        if !std::ptr::eq(child, content) {
            collect_nested_labels(child, map);
        }
    }
}

/// Whether the table declared `column-gutter`/`row-gutter` (the `gutter`
/// shorthand materializes into both, per the legacy U4 gate's comment).
fn table_gutter(table: &TableElem, styles: StyleChain<'_>) -> bool {
    !table.column_gutter.get_ref(styles).0.is_empty()
        || !table.row_gutter.get_ref(styles).0.is_empty()
}

/// Whether the figure declared a `placement:` argument (`auto` is the
/// default and is not custom). Mirrors the legacy U5 gate's element
/// read.
fn figure_custom_placement(figure: &FigureElem) -> bool {
    matches!(figure.placement.as_option(), Some(Some(Smart::Custom(_))))
}

/// Whether the outline's `target:` is not the default heading selector —
/// the legacy U6 gate reads the same field (its default is materialized,
/// so compare the value, not presence).
fn outline_custom_target(outline: &OutlineElem, styles: StyleChain<'_>) -> bool {
    outline.target.get_ref(styles) != &LocatableSelector(HeadingElem::ELEM.select())
}

/// Whether the outline declared a custom `indent:`.
fn outline_custom_indent(outline: &OutlineElem) -> bool {
    matches!(outline.indent.as_option(), Some(Smart::Custom(_)))
}

/// The figure kind for the IR, from the `FigureElem`'s kind.
fn figure_kind(figure: &FigureElem, styles: StyleChain<'_>) -> IrFigureKind {
    match figure.kind.get_cloned(styles) {
        Smart::Custom(FigureKind::Elem(func)) => {
            if func == ImageElem::ELEM {
                IrFigureKind::Image
            } else if func == TableElem::ELEM {
                IrFigureKind::Table
            } else {
                IrFigureKind::Other
            }
        }
        Smart::Custom(FigureKind::Name(name)) => {
            if name.as_str() == "image" {
                IrFigureKind::Image
            } else if name.as_str() == "table" {
                IrFigureKind::Table
            } else {
                IrFigureKind::Other
            }
        }
        Smart::Auto => IrFigureKind::Other,
    }
}

/// The rendered numbering text of a heading (`"1.1"`), when it carries
/// numbering. `HeadingElem.numbers` is the synthesized plain-text number
/// (empty when numbering is `None`).
fn heading_numbers(heading: &HeadingElem) -> Option<String> {
    match heading.numbers.as_ref() {
        Some(numbers) if !numbers.is_empty() => Some(numbers.to_string()),
        _ => None,
    }
}

/// An outline's maximum depth, clamped to a `u8`.
fn outline_depth(outline: &OutlineElem) -> u8 {
    match outline.depth.as_option() {
        Some(Some(depth)) => u8::try_from(depth.get()).unwrap_or(3),
        _ => 3,
    }
}

/// Force bold on every text run in a header row's cell body: the table
/// header's show rule renders header cells bold, but that bold is not in
/// the cell's own styles — it is applied at layout.
/// Force bold on every text run in an inline list (a term's runs: the
/// show rule renders the term `.strong()`).
fn bold_inlines(inlines: &mut [IrInline]) {
    for inline in inlines {
        match inline {
            IrInline::Text(run) => run.bold = true,
            IrInline::Link { body, .. } => bold_inlines(body),
            _ => {}
        }
    }
}

fn bold_header_cell_body(body: &mut [IrBlock]) {
    for block in body {
        match block {
            IrBlock::Paragraph(para) => {
                for inline in &mut para.inlines {
                    if let IrInline::Text(run) = inline {
                        run.bold = true;
                    }
                }
            }
            IrBlock::List(list) => {
                for item in &mut list.items {
                    bold_header_cell_body(&mut item.body);
                }
            }
            IrBlock::Enum(enumeration) => {
                for item in &mut enumeration.items {
                    bold_header_cell_body(&mut item.body);
                }
            }
            IrBlock::Quote { body: quote_body } => bold_header_cell_body(quote_body),
            _ => {}
        }
    }
}

/// A resolved `CellGrid`'s borders (`None` = the writer's uniform
/// default grid).
///
/// Explicit `hline`/`vline` groups map per side (`top`/`bottom`/`inside_h`
/// from `hlines`, `left`/`right`/`inside_v` from `vlines`); a line directly
/// after the header rows becomes `header_sep`. With no explicit lines,
/// the cell strokes decide: any stroke at all keeps the uniform grid (a
/// plain table's default stroke), no stroke anywhere means borderless
/// (`Some(nil)` — a `stroke: none` table or a plain grid).
fn table_borders_from_grid(grid: &CellGrid) -> Option<TableBorders> {
    let has_lines = grid.hlines.iter().any(|group| !group.is_empty())
        || grid.vlines.iter().any(|group| !group.is_empty());
    if has_lines {
        return Some(borders_from_lines(grid));
    }
    let any_stroke = grid.entries.iter().filter_map(Entry::as_cell).any(|cell| {
        cell.stroke.left.is_some()
            || cell.stroke.top.is_some()
            || cell.stroke.right.is_some()
            || cell.stroke.bottom.is_some()
    });
    if any_stroke {
        None
    } else {
        Some(TableBorders::default())
    }
}

/// Per-side borders from a grid's explicit lines. A line counts only
/// when it spans the full width (horizontal) or height (vertical);
/// partial lines have no table-level border. A line with no stroke
/// explicitly removes the border there (later lines win per side).
fn borders_from_lines(grid: &CellGrid) -> TableBorders {
    let cols = grid.non_gutter_column_count();
    let rows = grid.non_gutter_row_count();
    let header_rows = grid
        .headers
        .iter()
        .map(|header| header.range.end)
        .max()
        .unwrap_or(0);
    let mut top = None;
    let mut bottom = None;
    let mut header_sep = None;
    let mut inside_h: Vec<u32> = Vec::new();
    for group in &grid.hlines {
        for line in group {
            if !line_spans_full(line, cols) {
                continue;
            }
            let thickness = line_thickness(line);
            if line.index == 0 {
                top = thickness;
            } else if line.index == rows {
                bottom = thickness;
            } else if line.index == header_rows && header_rows > 0 {
                header_sep = thickness;
            } else if let Some(thickness) = thickness {
                inside_h.push(thickness);
            }
        }
    }
    let mut left = None;
    let mut right = None;
    let mut inside_v: Vec<u32> = Vec::new();
    for group in &grid.vlines {
        for line in group {
            if !line_spans_full(line, rows) {
                continue;
            }
            let thickness = line_thickness(line);
            if line.index == 0 {
                left = thickness;
            } else if line.index == cols {
                right = thickness;
            } else if let Some(thickness) = thickness {
                inside_v.push(thickness);
            }
        }
    }
    TableBorders {
        top,
        bottom,
        left,
        right,
        inside_h: mode_thickness(&inside_h),
        inside_v: mode_thickness(&inside_v),
        header_sep,
        header_rows,
    }
}

/// Whether a grid line spans the full width (for `hlines`, `tracks` is
/// the column count) or height (for `vlines`, the row count).
fn line_spans_full(line: &Line, tracks: usize) -> bool {
    line.start == 0 && line.end.is_none_or(|end| end.get() >= tracks)
}

/// One line's thickness in eighths of a point (`None` = the line was
/// explicitly removed, or it has no stroke). An `Auto` thickness is the
/// default rule (0.5pt).
fn line_thickness(line: &Line) -> Option<u32> {
    let stroke = line.stroke.as_ref()?;
    let pt = match stroke.thickness {
        Smart::Custom(thickness) => thickness.to_pt(),
        Smart::Auto => 0.5,
    };
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "thicknesses are small and clamped to at least 0.0 first"
    )]
    let eighths = (pt * 8.0).round().max(0.0) as u32;
    Some(eighths)
}

/// The most frequent thickness in a list (`None` when empty).
fn mode_thickness(thicknesses: &[u32]) -> Option<u32> {
    let mut counts: HashMap<u32, usize> = HashMap::new();
    for thickness in thicknesses {
        *counts.entry(*thickness).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(thickness, _)| thickness)
}

/// The marker text of a list at `depth`: the marker list's entry for that
/// depth (cycling the last), as plain text.
fn list_marker_at(list: &ListElem, depth: u8, styles: StyleChain<'_>) -> Option<String> {
    match list.marker.get_cloned(styles) {
        ListMarker::Content(markers) => marker_text_at(&markers, depth),
        ListMarker::Func(_) => None,
    }
}

/// The marker text of a bare `ListItem` run at `depth`, from the marker
/// list in force in the `StyleChain` (a `#set list(marker: …)` applies
/// there, as it does to a `ListElem`'s own marker field).
fn list_marker_from_styles(styles: StyleChain<'_>, depth: u8) -> Option<String> {
    match styles.get_cloned(ListElem::marker) {
        ListMarker::Content(markers) => marker_text_at(&markers, depth),
        ListMarker::Func(_) => None,
    }
}

/// One marker list's entry for `depth` (cycling the last), as plain text.
fn marker_text_at(markers: &[Content], depth: u8) -> Option<String> {
    let idx = (depth as usize).min(markers.len().saturating_sub(1));
    markers.get(idx).map(plain_text).filter(|t| !t.is_empty())
}

/// A `Numbering` value as an IR pattern (or `None` for a closure
/// numbering, which keeps the default).
fn pattern_from_numbering(numbering: &Numbering, full: bool) -> Option<IrNumberingPattern> {
    let Numbering::Pattern(pattern) = numbering else {
        return None;
    };
    let mut pieces = Vec::new();
    for (prefix, system) in &pattern.pieces {
        let system = match system {
            NamedNumeralSystem::Arabic => IrNumeralSystem::Arabic,
            NamedNumeralSystem::LowerLatin => IrNumeralSystem::LowerLatin,
            NamedNumeralSystem::UpperLatin => IrNumeralSystem::UpperLatin,
            NamedNumeralSystem::LowerRoman => IrNumeralSystem::LowerRoman,
            NamedNumeralSystem::UpperRoman => IrNumeralSystem::UpperRoman,
            _ => return None,
        };
        pieces.push(IrNumberingPiece {
            prefix: prefix.to_string(),
            system,
        });
    }
    Some(IrNumberingPattern {
        pieces,
        suffix: pattern.suffix.to_string(),
        full,
    })
}

/// An enum's numbering pattern, as an IR pattern (or `None` for a closure
/// numbering, which keeps the default).
fn enum_numbering(enumer: &EnumElem, styles: StyleChain<'_>) -> Option<IrNumberingPattern> {
    pattern_from_numbering(
        &enumer.numbering.get_cloned(styles),
        enumer.full.get(styles),
    )
}

/// The numbering pattern in force from the `StyleChain` (a
/// `#set enum(numbering: …)` applies there), for a bare `EnumItem` run
/// with no enclosing `EnumElem`.
fn enum_numbering_from_styles(styles: StyleChain<'_>) -> Option<IrNumberingPattern> {
    pattern_from_numbering(
        &styles.get_cloned(EnumElem::numbering),
        styles.get(EnumElem::full),
    )
}

/// The rendered number text of one enum item, from the pattern's k-th
/// piece (the piece index is the depth relative to the pattern's origin,
/// cycling the last piece, as `NumberingPattern::apply` does).
fn enum_number_text(
    pattern: &IrNumberingPattern,
    origin: u8,
    depth: u8,
    number: u64,
) -> Option<String> {
    if pattern.pieces.is_empty() {
        return None;
    }
    let idx = (depth.saturating_sub(origin) as usize).min(pattern.pieces.len() - 1);
    let piece = &pattern.pieces[idx];
    // Render the single piece: prefix + the number in this piece's system
    // + the pattern suffix. The numeral rendering is Typst's own
    // (via the pattern), so unmappable systems still render correctly
    // here even though the lowering cannot express them as Word numbering.
    let rendered = render_numeral(piece.system, number);
    Some(format!("{}{}{}", piece.prefix, rendered, pattern.suffix))
}

/// Render a number in a numeral system (the systems the lowering can also
/// express; others fall back to Arabic here so the literal text stays
/// correct — the Word-numbering level definition is what cannot express
/// them).
fn render_numeral(system: IrNumeralSystem, number: u64) -> String {
    match system {
        IrNumeralSystem::Arabic => number.to_string(),
        IrNumeralSystem::LowerLatin => to_latin(number, false),
        IrNumeralSystem::UpperLatin => to_latin(number, true),
        IrNumeralSystem::LowerRoman => to_roman(number, false),
        IrNumeralSystem::UpperRoman => to_roman(number, true),
    }
}

/// A number as Latin letters (1 -> a, 27 -> aa), as Typst renders them.
fn to_latin(mut number: u64, upper: bool) -> String {
    let mut out = String::new();
    while number > 0 {
        number -= 1;
        let c = (b'a' + (number % 26) as u8) as char;
        out.push(if upper { c.to_ascii_uppercase() } else { c });
        number /= 26;
    }
    out.chars().rev().collect()
}

/// A number as Roman numerals (1 -> i, 4 -> iv), as Typst renders them.
fn to_roman(mut number: u64, upper: bool) -> String {
    const TABLE: &[(u64, &str)] = &[
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut out = String::new();
    for (value, numeral) in TABLE {
        while number >= *value {
            out.push_str(numeral);
            number -= value;
        }
    }
    if upper { out.to_ascii_uppercase() } else { out }
}

/// Page content width in points, from the root styles: page width minus
/// left/right margins. Unset margins default to `2.5/21` of the smaller
/// page dimension (Typst's automatic margin).
fn page_content_width_pt(styles: StyleChain<'_>) -> f64 {
    let width_pt = match styles.get(PageElem::width) {
        Smart::Custom(length) => length.at(Abs::pt(11.0)).to_pt(),
        Smart::Auto => 595.28,
    };
    let height_pt = match styles.get(PageElem::height) {
        Smart::Custom(length) => length.at(Abs::pt(11.0)).to_pt(),
        Smart::Auto => 841.89,
    };
    let auto_margin = 2.5 / 21.0 * width_pt.min(height_pt);
    let margin = styles.get(PageElem::margin);
    let left = margin_side_pt(&margin, true, width_pt, auto_margin);
    let right = margin_side_pt(&margin, false, width_pt, auto_margin);
    (width_pt - left - right).max(0.0)
}

/// One side's margin in points, from the page margin setting.
fn margin_side_pt(
    margin: &Smart<Margin<Smart<Rel<Length>>>>,
    left: bool,
    page_width_pt: f64,
    auto_margin: f64,
) -> f64 {
    let Smart::Custom(margin) = margin else {
        return auto_margin;
    };
    let side = if left {
        margin.sides.left
    } else {
        margin.sides.right
    };
    let Some(side) = side else {
        return auto_margin;
    };
    match side {
        Smart::Custom(rel) => rel.abs.at(Abs::pt(11.0)).to_pt() + rel.rel.get() * page_width_pt,
        Smart::Auto => auto_margin,
    }
}

/// Plain text of a `Content`: the concatenation of its `TextElem` texts,
/// recursing into sequences, styled elements and par/block bodies.
pub fn plain_text(content: &Content) -> String {
    let mut out = String::new();
    plain_text_into(content, &mut out);
    out
}

fn plain_text_into(content: &Content, out: &mut String) {
    if let Some(text) = content.to_packed::<TextElem>() {
        out.push_str(&text.text);
        return;
    }
    for child in content_children(content) {
        if !std::ptr::eq(child, content) {
            plain_text_into(child, out);
        }
    }
}

/// Strip a leading run text from inlines (the rendered heading number, or
/// the single space after it). Removes the run when it becomes empty.
fn strip_leading_text(inlines: &mut Vec<IrInline>, prefix: &str) {
    if let Some(IrInline::Text(run)) = inlines.first_mut()
        && let Some(rest) = run.text.strip_prefix(prefix)
    {
        run.text = rest.to_string();
        if run.text.is_empty() {
            inlines.remove(0);
        }
    }
}

/// Find a `DirectLinkElem` nested in a `Content`.
fn find_direct_link(content: &Content) -> Option<&DirectLinkElem> {
    if let Some(link) = content.to_packed::<DirectLinkElem>() {
        return Some(link);
    }
    for child in content_children(content) {
        if !std::ptr::eq(child, content)
            && let Some(link) = find_direct_link(child)
        {
            return Some(link);
        }
    }
    None
}

/// Find a `PdfMarkerTag` of the given kind nested in a `Content`.
fn find_pdf_marker<'a>(
    content: &'a Content,
    kind: &PdfMarkerTagKind,
) -> Option<&'a typst_library::pdf::PdfMarkerTag> {
    if let Some(tag) = content.to_packed::<typst_library::pdf::PdfMarkerTag>()
        && tag.kind == *kind
    {
        return Some(tag);
    }
    for child in content_children(content) {
        if !std::ptr::eq(child, content)
            && let Some(tag) = find_pdf_marker(child, kind)
        {
            return Some(tag);
        }
    }
    None
}
