mod chain;
pub(crate) mod collect;
mod inline;
mod overline;
mod patch;
pub(crate) mod place;
mod run_sizes;
mod shape;
mod style;

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::ir::{
    CompatMode as IrCompatMode, Degradation as IrDegradation, DegradationAction as IrAction,
    GeometryInput, IrBlock, IrDocument, IrInline, IrParagraph, lower,
};
use crate::ooxml::document::{Document, FootnoteFormat, HeaderFooter, SectionFooter};
use typst::World;
use typst::comemo::Track;
use typst::diag::{At, SourceDiagnostic, SourceResult, StrResult};
use typst::ecow::{EcoString, EcoVec};
use typst::engine::{Engine, Route, Sink};
use typst::foundations::{Content, Label, Output, Selector, StyleChain, StyledElem, Target};
use typst::introspection::{
    DocumentPosition, ElementIntrospector, ElementIntrospectorBuilder, Introspector, Location,
    Locator, SplitLocator, Tag, TagElem,
};
use typst::layout::{
    Axis, BlockBody, BlockElem, ColumnsElem, Dir, HElem, PadElem, PlaceElem, StackChild, StackElem,
    VElem,
};
use typst::math::EquationElem;
use typst::model::{CiteGroup, DocumentInfo, Numbering, ParElem, ParbreakElem};
use typst::routines::{Arenas, FragmentKind, Pair, RealizationKind};
use typst::syntax::{Span, VirtualPath};
use typst_layout::PagedDocument;

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
    let (omml, notes) = crate::math::equation_to_omml_report(content);
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
    /// Any section declared inside/outside margins — see
    /// `CollectorState::two_sided_margins`.
    two_sided_margins: bool,
    section_span: Option<Span>,
    page_anchors: Vec<(usize, Span)>,
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
        let introspector = build_introspector(engine, content, &pairs, &state.fragment_tags);

        Ok(Self {
            ir,
            footnote_markers: state.footnote_markers,
            footnote_bodies: state.footnote_bodies,
            footnote_circled: state.footnote_circled,
            degradations: state.degradations,
            chain_overrides: state.chain_overrides,
            two_sided_margins: state.two_sided_margins,
            section_span: state.section_span,
            page_anchors: state.page_anchors,
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

fn build_introspector(
    engine: &mut Engine,
    original: &Content,
    pairs: &[Pair<'_>],
    fragment_tags: &[(Tag, usize)],
) -> ElementIntrospector<DocxPosition> {
    let mut sink = Sink::new();
    let mut probe = Engine {
        sink: sink.track_mut(),
        route: Route::extend(engine.route.track()).unnested(),
        ..*engine
    };
    let mut feed = DeferredTagFeed::new(original, fragment_tags);
    let mut locator = Locator::root().split();
    let mut extra: BTreeMap<usize, Vec<&Tag>> = BTreeMap::new();
    for (tag, index) in fragment_tags {
        extra.entry(*index).or_default().push(tag);
    }
    for (index, (content, styles)) in pairs.iter().enumerate() {
        let position = DocxPosition { pair: index };
        feed.visit(content, *styles, position, &mut probe, &mut locator);
        if let Some(tags) = extra.get(&index) {
            for tag in tags {
                feed.builder.discover_tag(tag, position);
            }
        }
    }
    if let Some(error) = feed.recursion_error {
        engine.sink.delayed_error(error);
    }
    feed.builder.finalize()
}

struct DeferredTagFeed<'a> {
    builder: ElementIntrospectorBuilder<DocxPosition>,
    layouters: HashMap<Span, &'a Content>,
    skipped_cites: HashSet<Location>,
    suppressed_equations: HashMap<(usize, Span), usize>,
    skipped_equation_ends: HashSet<Location>,
    active_fragments: HashSet<u128>,
    recursion_error: Option<SourceDiagnostic>,
}

impl<'a> DeferredTagFeed<'a> {
    fn new(original: &'a Content, fragment_tags: &[(Tag, usize)]) -> Self {
        let mut layouters = HashMap::new();
        index_deferred_containers(original, &mut layouters);
        let mut suppressed_equations = HashMap::new();
        for (tag, index) in fragment_tags {
            if let Tag::Start(inner, _) = tag
                && inner.is::<EquationElem>()
            {
                *suppressed_equations
                    .entry((*index, inner.span()))
                    .or_insert(0) += 1;
            }
        }
        Self {
            builder: ElementIntrospectorBuilder::new(),
            layouters,
            skipped_cites: HashSet::new(),
            suppressed_equations,
            skipped_equation_ends: HashSet::new(),
            active_fragments: HashSet::new(),
            recursion_error: None,
        }
    }

    fn visit(
        &mut self,
        content: &Content,
        styles: StyleChain<'_>,
        position: DocxPosition,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) {
        if let Some(tagged) = content.to_packed::<TagElem>() {
            if let Tag::Start(inner, _) = &tagged.tag
                && inner.is::<EquationElem>()
                && let Some(count) = self
                    .suppressed_equations
                    .get_mut(&(position.pair, inner.span()))
                && *count > 0
            {
                *count -= 1;
                if let Some(location) = inner.location() {
                    self.skipped_equation_ends.insert(location);
                }
                return;
            }
            if let Tag::End(location, ..) = &tagged.tag
                && self.skipped_equation_ends.remove(location)
            {
                return;
            }
            self.discover_tag(&tagged.tag, position);
        } else if let Some(styled) = content.to_packed::<StyledElem>() {
            self.visit(
                &styled.child,
                styles.chain(&styled.styles),
                position,
                engine,
                locator,
            );
        } else if let Some(par) = content.to_packed::<ParElem>() {
            self.visit(&par.body, styles, position, engine, locator);
        } else if let Some(place) = content.to_packed::<PlaceElem>() {
            self.fragment(&place.body, styles, position, engine, locator);
        } else if let Some(stack) = content.to_packed::<StackElem>() {
            let mut locator = stack_locator(locator, content.span());
            for child in stack_child_bodies(stack, stack.dir.get(styles)) {
                self.fragment(
                    &stack_fragment_body(child),
                    styles,
                    position,
                    engine,
                    &mut locator,
                );
            }
        } else if let Some(block) = content.to_packed::<BlockElem>() {
            match block.body.get_ref(styles) {
                Some(BlockBody::Content(body)) => {
                    self.fragment(body, styles, position, engine, locator);
                }
                Some(BlockBody::SingleLayouter(_) | BlockBody::MultiLayouter(_)) => {
                    if let Some(original) = self.layouters.remove(&content.span()) {
                        if original.is::<StackElem>() {
                            self.visit(original, styles, position, engine, locator);
                        } else {
                            for child in collect::content_children(original) {
                                self.fragment(child, styles, position, engine, locator);
                            }
                        }
                        self.layouters.insert(content.span(), original);
                    }
                }
                None => {}
            }
        } else {
            for child in collect::content_children(content) {
                if !std::ptr::eq(child, content) {
                    self.visit(child, styles, position, engine, locator);
                }
            }
        }
    }

    fn fragment(
        &mut self,
        body: &Content,
        styles: StyleChain<'_>,
        position: DocxPosition,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) {
        if self.recursion_error.is_some() {
            return;
        }
        let key = typst_utils::hash128(&(body, styles.to_map()));
        if !self.active_fragments.insert(key) {
            self.recursion_error = Some(SourceDiagnostic::error(
                body.span(),
                "recursive layout reenters the same deferred content",
            ));
            return;
        }
        engine.route.increase();
        match engine.route.check_layout_depth().at(body.span()) {
            Ok(()) => self.realize_fragment(body, styles, position, engine, locator),
            Err(errors) => self.recursion_error = errors.into_iter().next(),
        }
        engine.route.decrease();
        self.active_fragments.remove(&key);
    }

    fn realize_fragment(
        &mut self,
        body: &Content,
        styles: StyleChain<'_>,
        position: DocxPosition,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) {
        let mut kind = FragmentKind::Block;
        let arenas = Arenas::default();
        let mut locator = locator.next(&body.span()).split();
        let route = engine.route.clone();
        let pairs = match realize(
            RealizationKind::Fragment { kind: &mut kind },
            engine,
            &mut locator,
            &arenas,
            body,
            styles,
        ) {
            Ok(pairs) => pairs,
            Err(errors) => {
                if engine.route.check_show_depth().is_err() {
                    self.recursion_error = errors.into_iter().next();
                }
                engine.route = route;
                return;
            }
        };
        for (content, styles) in pairs {
            self.visit(content, styles, position, engine, &mut locator);
        }
    }

    fn discover_tag(&mut self, tag: &Tag, position: DocxPosition) {
        match tag {
            Tag::Start(inner, _)
                if !self.active_fragments.is_empty()
                    && inner.to_packed::<CiteGroup>().is_some() =>
            {
                self.skipped_cites.extend(inner.location());
            }
            Tag::End(location, ..) if self.skipped_cites.remove(location) => {}
            tag => {
                self.builder.discover_tag(tag, position);
                if self.active_fragments.is_empty()
                    && let Tag::Start(inner, _) = tag
                {
                    collect_nested_tags(inner, position, &mut self.builder);
                }
            }
        }
    }
}

fn stack_locator<'a>(locator: &mut SplitLocator<'a>, span: Span) -> SplitLocator<'a> {
    locator.next(&("stack", span)).split()
}

fn stack_child_bodies(stack: &StackElem, dir: Dir) -> impl Iterator<Item = &Content> {
    stack.children.iter().filter_map(move |child| {
        let StackChild::Block(body) = child else {
            return None;
        };
        match dir.axis() {
            Axis::X if body.is::<HElem>() => None,
            Axis::Y if body.is::<VElem>() => None,
            _ => Some(body),
        }
    })
}

fn stack_fragment_body(body: &Content) -> Content {
    (body.clone() + ParbreakElem::shared().clone()).spanned(body.span())
}

fn index_deferred_containers<'a>(content: &'a Content, layouters: &mut HashMap<Span, &'a Content>) {
    let span = content.span();
    if span.id().is_some()
        && (content.to_packed::<ColumnsElem>().is_some()
            || content.to_packed::<PadElem>().is_some()
            || content.to_packed::<StackElem>().is_some())
    {
        layouters.entry(span).or_insert(content);
    }
    for child in collect::content_children(content) {
        if !std::ptr::eq(child, content) {
            index_deferred_containers(child, layouters);
        }
    }
}

/// Layouter bodies hold closures, not content, so only plain `Content`
/// bodies recurse.
fn collect_nested_tags(
    content: &Content,
    position: DocxPosition,
    builder: &mut ElementIntrospectorBuilder<DocxPosition>,
) {
    if content.is::<BlockElem>() {
        return;
    }
    if let Some(tagged) = content.to_packed::<TagElem>() {
        builder.discover_tag(&tagged.tag, position);
        if let Tag::Start(inner, _) = &tagged.tag {
            collect_nested_tags(inner, position, builder);
        }
        return;
    }
    for child in collect::content_children(content) {
        if !std::ptr::eq(child, content) {
            collect_nested_tags(child, position, builder);
        }
    }
}

pub(super) fn with_fragment<R>(
    engine: &mut Engine,
    locator: &mut SplitLocator,
    content: &Content,
    styles: StyleChain<'_>,
    f: impl FnOnce(&mut Engine, &mut SplitLocator, &[Pair<'_>]) -> SourceResult<R>,
) -> SourceResult<R> {
    let fragment = locator.next(&content.span()).split();
    with_fragment_at(engine, fragment, content, styles, |engine, _, pairs| {
        f(engine, locator, pairs)
    })
}

fn with_fragment_at<R>(
    engine: &mut Engine,
    mut locator: SplitLocator,
    content: &Content,
    styles: StyleChain<'_>,
    f: impl FnOnce(&mut Engine, &mut SplitLocator, &[Pair<'_>]) -> SourceResult<R>,
) -> SourceResult<R> {
    let mut kind = FragmentKind::Block;
    let frag_arenas = Arenas::default();
    let pairs = realize(
        RealizationKind::Fragment { kind: &mut kind },
        engine,
        &mut locator,
        &frag_arenas,
        content,
        styles,
    )?;
    f(engine, &mut locator, &pairs)
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
    let paged_doc = match paged_result.output {
        Ok(doc) => doc,
        Err(errors) => return Err(errors.into_iter().collect()),
    };
    Ok(export_ir(&docx_doc, Some(&paged_doc), world, options))
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
            | IrBlock::Place { body, .. }
            | IrBlock::Columns { body, .. } => {
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
    compat: CompatMode,
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
    super::source::report_mixed_margin_mirroring(&styles, compat, degradations);
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
    // `w:mirrorMargins` is document-global: it reflects *any* section's
    // inside/outside margins, accumulated across page states the same way
    // `one_sided_asymmetric_margins` is — the chain read of
    // `margins_mirrored` is last-wins and masks an earlier mirrored
    // section. `chain_overrides` itself keeps the chain value so C1's
    // report contract is unchanged.
    let mut chain = docx_doc.chain_overrides.clone();
    chain.margins_mirrored |= docx_doc.two_sided_margins;
    apply_page_and_style_ir(
        &chain,
        world,
        paged_doc,
        &mut tmp,
        options.compat,
        &mut degradations,
    );
    let section_break = !ir.sections.is_empty();
    if options.compat == CompatMode::C1 {
        ir.sections.clear();
    }
    let section_footer = paged_doc
        .filter(|_| !ir.sections.is_empty())
        .and_then(|paged| prepare_numbered_sections(&mut ir, paged, &mut tmp));
    let numbered_blank_pages = if section_footer.is_some() {
        paged_doc.map_or_else(BTreeMap::new, |paged| {
            source_blank_pages(&ir, &docx_doc.page_anchors, paged)
        })
    } else {
        BTreeMap::new()
    };
    apply_headers_and_footers(
        paged_doc,
        world,
        &mut tmp,
        options.compat,
        &mut degradations,
        &place::place_location_hashes(&ir.body),
        section_footer,
    );
    if ir.sections.is_empty() {
        ir.page = tmp.page_settings.clone();
    }
    ir.style = tmp.style.clone();
    if docx_doc.footnote_circled {
        ir.style.footnote_format = FootnoteFormat::CircledNumber;
    }

    measure_heading_sizes_from_ir(&mut ir);

    ir.citation_sources = extract_bibliography_sources(docx_doc.introspector(), world);

    if ir.info.title.is_none() {
        ir.info.title = first_heading_text(&ir);
    }

    let geometry = geometry_input(tmp, numbered_blank_pages);

    if let Some(paged) = paged_doc {
        let style = ir.style.clone();
        let patch_degradations = patch_document(
            &mut ir,
            &docx_doc.footnote_markers,
            &docx_doc.footnote_bodies,
            paged,
            &style,
            Some(world.main_source()),
            options.compat,
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
    run_sizes::strip_redundant(&mut doc);
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

fn geometry_input(doc: Document, numbered_blank_pages: BTreeMap<usize, usize>) -> GeometryInput {
    GeometryInput {
        page_size: (
            doc.page_settings.width_twips,
            doc.page_settings.height_twips,
        ),
        page_numbering: doc.page_numbering,
        page_number_start: doc.page_number_start,
        supplement: None,
        header: doc.header,
        header_even: doc.header_even,
        header_first: doc.header_first,
        footer: doc.footer,
        footer_even: doc.footer_even,
        footer_first: doc.footer_first,
        page_number_footers: doc.page_number_footers,
        even_and_odd_headers: doc.even_and_odd_headers,
        mirror_margins: doc.mirror_margins,
        title_page: doc.title_page,
        numbered_blank_pages,
    }
}

fn prepare_numbered_sections(
    ir: &mut IrDocument,
    paged: &PagedDocument,
    doc: &mut Document,
) -> Option<HeaderFooter> {
    let rendered = paged.pages();
    let first = rendered.first()?;
    let formats: Vec<_> = rendered
        .iter()
        .map(|p| page::numbering_page_format(p.numbering.as_ref()))
        .collect();
    let footers = rendered
        .iter()
        .map(page::numbered_footer)
        .collect::<Result<Vec<_>, _>>()
        .ok();
    let first_footer = footers.as_ref().and_then(|all| all.iter().flatten().next());
    let footer_at = |index: usize| footers.as_ref().and_then(|all| all[index].as_ref());
    let show_footer = |fmt, settings: &_| fmt && *settings != SectionFooter::Omitted;
    let mut previous = (
        ir.first_numbering,
        show_footer(ir.first_numbering.is_some(), &ir.page.footer_reference),
    );
    if ir.first_numbering.is_some() && first.number != 1 {
        doc.page_number_start = u32::try_from(first.number).ok();
    }
    let mut index = 0;
    let mut seen_footer = false;
    if first_footer.is_some() {
        set_section_footer(
            &mut ir.page,
            footer_at(index).map(|f| f.distance),
            &mut seen_footer,
        );
    }
    for section in &mut ir.sections {
        let target = (
            section.numbering,
            show_footer(
                section.numbering.is_some(),
                &section.settings.footer_reference,
            ),
        );
        if target != previous
            && let Some(next) = (index + 1..rendered.len()).find(|&i| {
                formats[i] == target.0
                    && (first_footer.is_none() || footer_at(i).is_some() == target.1)
            })
        {
            index = next;
            let number = rendered[next].number;
            if section.numbering.is_some()
                && (previous.0.is_none() || number != rendered[next - 1].number.saturating_add(1))
            {
                section.numbering_start = u32::try_from(number).ok();
            }
        }
        if first_footer.is_some() {
            let distance = if target.1 && formats[index] == target.0 {
                footer_at(index).map(|f| f.distance)
            } else {
                None
            };
            set_section_footer(&mut section.settings, distance, &mut seen_footer);
        } else {
            section.settings.footer_reference = SectionFooter::Shared;
        }
        previous = target;
    }
    first_footer.map(|f| f.part.clone())
}

fn set_section_footer(
    settings: &mut crate::ooxml::document::PageSettings,
    distance: Option<u32>,
    seen_footer: &mut bool,
) {
    settings.footer_reference = if let Some(distance) = distance {
        settings.footer_twips = Some(distance);
        *seen_footer = true;
        SectionFooter::Shared
    } else if *seen_footer {
        SectionFooter::Empty
    } else {
        SectionFooter::Omitted
    };
}

fn source_blank_pages(
    ir: &IrDocument,
    anchors: &[(usize, Span)],
    paged: &PagedDocument,
) -> BTreeMap<usize, usize> {
    let mut spans = HashMap::new();
    let mut blank = Vec::new();
    let wanted: HashSet<_> = anchors.iter().map(|&(_, span)| span).collect();
    for (index, page) in paged.pages().iter().enumerate() {
        if let Some(body) = page::body_spans(&page.frame) {
            for span in body.into_iter().filter(|s| wanted.contains(s)) {
                let range = spans.entry(span).or_insert((index, index));
                range.1 = index;
            }
        } else {
            blank.push(index);
        }
    }
    let mut found = BTreeMap::new();
    let mut previous = None;
    for &(block, span) in anchors {
        let Some(&(start, end)) = spans.get(&span) else {
            continue;
        };
        if blank
            .get(blank.partition_point(|&p| p < start))
            .is_some_and(|&p| p < end)
        {
            continue;
        }
        if let Some((prev_block, prev_page)) = previous
            && start > prev_page + 1
            && let Some(boundary) =
                locate_blank_boundary(ir, prev_block, block, prev_page, start, &blank, paged)
        {
            found.insert(boundary, start - prev_page - 1);
        }
        if previous.is_none_or(|(_, prev_page)| end >= prev_page) {
            previous = Some((block, end));
        }
    }
    found
}

fn locate_blank_boundary(
    ir: &IrDocument,
    left: usize,
    right: usize,
    first_page: usize,
    last_page: usize,
    blank: &[usize],
    paged: &PagedDocument,
) -> Option<usize> {
    let gap = first_page + 1..last_page;
    if !gap.clone().all(|p| blank.binary_search(&p).is_ok()) {
        return None;
    }
    let blocks = &ir.body[left + 1..right];
    let parity = blocks
        .iter()
        .any(|b| matches!(b, IrBlock::PageBreak { to: Some(_), .. }));
    if parity && gap.clone().all(|p| paged.pages()[p].numbering.is_none()) {
        return None;
    }
    let explicit_break = blocks
        .iter()
        .rposition(|b| matches!(b, IrBlock::PageBreak { .. }))
        .map(|index| left + 2 + index);
    let section_start = ir
        .sections
        .iter()
        .rfind(|s| (left + 1..=right).contains(&s.start_block))
        .map(|s| s.start_block);
    explicit_break.max(section_start)
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
    doc: &mut crate::ooxml::document::Document,
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

#[cfg(test)]
mod tests {
    use super::*;
    use typst::diag::{FileError, FileResult};
    use typst::foundations::{Bytes, Datetime, Duration, NativeElement};
    use typst::introspection::MetadataElem;
    use typst::syntax::{FileId, Source};
    use typst::text::{Font, FontBook};
    use typst::utils::LazyHash;
    use typst::{Library, LibraryExt};

    struct ProbeWorld {
        source: Source,
        book: LazyHash<FontBook>,
        library: LazyHash<Library>,
    }

    impl World for ProbeWorld {
        fn library(&self) -> &LazyHash<Library> {
            &self.library
        }

        fn book(&self) -> &LazyHash<FontBook> {
            &self.book
        }

        fn main(&self) -> FileId {
            self.source.id()
        }

        fn source(&self, id: FileId) -> FileResult<Source> {
            if id == self.main() {
                Ok(self.source.clone())
            } else {
                Err(FileError::AccessDenied)
            }
        }

        fn file(&self, id: FileId) -> FileResult<Bytes> {
            Ok(Bytes::from_string(self.source(id)?.text().to_owned()))
        }

        fn font(&self, _: usize) -> Option<Font> {
            None
        }

        fn today(&self, _: Option<Duration>) -> Option<Datetime> {
            None
        }
    }

    fn probe_docx(source: &str) -> SourceResult<DocxDocument> {
        let world = ProbeWorld {
            source: Source::detached(source),
            book: LazyHash::new(FontBook::new()),
            library: LazyHash::new(Library::default()),
        };
        typst::compile::<DocxDocument>(&world).output
    }

    #[test]
    fn deferred_show_rule_cannot_reenter_the_same_fragment() {
        // A fresh metadata node has no show-rule guard. The rule only
        // repeats when another pass opens the generated place's body.
        let errors = probe_docx(
            "#show metadata: it => place(top)[#metadata(it.value)]\n#metadata(\"probe\")",
        )
        .err()
        .expect("recursive layout must return a diagnostic, not overflow the stack");
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("reenters the same deferred content")),
            "{errors:?}"
        );
    }

    #[test]
    fn deferred_show_rules_with_changing_bodies_respect_layout_depth() {
        // Each recursive body differs, so a repeated-content check alone cannot stop it.
        let errors =
            probe_docx("#show metadata: it => place(top)[#metadata(it.value + 1)]\n#metadata(0)")
                .err()
                .expect("the layout depth must also bound changing fragments");
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("depth exceeded")),
            "{errors:?}"
        );
    }

    #[test]
    fn deferred_tag_probe_keeps_distinct_sibling_occurrences() {
        let docx =
            probe_docx("#let body = [#metadata(\"probe\")]\n#place(top, body)\n#place(top, body)")
                .expect("the repeated body in a sibling place is not recursive");
        let tags = docx.query(&MetadataElem::ELEM.select());
        assert_eq!(tags.len(), 2, "{tags:?}");
        assert_ne!(tags[0].location(), tags[1].location());
    }
}
