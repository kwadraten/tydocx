//! The realize-based conversion path (step 2).
//!
//! `DocxDocument` is a Typst `Output` (`Target::Paged`) that realizes the
//! document once into `(Content, StyleChain)` pairs and collects them
//! into an owned IR (`typort_ir::IrDocument`) during `create`. The
//! companion `PagedDocument` supplies only what the pairs cannot:
//! footnote entry bodies and on-page image sizes (see `patch`).
//!
//! `convert_ir` compiles both targets and lowers the IR to an OOXML
//! `Document`; `export_ir` does the same from already-compiled documents
//! (for tests). The legacy `export` in `super` stays as the fallback and
//! comparison baseline.

mod collect;
mod inline;
mod patch;
mod style;

use std::collections::HashMap;

use typort_ir::{
    CompatMode as IrCompatMode, Degradation as IrDegradation, DegradationAction as IrAction,
    GeometryInput, IrBlock, IrDocument, IrInline, IrParagraph, lower,
};
use typort_ooxml::document::{Document, FootnoteFormat};
use typst::World;
use typst::diag::{SourceDiagnostic, SourceResult, StrResult};
use typst::ecow::{EcoString, EcoVec};
use typst::engine::Engine;
use typst::foundations::{Content, Label, Output, Selector, StyleChain, Target};
use typst::introspection::{
    DocumentPosition, ElementIntrospector, ElementIntrospectorBuilder, Introspector, Location,
    Locator, SplitLocator, TagElem,
};
use typst::model::{DocumentInfo, Numbering};
use typst::routines::{Arenas, FragmentKind, Pair, RealizationKind};
use typst::syntax::VirtualPath;
use typst_layout::PagedDocument;
use typst_library::foundations::{SequenceElem, StyledElem};
use typst_library::layout::{BlockBody, BlockElem};
use typst_library::model::ParElem;
use typst_realize::realize;

use crate::compat::{CompatMode, ConvertOptions, ConvertOutcome, Degradation, DegradationAction};
use crate::convert::bibliography::extract_bibliography_sources;
use crate::world::TyportWorld;

use super::coalesce::coalesce_runs;
use super::page::{self, canonicalize_font_names, localize_cjk_fonts};
use super::postprocess::apply_paragraph_formatting;
use super::smallcaps::apply_smallcaps_from_source;
use super::source::apply_hanging_indent_from_source;
use super::{apply_headers_and_footers, apply_page_and_style};
use collect::{collect_document, document_info};
use patch::patch_document;

/// A position in the DOCX output: the index of the pair the element came
/// from. The DOCX backend has no pages or DOM nodes, so positions only
/// need to be distinct enough for debugging; the introspector itself never
/// uses them.
#[derive(Debug, Clone, Copy)]
pub struct DocxPosition {
    pub pair: usize,
}

/// A realized document: the owned IR collected during `create`, plus the
/// introspector built from the pairs' tags and the collection state the
/// patch step needs.
pub struct DocxDocument {
    ir: IrDocument,
    footnote_markers: Vec<(Location, String)>,
    footnote_bodies: HashMap<Location, Vec<IrParagraph>>,
    footnote_circled: bool,
    degradations: Vec<IrDegradation>,
    introspector: ElementIntrospector<DocxPosition>,
}

impl Output for DocxDocument {
    fn target() -> Target {
        Target::Paged
    }

    fn create(engine: &mut Engine, content: &Content, styles: StyleChain) -> SourceResult<Self> {
        let mut info = DocumentInfo::default();
        info.populate(styles);
        info.populate_locale(styles);

        let mut locator = Locator::root().split();
        let arenas = Arenas::default();

        let pairs = realize(
            RealizationKind::Document { info: &mut info },
            engine,
            &mut locator,
            &arenas,
            content,
            styles,
        )?;

        let ir_info = document_info(&info, styles);
        let source = engine.world.source(engine.world.main()).ok();
        let (ir, state) = collect_document(
            &pairs,
            engine,
            &mut locator,
            styles,
            &ir_info,
            source,
            content,
        )?;
        let introspector = build_introspector(&pairs);

        Ok(Self {
            ir,
            footnote_markers: state.footnote_markers,
            footnote_bodies: state.footnote_bodies,
            footnote_circled: state.footnote_circled,
            degradations: state.degradations,
            introspector,
        })
    }

    fn introspector(&self) -> &dyn Introspector {
        self
    }
}

impl Introspector for DocxDocument {
    fn query(&self, selector: &Selector) -> EcoVec<Content> {
        self.introspector.query(selector)
    }

    fn query_first(&self, selector: &Selector) -> Option<Content> {
        self.introspector.query_first(selector)
    }

    fn query_unique(&self, selector: &Selector) -> StrResult<Content> {
        self.introspector.query_unique(selector)
    }

    fn query_label(&self, label: Label) -> StrResult<&Content> {
        self.introspector.query_label(label)
    }

    fn query_labelled(&self) -> EcoVec<Content> {
        self.introspector.query_labelled()
    }

    fn query_count_before(&self, selector: &Selector, end: Location) -> usize {
        self.introspector.query_count_before(selector, end)
    }

    fn label_count(&self, label: Label) -> usize {
        self.introspector.label_count(label)
    }

    fn locator(&self, key: u128, base: Location) -> Option<Location> {
        self.introspector.locator(key, base)
    }

    fn pages(&self, _: Location) -> Option<std::num::NonZeroUsize> {
        None
    }

    fn page(&self, _: Location) -> Option<std::num::NonZeroUsize> {
        None
    }

    fn position(&self, _: Location) -> Option<DocumentPosition> {
        None
    }

    fn page_numbering(&self, _: Location) -> Option<&Numbering> {
        None
    }

    fn page_supplement(&self, _: Location) -> Option<&Content> {
        None
    }

    fn anchor(&self, _: Location) -> Option<&EcoString> {
        None
    }

    fn document(&self, _: Location) -> Option<Location> {
        None
    }

    fn path(&self, _: Location) -> Option<&VirtualPath> {
        None
    }
}

/// Build the introspector from the pairs' tag stream: top-level tag pairs
/// directly, and tags nested inside par/block bodies (cite-group,
/// footnote, …) recursively, in document order.
fn build_introspector(pairs: &[Pair<'_>]) -> ElementIntrospector<DocxPosition> {
    let mut builder = ElementIntrospectorBuilder::<DocxPosition>::new();
    for (index, (content, styles)) in pairs.iter().enumerate() {
        let position = DocxPosition { pair: index };
        if let Some(tagged) = content.to_packed::<TagElem>() {
            builder.discover_tag(&tagged.tag, position);
        } else {
            collect_nested_tags(content, position, *styles, &mut builder);
        }
    }
    builder.finalize()
}

/// Collect tags nested inside a non-tag pair's body (`SequenceElem`,
/// `StyledElem`, `ParElem`, `BlockElem`). Block bodies that are layouters
/// (`SingleLayouter`/`MultiLayouter`) hold closures, not content, so only
/// plain `Content` bodies recurse.
fn collect_nested_tags(
    content: &Content,
    position: DocxPosition,
    styles: StyleChain,
    builder: &mut ElementIntrospectorBuilder<DocxPosition>,
) {
    if let Some(tagged) = content.to_packed::<TagElem>() {
        builder.discover_tag(&tagged.tag, position);
        return;
    }
    if let Some(seq) = content.to_packed::<SequenceElem>() {
        for child in &seq.children {
            collect_nested_tags(child, position, styles, builder);
        }
    } else if let Some(styled) = content.to_packed::<StyledElem>() {
        collect_nested_tags(&styled.child, position, styles, builder);
    } else if let Some(par) = content.to_packed::<ParElem>() {
        collect_nested_tags(&par.body, position, styles, builder);
    } else if let Some(block) = content.to_packed::<BlockElem>()
        && let Some(BlockBody::Content(body)) = block.body.get_ref(styles)
    {
        collect_nested_tags(body, position, styles, builder);
    }
}

/// Re-realize one body as a fragment (verification loop route (a)) and run
/// `f` on its pairs.
///
/// The fragment shares the main realize's `Engine` and derives its
/// locator from the main `SplitLocator` at the body's span, so locations
/// stay consistent with the main realize. The callback receives the
/// engine and the main locator as parameters — it cannot capture them,
/// because the fragment borrows both while it runs.
pub(super) fn with_fragment<R>(
    engine: &mut Engine,
    locator: &mut SplitLocator,
    content: &Content,
    styles: StyleChain<'_>,
    f: impl FnOnce(&mut Engine, &mut SplitLocator, &[Pair<'_>]) -> SourceResult<R>,
) -> SourceResult<R> {
    let mut kind = FragmentKind::Block;
    let frag_arenas = Arenas::default();
    let mut frag_locator = locator.next(&content.span()).split();
    let pairs = realize(
        RealizationKind::Fragment { kind: &mut kind },
        engine,
        &mut frag_locator,
        &frag_arenas,
        content,
        styles,
    )?;
    f(engine, locator, &pairs)
}

/// Convert a document through the realize path: compile the IR document
/// and a companion `PagedDocument`, then lower.
///
/// # Errors
///
/// Returns the Typst compile diagnostics when the source fails to compile.
pub fn convert_ir(
    world: &TyportWorld,
    options: &ConvertOptions,
) -> Result<ConvertOutcome, Vec<SourceDiagnostic>> {
    let docx_result = typst::compile::<DocxDocument>(world);
    let docx_doc = match docx_result.output {
        Ok(doc) => doc,
        Err(errors) => return Err(errors.into_iter().collect()),
    };
    let paged_result = typst::compile::<PagedDocument>(world);
    let paged_doc = paged_result.output.ok();
    Ok(export_ir(&docx_doc, paged_doc.as_ref(), world, options))
}

/// Measure per-level heading sizes from the collected headings' own
/// runs, mirroring the legacy `measure_heading_sizes` (which reads the
/// same sizes from Paged runs). For each level, the dominant run size
/// (weighted by glyph count) is that level's measured size. A level
/// whose headings render at exactly the body size carries no sized run
/// (the size is omitted when it equals the body default), so it falls
/// back to the body size rather than the histogram value: such headings
/// exist, they are just not bigger than the body. A level with no
/// heading runs at all keeps the histogram value.
fn measure_heading_sizes_from_ir(ir: &mut IrDocument) {
    let mut per_level: [std::collections::HashMap<u32, usize>; 5] = Default::default();
    let mut run_counts = [0_usize; 5];
    collect_heading_sizes(&ir.body, &mut per_level, &mut run_counts);
    for (idx, counts) in per_level.iter().enumerate() {
        if let Some((&size, _)) = counts.iter().max_by_key(|(_, count)| **count) {
            ir.style.heading_sizes[idx] = size;
        } else if run_counts[idx] > 0 {
            ir.style.heading_sizes[idx] = ir.style.body_size_half_pt;
        }
    }
}

/// Accumulate per-level heading-run sizes (size -> glyph count) from a
/// block list, recursing into nested blocks. `run_counts` counts every
/// heading text run per level, sized or not.
fn collect_heading_sizes(
    blocks: &[IrBlock],
    per_level: &mut [std::collections::HashMap<u32, usize>; 5],
    run_counts: &mut [usize; 5],
) {
    for block in blocks {
        match block {
            IrBlock::Heading { level, body, .. } => {
                let idx = usize::from(*level).saturating_sub(1).min(4);
                for inline in body {
                    if let IrInline::Text(run) = inline {
                        run_counts[idx] += 1;
                        if let Some(size) = run.size_half_pt {
                            *per_level[idx].entry(size).or_default() += run.text.chars().count();
                        }
                    }
                }
            }
            IrBlock::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        collect_heading_sizes(&cell.body, per_level, run_counts);
                    }
                }
            }
            IrBlock::Grid(grid) => {
                for row in &grid.rows {
                    for cell in &row.cells {
                        collect_heading_sizes(&cell.body, per_level, run_counts);
                    }
                }
            }
            IrBlock::Figure { body, .. }
            | IrBlock::Quote { body }
            | IrBlock::Place { body, .. } => {
                collect_heading_sizes(body, per_level, run_counts);
            }
            IrBlock::List(list) => {
                for item in &list.items {
                    collect_heading_sizes(&item.body, per_level, run_counts);
                }
            }
            IrBlock::Enum(enumer) => {
                for item in &enumer.items {
                    collect_heading_sizes(&item.body, per_level, run_counts);
                }
            }
            _ => {}
        }
    }
}

/// Lower an already-compiled IR document (and its companion Paged
/// document, when the Paged compile succeeded) to an OOXML `Document`.
/// The entry point for tests; `convert_ir` is the entry point for the CLI.
pub fn export_ir(
    docx_doc: &DocxDocument,
    paged_doc: Option<&PagedDocument>,
    world: &TyportWorld,
    options: &ConvertOptions,
) -> ConvertOutcome {
    let mut ir = docx_doc.ir.clone();
    let mut degradations: Vec<Degradation> =
        docx_doc.degradations.iter().map(core_degradation).collect();

    // Page settings, document style, and running headers/footers come
    // from the legacy three-source extraction (Paged + source-AST
    // overrides) applied to a scratch document, then copied into the IR.
    let mut tmp = Document::new();
    apply_page_and_style(world, paged_doc, &mut tmp, &mut degradations);
    apply_headers_and_footers(paged_doc, &mut tmp, options.compat, &mut degradations);
    ir.page = tmp.page_settings.clone();
    ir.style = tmp.style.clone();

    // Circled footnote numbering (①②③) is declared on the footnote
    // elements, not visible in the Paged-derived style: the legacy
    // `detect_footnote_format` reads it from the rendered markers, the
    // IR path reads it from the numbering patterns (see the `FootnoteElem`
    // branch in `handle_tag_start`).
    if docx_doc.footnote_circled {
        ir.style.footnote_format = FootnoteFormat::CircledNumber;
    }

    // Per-level heading sizes, measured from the collected headings'
    // own runs (see below): the histogram `detect_heading_sizes`
    // assigns sizes by rank, so a document whose largest heading is not
    // level 1 — or that skips levels — gets the wrong per-level sizes,
    // and `lower_heading` then fails to strip the run size that restates
    // the style.
    measure_heading_sizes_from_ir(&mut ir);

    // Bibliography sources, from the IR document's own introspector.
    ir.citation_sources = extract_bibliography_sources(docx_doc.introspector(), world);

    // Title fallback: the first heading's text, as the legacy path does.
    if ir.info.title.is_none() {
        ir.info.title = first_heading_text(&ir);
    }

    // Geometry: page-level facts only.
    let geometry = GeometryInput {
        page_size: (
            tmp.page_settings.width_twips,
            tmp.page_settings.height_twips,
        ),
        page_numbering: tmp.page_numbering,
        supplement: None,
        header: tmp.header.clone(),
        footer: tmp.footer.clone(),
        page_number_footer: tmp.page_number_footer,
    };

    // Footnote bodies and image sizes from the companion Paged document.
    if let Some(paged) = paged_doc {
        let style = ir.style.clone();
        let patch_degradations = patch_document(
            &mut ir,
            &docx_doc.footnote_markers,
            &docx_doc.footnote_bodies,
            paged,
            &style,
        );
        degradations.extend(patch_degradations.iter().map(core_degradation));
    }

    let (mut doc, lower_degradations) = lower(&ir, &geometry, ir_compat(options.compat));
    degradations.extend(lower_degradations.iter().map(core_degradation));

    // Post-passes that are path-independent: paragraph formatting
    // (indent after headings, bibliography hanging indent), source
    // hanging indent, CJK font localization, source small caps, and run
    // coalescing. Deliberately skipped here (see fallback-registry.md):
    // recovery passes, three-line-table detection, section-break
    // detection, `apply_breaks_from_source` (the IR already emits
    // pagebreaks), `apply_styles_from_paged` (redundant: the collector
    // already resolves per-run font/size from the `StyleChain` in
    // `run_from_styles`, and IR runs do carry spans — `run.span` is set
    // — so the Paged override pass would only re-derive the same
    // properties through span-keyed matching), and same-line paragraph
    // merging (needs HTML body geometry).
    apply_paragraph_formatting(&mut doc);
    apply_hanging_indent_from_source(world, &mut doc);
    canonicalize_font_names(world, &mut doc);
    localize_cjk_fonts(world, &mut doc);
    apply_smallcaps_from_source(world, &mut doc);
    coalesce_runs(&mut doc);

    // M6 (C1 only): a mid-document columns/margins change starts a new
    // Typst section; C1 keeps a single section, so the change is dropped.
    // Size-only section breaks stay in all modes (compat-modes §1.3).
    if options.compat == CompatMode::C1 {
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
    }

    ConvertOutcome::new(doc, degradations)
}

/// Convert the core `CompatMode` to the IR crate's identical enum. (The
/// two crates define the mode separately; the conversion is the boundary
/// between them.)
fn ir_compat(mode: CompatMode) -> IrCompatMode {
    match mode {
        CompatMode::C1 => IrCompatMode::C1,
        CompatMode::C2 => IrCompatMode::C2,
        CompatMode::C3 => IrCompatMode::C3,
    }
}

/// Convert an IR-crate degradation into the core crate's identical record.
fn core_degradation(degradation: &IrDegradation) -> Degradation {
    let action = match degradation.action {
        IrAction::Skipped => DegradationAction::Skipped,
        IrAction::Approximated => DegradationAction::Approximated,
        IrAction::Literalized => DegradationAction::Literalized,
    };
    Degradation::new(
        degradation.span,
        degradation.construct,
        action,
        degradation.message.clone(),
    )
}

/// Plain text of the first heading, for the title fallback.
fn first_heading_text(ir: &IrDocument) -> Option<String> {
    for block in &ir.body {
        if let IrBlock::Heading { body, .. } = block {
            let title = ir_plain_text(body);
            if !title.trim().is_empty() {
                return Some(title);
            }
        }
    }
    None
}

/// Plain text of an inline list (runs and link bodies; everything else
/// contributes nothing).
fn ir_plain_text(inlines: &[IrInline]) -> String {
    let mut text = String::new();
    for inline in inlines {
        match inline {
            IrInline::Text(run) => text.push_str(&run.text),
            IrInline::Link { body, .. } => text.push_str(&ir_plain_text(body)),
            IrInline::Space => text.push(' '),
            _ => {}
        }
    }
    text
}
