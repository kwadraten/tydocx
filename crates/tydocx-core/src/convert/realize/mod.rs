mod chain;
pub(crate) mod collect;
mod inline;
mod overline;
mod patch;
pub(crate) mod place;
mod style;

use std::collections::HashMap;

use codex::numeral_systems::NamedNumeralSystem;
use tydocx_ir::{
    CompatMode as IrCompatMode, Degradation as IrDegradation, DegradationAction as IrAction,
    GeometryInput, IrBlock, IrDocument, IrInline, IrNumeralSystem, IrParagraph, lower,
};
use tydocx_ooxml::document::{Document, FootnoteFormat};
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
use typst::syntax::{Span, VirtualPath};
use typst_layout::PagedDocument;
use typst_library::foundations::{SequenceElem, StyledElem};
use typst_library::layout::{BlockBody, BlockElem};
use typst_library::model::ParElem;
use typst_realize::realize;

use crate::compat::{CompatMode, ConvertOptions, ConvertOutcome, Degradation, DegradationAction};
use crate::convert::bibliography::extract_bibliography_sources;
use crate::world::TydocxWorld;

use super::apply_headers_and_footers;
use super::coalesce::coalesce_runs;
use super::page::{self, canonicalize_font_names, localize_cjk_fonts};
use super::postprocess::apply_paragraph_formatting;
use super::source::apply_source_overrides;
use collect::{collect_document, document_info};
use patch::patch_document;

/// Convert an equation to OMML, forwarding the math converter's
/// dropped/approximated-construct notes as degradations (deduplicated by
/// construct + message — a document may repeat the same wrapper many times).
pub(crate) fn equation_omml_ir(content: &Content, degradations: &mut Vec<IrDegradation>) -> String {
    let (omml, notes) = tydocx_math::equation_to_omml_report(content);
    let span = content.span();
    let span = span.id().map(|_| span);
    for note in notes {
        if !degradations
            .iter()
            .any(|d| d.construct == note.construct && d.message == note.message)
        {
            degradations.push(IrDegradation {
                span,
                construct: note.construct,
                action: if note.dropped {
                    IrAction::Skipped
                } else {
                    IrAction::Approximated
                },
                message: note.message,
            });
        }
    }
    omml
}

#[derive(Debug, Clone, Copy)]
pub struct DocxPosition {
    pub pair: usize,
}

pub struct DocxDocument {
    ir: IrDocument,
    footnote_markers: Vec<inline::FootnoteMarker>,
    footnote_bodies: HashMap<Location, Vec<IrParagraph>>,
    footnote_circled: bool,
    degradations: Vec<IrDegradation>,
    chain_overrides: page::SourceStyleOverrides,
    section_span: Option<Span>,
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
            chain_overrides: state.chain_overrides,
            section_span: state.section_span,
            introspector,
        })
    }

    fn introspector(&self) -> &dyn Introspector {
        self
    }
}

impl DocxDocument {
    /// The collected styled IR before lowering — the conformance suite
    /// audits element coverage against it.
    #[must_use]
    pub fn ir(&self) -> &IrDocument {
        &self.ir
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

/// Layouter bodies hold closures, not content, so only plain `Content`
/// bodies recurse.
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

/// # Errors
///
/// Returns the Typst compile diagnostics when the source fails to compile.
pub fn convert_ir(
    world: &TydocxWorld,
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

/// C1-only degradations that are not already emitted by the collector.
fn c1_chain_degradations(
    world: &TydocxWorld,
    chain: &page::SourceStyleOverrides,
    has_section_change: bool,
    section_span: Option<Span>,
    degradations: &mut Vec<Degradation>,
) {
    if has_section_change {
        degradations.push(Degradation::new(
            section_span,
            "section-break",
            DegradationAction::Skipped,
            "page columns or margins change mid-document; kept as a single section",
        ));
    }
    if chain.margins_mirrored {
        degradations.push(Degradation::new(
            None,
            "mirrored-margins",
            DegradationAction::Skipped,
            "inside/outside margins are not representable without w:mirrorMargins; \
             both sides keep the Typst default",
        ));
    }
    if chain.numbering_changed {
        degradations.push(Degradation::new(
            None,
            "page-numbering-change",
            DegradationAction::Skipped,
            "the page numbering format or counter restart changes mid-document; \
             kept as a single numbering scheme",
        ));
    }
    if chain.page_furniture {
        degradations.push(Degradation::new(
            None,
            "page-foreground",
            DegradationAction::Approximated,
            "page foreground/background is not representable as anchored content; \
             its content is kept in the page header or dropped",
        ));
    }
    // Whether a `show` rule re-emitted `it` is unobservable in the pairs.
    let main = world.main_source();
    let template_names = page::extract_show_template_names_from_source(main.text());
    let show = page::extract_source_style_overrides(main.text(), &template_names);
    if show.show_outline_entry_rule {
        degradations.push(Degradation::new(
            None,
            "show-outline-entry-rule",
            DegradationAction::Approximated,
            "a `show outline.entry` rule rewrites outline entries; the table of \
             contents keeps Word's default entry formatting",
        ));
    }
    if show.show_heading_rule {
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

/// Page settings and document style for the IR path.
fn apply_page_and_style_ir(
    chain: &page::SourceStyleOverrides,
    world: &TydocxWorld,
    paged_doc: Option<&PagedDocument>,
    doc: &mut Document,
    degradations: &mut Vec<Degradation>,
) {
    if let Some(paged) = paged_doc {
        doc.style = page::extract_document_style(paged);
        page::extract_page_settings(paged, &mut doc.page_settings);
    }
    // AST fill-in for what no chain carries; `page_numbering` stays AST-only
    // on purpose — a single source, last-wins as before.
    let mut styles = chain.clone();
    styles.merge_from(&super::source::gather_source_overrides(world));
    apply_source_overrides(&styles, doc, paged_doc.is_some(), degradations);
    // `counter(footnote).update(0)` in a page header — `CounterUpdateElem`'s
    // key is private, so the chain can't identify the counter.
    doc.style.footnote_restart_each_page = super::source::detect_footnote_restart_each_page(world);
}

pub fn export_ir(
    docx_doc: &DocxDocument,
    paged_doc: Option<&PagedDocument>,
    world: &TydocxWorld,
    options: &ConvertOptions,
) -> ConvertOutcome {
    export_ir_with_ir(docx_doc, paged_doc, world, options).0
}

/// `export_ir` plus the patched IR it lowered — the conformance suite audits
/// element coverage against the post-patch IR, which is what `lower`
/// consumed.
#[must_use]
pub fn export_ir_with_ir(
    docx_doc: &DocxDocument,
    paged_doc: Option<&PagedDocument>,
    world: &TydocxWorld,
    options: &ConvertOptions,
) -> (ConvertOutcome, IrDocument) {
    let mut ir = docx_doc.ir.clone();
    let mut degradations: Vec<Degradation> =
        docx_doc.degradations.iter().map(core_degradation).collect();

    let mut tmp = Document::new();
    apply_page_and_style_ir(
        &docx_doc.chain_overrides,
        world,
        paged_doc,
        &mut tmp,
        &mut degradations,
    );
    apply_headers_and_footers(
        paged_doc,
        world,
        &mut tmp,
        options.compat,
        &mut degradations,
        &place::place_location_hashes(&ir.body),
    );
    let section_break = !ir.sections.is_empty();
    if options.compat == CompatMode::C1 {
        ir.sections.clear();
    }
    if ir.sections.is_empty() {
        ir.page = tmp.page_settings.clone();
    }
    ir.style = tmp.style.clone();
    if !ir.sections.is_empty()
        && let Some(paged) = paged_doc
    {
        assign_numbering_starts(&mut ir.sections, ir.first_numbering, paged);
    }

    if docx_doc.footnote_circled {
        ir.style.footnote_format = FootnoteFormat::CircledNumber;
    }

    measure_heading_sizes_from_ir(&mut ir);

    ir.citation_sources = extract_bibliography_sources(docx_doc.introspector(), world);

    if ir.info.title.is_none() {
        ir.info.title = first_heading_text(&ir);
    }

    let geometry = GeometryInput {
        page_size: (
            tmp.page_settings.width_twips,
            tmp.page_settings.height_twips,
        ),
        page_numbering: tmp.page_numbering,
        page_number_start: tmp.page_number_start,
        supplement: None,
        header: tmp.header.clone(),
        header_even: tmp.header_even.clone(),
        header_first: tmp.header_first.clone(),
        footer: tmp.footer.clone(),
        footer_even: tmp.footer_even.clone(),
        footer_first: tmp.footer_first.clone(),
        page_number_footers: tmp.page_number_footers.clone(),
        even_and_odd_headers: tmp.even_and_odd_headers,
        title_page: tmp.title_page,
    };

    if let Some(paged) = paged_doc {
        patch_outline_page_numbers(&mut ir, paged, &mut degradations);
        let style = ir.style.clone();
        let patch_degradations = patch_document(
            &mut ir,
            &docx_doc.footnote_markers,
            &docx_doc.footnote_bodies,
            paged,
            &style,
        );
        degradations.extend(patch_degradations.iter().map(core_degradation));
        if options.compat != CompatMode::C1 {
            overline::patch_overlines(&mut ir, paged, world);
            place::patch_places(&mut ir, paged);
        }
    }

    let (mut doc, lower_degradations) = lower(&ir, &geometry, ir_compat(options.compat));
    degradations.extend(lower_degradations.iter().map(core_degradation));

    apply_bibliography_style_option(&mut doc, options, &mut degradations);

    apply_paragraph_formatting(&mut doc);
    canonicalize_font_names(world, &mut doc);
    localize_cjk_fonts(world, &mut doc);
    coalesce_runs(&mut doc);

    if options.compat == CompatMode::C1 {
        c1_chain_degradations(
            world,
            &docx_doc.chain_overrides,
            section_break,
            docx_doc.section_span,
            &mut degradations,
        );
    }

    (ConvertOutcome::new(doc, degradations), ir)
}

fn assign_numbering_starts(
    sections: &mut [tydocx_ir::IrSection],
    first_numbering: Option<tydocx_ooxml::document::PageNumberFormat>,
    paged: &PagedDocument,
) {
    let rendered = paged.pages();
    if rendered.len() < 2 {
        return;
    }
    let fmts: Vec<Option<tydocx_ooxml::document::PageNumberFormat>> = rendered
        .iter()
        .map(|p| page::numbering_page_format(p.numbering.as_ref()))
        .collect();
    let mut prev_fmt = first_numbering;
    let mut scan_from = 1_usize;
    for section in sections.iter_mut() {
        if let Some(fmt) = section.numbering
            && Some(fmt) != prev_fmt
            && let Some(index) = (scan_from..rendered.len()).find(|&i| fmts[i] == Some(fmt))
        {
            let number = rendered[index].number;
            let prev_number = rendered[index - 1].number;
            if number != prev_number + 1 {
                section.numbering_start = u32::try_from(number).ok();
            }
            scan_from = index + 1;
        }
        prev_fmt = section.numbering.or(prev_fmt);
    }
}

fn ir_compat(mode: CompatMode) -> IrCompatMode {
    match mode {
        CompatMode::C1 => IrCompatMode::C1,
        CompatMode::C2 => IrCompatMode::C2,
        CompatMode::C3 => IrCompatMode::C3,
    }
}

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

fn ir_plain_text(inlines: &[IrInline]) -> String {
    let mut text = String::new();
    for inline in inlines {
        match inline {
            IrInline::Text(run) => text.push_str(&run.text),
            IrInline::Link { body, .. } | IrInline::Field { body, .. } => {
                text.push_str(&ir_plain_text(body));
            }
            IrInline::Space => text.push(' '),
            _ => {}
        }
    }
    text
}

/// Render `n` in a `#set page(numbering:)` pattern — `prefix + numeral` per
/// piece plus the trailing `suffix`. Function numberings and numeral systems
/// the IR cannot render return `None`.
fn render_page_number(numbering: &Numbering, n: usize) -> Option<String> {
    let Numbering::Pattern(pattern) = numbering else {
        return None;
    };
    let mut out = String::new();
    for (prefix, system) in &pattern.pieces {
        let system = match system {
            NamedNumeralSystem::Arabic => IrNumeralSystem::Arabic,
            NamedNumeralSystem::LowerLatin => IrNumeralSystem::LowerLatin,
            NamedNumeralSystem::UpperLatin => IrNumeralSystem::UpperLatin,
            NamedNumeralSystem::LowerRoman => IrNumeralSystem::LowerRoman,
            NamedNumeralSystem::UpperRoman => IrNumeralSystem::UpperRoman,
            _ => return None,
        };
        out.push_str(prefix);
        out.push_str(&system.render(n as u64));
    }
    out.push_str(&pattern.suffix);
    Some(out)
}

/// Fill `IrOutlineEntry.page_number` from the companion `PagedDocument` — the
/// only representation that knows rendered page positions and the page
/// numbering format applying at each target. Locations are content hashes
/// stable across the two compiles. A numbering the pattern renderer cannot
/// express falls back to decimal digits and is reported — the `PAGEREF`
/// field still re-renders correctly on update.
fn patch_outline_page_numbers(
    ir: &mut IrDocument,
    paged: &PagedDocument,
    degradations: &mut Vec<Degradation>,
) {
    let mut unrenderable = false;
    for block in &mut ir.body {
        let IrBlock::Outline { entries, .. } = block else {
            continue;
        };
        for entry in entries.iter_mut() {
            if entry.page_number.is_none()
                && let Some(hash) = entry.page_loc
            {
                let location = typst_library::introspection::Location::new(hash);
                let Some(page) = paged.introspector().page(location) else {
                    continue;
                };
                let formatted = paged
                    .introspector()
                    .page_numbering(location)
                    .and_then(|numbering| {
                        render_page_number(numbering, page.get()).or_else(|| {
                            unrenderable = true;
                            None
                        })
                    })
                    .unwrap_or_else(|| page.get().to_string());
                entry.page_number = Some(formatted);
            }
        }
    }
    if unrenderable {
        degradations.push(Degradation::new(
            None,
            "outline-page-number",
            DegradationAction::Approximated,
            "a page numbering format not renderable in isolation (a numbering function or \
             an unsupported numeral system) fell back to decimal digits in the cached TOC \
             page numbers; a field update re-renders them"
                .to_string(),
        ));
    }
}

/// Extract the Word style name from a bibliography XSL: Office styles declare
/// `<b:StyleNameLocalized ...>NAME</b:StyleNameLocalized>` (or `b:StyleName`) in
/// the header comment. Falls back to the file stem by the caller.
fn xsl_style_name(xsl: &str) -> Option<String> {
    for tag in ["StyleNameLocalized", "StyleName"] {
        let mut rest = xsl;
        while let Some(open) = rest.find(&format!("<b:{tag}")) {
            let after = &rest[open..];
            let Some(gt) = after.find('>') else { break };
            let body = &after[gt + 1..];
            if let Some(close) = body.find("</b:") {
                let name = body[..close].trim();
                if !name.is_empty() {
                    return Some(name.to_string());
                }
            }
            rest = body;
        }
    }
    None
}

/// Resolve `--bibliography-style <file.xsl>` into Sources.xml's
/// `SelectedStyle`/`StyleName` plus an embedded copy of the XSL
/// (`customXml/item2.xml`). Word resolves `SelectedStyle` only from
/// installed style directories, so the embedded XSL travels with the
/// document for tools that can extract and install it — it does not make
/// Word apply the style. The cached field result keeps the document's CSL
/// render and is never silently re-rendered under another style: the
/// selection always reports what the cache does not honour.
fn apply_bibliography_style_option(
    doc: &mut tydocx_ooxml::document::Document,
    options: &ConvertOptions,
    degradations: &mut Vec<Degradation>,
) {
    let Some(style_path) = &options.bibliography_style else {
        return;
    };
    if doc.citation_sources.is_empty() {
        degradations.push(Degradation::new(
            None,
            "bibliography-style",
            DegradationAction::Skipped,
            format!(
                "bibliography style {style_path:?} was requested but the document has no \
                 bibliography sources; no Sources.xml is emitted and the selection is dropped"
            ),
        ));
        return;
    }
    let file_name = std::path::Path::new(style_path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned());
    let Some((name, xsl)) =
        file_name.and_then(|name| std::fs::read(style_path).ok().map(|x| (name, x)))
    else {
        degradations.push(Degradation::new(
            None,
            "bibliography-style",
            DegradationAction::Skipped,
            format!(
                "bibliography style file {style_path:?} could not be read; \
                 Sources.xml keeps the default APA selection"
            ),
        ));
        return;
    };
    let style_name = xsl_style_name(&String::from_utf8_lossy(&xsl)).unwrap_or_else(|| {
        name.trim_end_matches(".xsl")
            .trim_end_matches(".XSL")
            .to_string()
    });
    doc.bibliography_style = Some((format!("\\{name}"), style_name.clone()));
    doc.bibliography_style_xsl = Some(xsl);
    degradations.push(Degradation::new(
        None,
        "bibliography-style",
        DegradationAction::Approximated,
        format!(
            "Sources.xml selects bibliography style '{style_name}' ({name}); \
             the XSL is embedded in the package, but the cached field result keeps the \
             document's CSL render — it is not re-rendered in '{style_name}'. Word \
             resolves the selection only after the style is installed on the reader's \
             system, at which point a field update re-renders the entries"
        ),
    ));
}
