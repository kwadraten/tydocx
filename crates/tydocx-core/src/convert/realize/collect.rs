use std::collections::{BTreeMap, HashMap, HashSet};

use codex::numeral_systems::NamedNumeralSystem;
use tydocx_ir::{
    Degradation, DegradationAction, DocumentStyle, IrBibEntry, IrBlock, IrBorder, IrCaptionSeq,
    IrCellBorderFlags, IrCellBorders, IrDocument, IrDocumentInfo, IrEnum, IrEnumItem, IrEquation,
    IrEquationNumber, IrFigureKind, IrGrid, IrGridLine, IrHeadingNumbering, IrInline,
    IrLineSpacing, IrList, IrListItem, IrNumberingPattern, IrNumberingPiece, IrNumeralSystem,
    IrOutlineEntry, IrParStyle, IrParagraph, IrPlaceWrap, IrRaw, IrSection, IrSectionBreakType,
    IrSeqSplit, IrShapeKind, IrTable, IrTableCell, IrTableRow, IrTrackSize, IrVMerge, PageSettings,
    TableBorders, TableCellMargins,
};
use tydocx_ooxml::document::{
    Alignment, ParagraphStyle, Run, VerticalAlign, truncate_bookmark_name,
};
use typst_library::World;
use typst_library::diag::{At, SourceResult};
use typst_library::engine::Engine;
use typst_library::foundations::{
    Content, ContextElem, Datetime, Element, LocatableSelector, NativeElement, Packed, Resolve,
    SequenceElem, Smart, StyleChain, StyledElem, Value,
};
use typst_library::introspection::{
    Counter, Location, QueryIntrospection, SplitLocator, Tag, TagElem,
};
use typst_library::layout::grid::Celled;
use typst_library::layout::grid::GridCell;
use typst_library::layout::grid::resolve::{Cell, CellGrid, Entry, Line, LinePosition};
use typst_library::layout::{
    Abs, AlignElem, Axes, BlockBody, BlockElem, BoxElem, ColbreakElem, ColumnsElem, Frame,
    FrameItem, GridChild, GridElem, GridFooter, GridHeader, GridItem, HAlignment, HideElem, Length,
    Margin, MoveElem, PadElem, PageElem, PagebreakElem, Parity, PlaceElem, Region, Rel, RepeatElem,
    RotateElem, ScaleElem, Sides, Size, Sizing, SkewElem, Spacing, StackChild, StackElem,
    VAlignment, VElem,
};
use typst_library::math::EquationElem;
use typst_library::model::{
    BibliographyElem, CiteGroup, Destination, DirectLinkElem, EmphElem, EnumElem, EnumItem,
    FigureCaption, FigureElem, FigureKind, FootnoteBody, FootnoteElem, HeadingElem, LinkElem,
    LinkTarget, ListElem, ListItem, ListMarker, Numbering, OutlineElem, OutlineIndent, ParElem,
    ParbreakElem, QuoteElem, RenderedBibliography, RenderedEntry, StrongElem, Supplement,
    TableCell, TableChild, TableElem, TableFooter, TableHeader, TableItem, TermItem, TermsElem,
    Works,
};
use typst_library::pdf::{PdfMarkerTag, PdfMarkerTagKind};
use typst_library::routines::Pair;
use typst_library::text::{
    HighlightElem, OverlineElem, RawElem, RawLine, SmallcapsElem, SmartQuoter, SpaceElem,
    StrikeElem, SubElem, SuperElem, TextElem, UnderlineElem,
};
use typst_library::visualize::{
    CircleElem, CurveElem, EllipseElem, ImageElem, LineElem, PolygonElem, RectElem, SquareElem,
};
use typst_syntax::Source;
use typst_syntax::Span;
use typst_utils::Get;

use super::inline::{
    FootnoteMarker, InlineCtx, ParLayout, alignment_from_styles, collect_fragment_inlines,
    collect_inline_paragraph, collect_inlines, collect_paragraph, image_inline,
    par_style_from_styles, smart_quotes_for, spacing_twips,
};
use super::page;
use super::shape::{
    is_default_full_width_rule, merge_stack_line_paths, native_border_style_is_lossy, resolve_shape,
};
use super::style::{
    StyleDefaults, additional_root_styles, declared_root_fonts, defaults_from_root,
    monospace_families, raw_text_styles, run_from_styles, text_size_half_pt,
};
use super::{
    stack_child_bodies, stack_fragment_body, stack_locator, with_fragment, with_fragment_at,
};
use crate::convert::table_align::paint_to_shd_hex;
use crate::convert::table_width::{TableWidthCtx, track_widths_pct};

pub struct CollectorState {
    pub footnote_markers: Vec<FootnoteMarker>,
    pub footnote_bodies: HashMap<Location, Vec<IrParagraph>>,
    pub footnote_circled: bool,
    pub degradations: Vec<Degradation>,
    pub fragment_tags: Vec<(Tag, usize)>,
    pub chain_overrides: page::SourceStyleOverrides,
    /// Any top-level page state declared inside/outside margins — OR'd into
    /// `margins_mirrored` for the C2/C3 flag and the mixed-margin report;
    /// `chain_overrides.margins_mirrored` keeps the chain's last-wins read
    /// so C1's report contract is unchanged.
    pub two_sided_margins: bool,
    /// Span of the pair whose ambient page settings first diverged — the
    /// `section-break` degradation's location.
    pub section_span: Option<Span>,
    pub page_anchors: Vec<(usize, Span)>,
}

#[derive(Default)]
struct EquationNumbering {
    seq_count: u64,
    refs: HashMap<String, (String, Option<String>)>,
    bookmarks: HashSet<String>,
    next_bookmark: usize,
}

pub struct Collector<'a> {
    defaults: StyleDefaults,
    normal_par_layout: ParLayout,
    label_map: HashMap<Location, String>,
    bib_map: HashMap<Location, String>,
    cite_span_map: HashMap<Span, String>,
    cite_render_map: HashMap<Span, String>,
    fragment_tags: Vec<(Tag, usize)>,
    current_pair: usize,
    block_depth: usize,
    /// Heading `Location` → synthesized TOC bookmark (`outline-target-N`),
    /// registered for unlabelled headings an `#outline` targets.
    heading_bookmarks: HashMap<Location, String>,
    heading_bookmark_spans: HashMap<(Element, Span), Option<String>>,
    /// `OutlineElem` location → resolved TOC entries, precomputed in `pass_one`
    /// (entry collection registers heading bookmarks, which headings consume
    /// later in document order — the outline itself precedes them).
    outline_entries: HashMap<Location, OutlineEntries>,
    /// Next synthesized `outline-target-N` bookmark index — monotonic so
    /// rebuilds and multiple outlines cannot collide on the same name.
    next_outline_bookmark: usize,
    pending_bibliographies: HashMap<Location, bool>,
    deferred_bibliographies: HashMap<Span, Vec<Location>>,
    works: Option<std::sync::Arc<Works>>,
    footnote_markers: Vec<FootnoteMarker>,
    footnote_bodies: HashMap<Location, Vec<IrParagraph>>,
    footnote_circled: bool,
    degradations: Vec<Degradation>,
    equations: EquationNumbering,
    source: Option<Source>,
    layouter_index: HashMap<Span, Content>,
    stack_layouter_index: Option<HashMap<Span, Content>>,
    page_snapshot: Option<PageSnapshot>,
    first_snapshot: Option<PageSnapshot>,
    sections: Vec<IrSection>,
    page_anchors: Vec<(usize, Span)>,
    section_span: Option<Span>,
    /// A top-level page state carried one-sided margins with left ≠ right —
    /// combined with `margins_mirrored` it means the document-global
    /// `w:mirrorMargins` also swaps that state's margins on even pages.
    one_sided_asymmetric_margins: bool,
    /// A top-level page state declared inside/outside margins —
    /// `w:mirrorMargins` is document-global, so any section's `two_sided`
    /// sets it, accumulated here because the chain read is last-wins.
    two_sided_margins: bool,
    /// The `\@` picture `document.date` renders in its kind-dependent auto
    /// format, resolved from `DocumentInfo::date` before collection.
    doc_date_picture: Option<&'static str>,
    _marker: std::marker::PhantomData<&'a ()>,
}

const BODY_DEFAULT_SAMPLE_GLYPHS: usize = 9_000;

fn defaults_from_pairs(pairs: &[Pair<'_>], root_styles: StyleChain<'_>) -> StyleDefaults {
    let fallback = defaults_from_root(root_styles);
    let declared_size = fallback.inherited_size_half_pt.is_some();
    let declared_font = root_styles.has(TextElem::font);
    if declared_size && declared_font {
        return fallback;
    }
    let mut sizes: HashMap<u32, usize> = HashMap::new();
    let mut first_fonts: HashMap<String, usize> = HashMap::new();
    let mut last_fonts: HashMap<String, usize> = HashMap::new();
    let mut sampled = 0usize;
    for (content, styles) in pairs {
        let Some(par) = content.to_packed::<ParElem>() else {
            continue;
        };
        let weight = shallow_plain_text(&par.body).chars().count();
        if weight == 0 {
            continue;
        }
        if !declared_size {
            *sizes.entry(text_size_half_pt(*styles)).or_default() += weight;
        }
        // The last-family vote deliberately uses `families()` including
        // the emoji fallback tail, so it is constant: taking the last
        // DECLARED family instead was tried and regressed the CJK fixtures.
        if !declared_font {
            let families = typst_library::text::families(*styles);
            if let Some(first) = families.clone().next() {
                *first_fonts.entry(first.as_str().to_owned()).or_default() += weight;
            }
            if let Some(last) = families.last() {
                *last_fonts.entry(last.as_str().to_owned()).or_default() += weight;
            }
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
        inherited_size_half_pt: fallback.inherited_size_half_pt,
        body_font: first_fonts
            .into_iter()
            .max_by_key(|(_, weight)| *weight)
            .map_or(fallback.body_font, |(font, _)| font),
        font_east_asia: last_fonts
            .into_iter()
            .max_by_key(|(_, weight)| *weight)
            .map_or(fallback.font_east_asia, |(font, _)| font),
        monospace_families: Vec::new(),
    }
}

/// The `\@` picture `document.date` renders in Typst's kind-dependent auto
/// format — `date` → ISO date, `datetime` → ISO date + time, `time` → ISO
/// time; `auto` resolves to `world.today()`, a date. `None` when the set
/// value is `none` (no datetime to format).
fn doc_date_picture(info: &typst_library::model::DocumentInfo) -> Option<&'static str> {
    use typst_library::foundations::Datetime;
    match &info.date {
        Smart::Auto | Smart::Custom(Some(Datetime::Date(_))) => Some("yyyy-MM-dd"),
        Smart::Custom(Some(Datetime::Datetime(_))) => Some("yyyy-MM-dd HH:mm:ss"),
        Smart::Custom(Some(Datetime::Time(_))) => Some("HH:mm:ss"),
        Smart::Custom(None) => None,
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
    let content_pairs: Vec<_> = pairs
        .iter()
        .copied()
        .filter(|(content, _)| !content.is::<PagebreakElem>())
        .collect();
    let document_styles = additional_root_styles(&content_pairs, root_styles);
    let document_styles = root_styles.chain(&document_styles);
    let mut defaults = defaults_from_pairs(pairs, document_styles);
    defaults.monospace_families = monospace_families(engine.world.book(), &defaults.body_font);
    let mut layouter_index = HashMap::new();
    index_content_tree(original, &mut layouter_index);
    let mut collector = Collector {
        defaults,
        normal_par_layout: ParLayout::from_styles(document_styles),
        label_map: HashMap::new(),
        bib_map: HashMap::new(),
        cite_span_map: HashMap::new(),
        cite_render_map: HashMap::new(),
        fragment_tags: Vec::new(),
        current_pair: 0,
        block_depth: 0,
        heading_bookmarks: HashMap::new(),
        heading_bookmark_spans: HashMap::new(),
        outline_entries: HashMap::new(),
        next_outline_bookmark: 0,
        pending_bibliographies: HashMap::new(),
        deferred_bibliographies: HashMap::new(),
        works: None,
        footnote_markers: Vec::new(),
        footnote_bodies: HashMap::new(),
        footnote_circled: false,
        degradations: Vec::new(),
        equations: EquationNumbering::default(),
        source,
        layouter_index,
        stack_layouter_index: None,
        page_snapshot: None,
        first_snapshot: None,
        sections: Vec::new(),
        page_anchors: Vec::new(),
        section_span: None,
        one_sided_asymmetric_margins: false,
        two_sided_margins: false,
        doc_date_picture: info.date_picture,
        _marker: std::marker::PhantomData,
    };
    collector.pass_one(pairs, engine, locator)?;
    let body = collector.collect_blocks(pairs, engine, locator, FlowScope::Document)?;
    let sections = std::mem::take(&mut collector.sections);
    let first_snapshot = collector.first_snapshot.take();
    let mut chain_overrides = super::chain::chain_overrides(pairs);
    chain_overrides.text_size_half_pt = collector.defaults.inherited_size_half_pt;
    chain_overrides.text_font = declared_root_fonts(document_styles, collector.source.as_ref());
    chain_overrides.one_sided_asymmetric_margins |= collector.one_sided_asymmetric_margins;
    let state = CollectorState {
        fragment_tags: std::mem::take(&mut collector.fragment_tags),
        footnote_markers: std::mem::take(&mut collector.footnote_markers),
        footnote_bodies: std::mem::take(&mut collector.footnote_bodies),
        footnote_circled: collector.footnote_circled,
        degradations: std::mem::take(&mut collector.degradations),
        chain_overrides,
        two_sided_margins: collector.two_sided_margins,
        section_span: collector.section_span,
        page_anchors: collector.page_anchors,
    };
    let mut page_furniture = Vec::new();
    for content in [
        root_styles.get_cloned(PageElem::foreground),
        root_styles.get_cloned(PageElem::background),
    ]
    .into_iter()
    .flatten()
    {
        let text = plain_text(&content);
        if !text.is_empty() {
            page_furniture.push(text);
        }
    }
    let doc = IrDocument {
        info: info.clone(),
        page: if sections.is_empty() {
            PageSettings::default()
        } else {
            first_snapshot
                .as_ref()
                .map_or_else(PageSettings::default, PageSnapshot::to_page_settings)
        },
        style: DocumentStyle::default(),
        body,
        equation_refs: collector.equations.refs,
        sections,
        first_numbering: first_snapshot
            .as_ref()
            .and_then(|s| page::numbering_page_format(s.numbering.as_ref())),
        footnotes: Vec::new(),
        citation_sources: Vec::new(),
        page_furniture,
    };
    Ok((doc, state))
}

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
        description: info.description.as_ref().map(ToString::to_string),
        keywords: info.keywords.iter().map(ToString::to_string).collect(),
        created: match &info.date {
            Smart::Custom(Some(date)) => datetime_iso(date),
            _ => None,
        },
        date_picture: doc_date_picture(info),
    }
}

fn datetime_iso(date: &Datetime) -> Option<String> {
    match date {
        Datetime::Date(d) => Some(format!(
            "{:04}-{:02}-{:02}T00:00:00Z",
            d.year(),
            u8::from(d.month()),
            d.day()
        )),
        Datetime::Datetime(dt) => Some(format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
            dt.year(),
            u8::from(dt.month()),
            dt.day(),
            dt.hour(),
            dt.minute(),
            dt.second()
        )),
        Datetime::Time(_) => None,
    }
}

#[derive(Debug, Clone, Default)]
struct ListCtx {
    depth: u8,
    inherited: Option<(IrNumberingPattern, u8)>,
}

#[derive(Default)]
struct BodyBlocks {
    blocks: Vec<IrBlock>,
    inherited_after: Option<(usize, u32)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FlowScope {
    Document,
    Page,
    Nested,
}

impl FlowScope {
    fn within_tag(self) -> Self {
        match self {
            Self::Document | Self::Page => Self::Page,
            Self::Nested => Self::Nested,
        }
    }
}

#[derive(Default)]
struct VerticalSpace {
    strong: u32,
    weak: u32,
}

impl VerticalSpace {
    fn add(
        &mut self,
        v: &Packed<VElem>,
        styles: StyleChain<'_>,
        degradations: &mut Vec<Degradation>,
    ) {
        if let Some(twips) = spacing_twips(&v.amount, styles) {
            if v.weak.get(styles) {
                self.weak = self.weak.max(twips);
            } else {
                self.strong = self.strong.saturating_add(twips);
            }
        } else {
            super::inline::report_spacing_gap(&v.amount, styles, false, v.span(), degradations);
        }
    }

    fn apply(&mut self, blocks: &mut Vec<IrBlock>, base: usize, styles: StyleChain<'_>) -> usize {
        if base == blocks.len() {
            return base;
        }
        let Self { strong, weak } = std::mem::take(self);
        let table = matches!(blocks[base], IrBlock::Table(_) | IrBlock::Grid(_));
        let gap = strong.saturating_add(weak);
        if let Some(IrBlock::Paragraph(para)) = blocks.get_mut(base) {
            if gap != 0 {
                para.spacing_before = Some(
                    para.spacing_before
                        .unwrap_or(0)
                        .max(weak)
                        .saturating_add(strong),
                );
            }
            return base;
        }
        if matches!(blocks[base], IrBlock::PageBreak { .. }) {
            return base;
        }
        if let Some(IrBlock::Paragraph(para)) = base.checked_sub(1).and_then(|i| blocks.get_mut(i))
        {
            if gap != 0 || (table && para.par.spacing_after.is_some()) {
                let table_after = if table {
                    ParLayout::from_styles(styles).spacing_after(styles.resolve(TextElem::size))
                } else {
                    0
                };
                let after = para
                    .spacing_after
                    .unwrap_or_else(|| para.par.spacing_after.unwrap_or_default().max(table_after));
                para.spacing_after = Some(after.max(weak).saturating_add(strong));
            }
            return base;
        }
        if gap == 0 {
            return base;
        }
        blocks.insert(
            base,
            IrBlock::Paragraph(IrParagraph {
                par: IrParStyle {
                    line_spacing: Some(IrLineSpacing::Exact(gap)),
                    ..IrParStyle::default()
                },
                spacing_before: Some(0),
                spacing_after: Some(0),
                suppress_indent: true,
                keep_next: true,
                ..IrParagraph::default()
            }),
        );
        base + 1
    }
}

impl BodyBlocks {
    fn push(&mut self, block: IrBlock) {
        if matches!(block, IrBlock::Paragraph(_)) {
            self.finish_inherited_after();
        }
        self.blocks.push(block);
    }

    fn extend(&mut self, blocks: impl Into<Self>) {
        let other = blocks.into();
        if other
            .blocks
            .iter()
            .any(|block| matches!(block, IrBlock::Paragraph(_)))
        {
            self.finish_inherited_after();
            self.inherited_after = other
                .inherited_after
                .map(|(index, gap)| (self.blocks.len() + index, gap));
        }
        self.blocks.extend(other.blocks);
    }

    fn finish_inherited_after(&mut self) {
        if let Some((index, gap)) = self.inherited_after.take()
            && let Some(IrBlock::Paragraph(para)) = self.blocks.get_mut(index)
        {
            para.spacing_after = Some(para.spacing_after.unwrap_or_default().saturating_add(gap));
        }
    }

    fn apply_vspace(&mut self, pending: &mut VerticalSpace, base: usize, styles: StyleChain<'_>) {
        if pending.apply(&mut self.blocks, base, styles) != base
            && let Some((index, _)) = &mut self.inherited_after
            && *index >= base
        {
            *index += 1;
        }
    }

    fn finish(mut self) -> Vec<IrBlock> {
        self.finish_inherited_after();
        self.blocks
    }
}

impl From<Vec<IrBlock>> for BodyBlocks {
    fn from(blocks: Vec<IrBlock>) -> Self {
        Self {
            blocks,
            inherited_after: None,
        }
    }
}

/// The entries `build_outline_entries` resolved for one `#outline`.
#[derive(Clone)]
struct OutlineEntries {
    entries: Vec<IrOutlineEntry>,
    /// `SEQ` identifier when every entry is a figure/equation caption —
    /// selects the Word `TOC \c "Id"` field instead of the heading `\o`
    /// switches.
    figure_caption_id: Option<String>,
    /// Any entry carried a numbering prefix — the `auto` outline indent
    /// then follows each level's prefix width, so the per-level fallback
    /// is an approximation.
    numbered: bool,
}

struct FigureMeta {
    kind: IrFigureKind,
    labels: Vec<String>,
    custom_placement: bool,
    gap_twips: Option<u32>,
    location_hash: Option<u64>,
}

impl Collector<'_> {
    fn pass_one(
        &mut self,
        pairs: &[Pair<'_>],
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<()> {
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
        // TOC entries resolve against the full introspector and register
        // synthesized bookmarks on their targets — ahead of document order,
        // so an outline anywhere (including at the end) finds every target
        // already book-marked when it is collected.
        for (content, styles) in pairs {
            let inner = if let Some(tagged) = content.to_packed::<TagElem>() {
                match &tagged.tag {
                    Tag::Start(inner, _) => inner,
                    Tag::End(..) => continue,
                }
            } else {
                *content
            };
            let Some(outline) = inner.to_packed::<OutlineElem>() else {
                continue;
            };
            if let Some(location) = outline.location() {
                let resolved = self.build_outline_entries(outline, *styles, engine, locator)?;
                self.outline_entries.insert(location, resolved);
            }
        }
        self.prepare_bibliographies(pairs, engine);
        self.resolve_cite_targets(engine);
        Ok(())
    }

    fn prepare_bibliographies(&mut self, pairs: &[Pair<'_>], engine: &mut Engine) {
        let bibliographies = engine.introspect(QueryIntrospection(
            BibliographyElem::ELEM.select(),
            Span::detached(),
        ));
        let Some(first) = bibliographies.first() else {
            return;
        };
        let tagged_locations: HashSet<_> = pairs
            .iter()
            .filter_map(|(content, _)| bibliography_tag(content)?.location())
            .collect();
        let works = match Works::generate(engine, first.span()) {
            Ok(works) => works,
            Err(errs) => {
                self.degradations.push(Degradation {
                    span: Some(first.span()),
                    construct: "bibliography-entries",
                    action: DegradationAction::Skipped,
                    message: format!(
                        "hayagriva could not render the bibliography ({}); \
                         no entries are exported",
                        diag_text(&errs)
                    ),
                });
                return;
            }
        };
        for bibliography in &bibliographies {
            let Some(location) = bibliography.location() else {
                continue;
            };
            self.pending_bibliographies.insert(location, true);
            if !tagged_locations.contains(&location) {
                self.deferred_bibliographies
                    .entry(bibliography.span())
                    .or_default()
                    .push(location);
            }
            if let Ok(rendered) = works.bibliography(location, bibliography.span()) {
                for entry in &rendered.entries {
                    let index = self.bib_map.len();
                    self.bib_map
                        .entry(entry.backlink)
                        .or_insert_with(|| format!("bib-{index}"));
                }
            }
        }
        self.works = Some(works);
    }

    /// A rendered citation's link to its bibliography entry lives in the
    /// `Works` render, not the realize pairs. `CiteGroup`s are locatable and
    /// introspectable; `Works::citation` replays the render to expose the
    /// entry backlink. Keyed by the group's span — the realized `RefElem`
    /// tag inside the group's fragment carries the same source span.
    fn resolve_cite_targets(&mut self, engine: &mut Engine) {
        let Some(works) = &self.works else { return };
        let groups = engine.introspect(QueryIntrospection(
            CiteGroup::ELEM.select(),
            Span::detached(),
        ));
        for group in &groups {
            let Some(loc) = group.location() else {
                continue;
            };
            let Ok(rendered) = works.citation(loc, group.span()) else {
                continue;
            };
            let rendered_text = tydocx_ooxml::document::strip_break_hints(&plain_text(&rendered));
            if !rendered_text.is_empty() {
                self.cite_render_map.insert(group.span(), rendered_text);
            }
            let Some(backlink) = dest_location_in(&rendered) else {
                continue;
            };
            if let Some(bookmark) = self.bib_map.get(&backlink).cloned() {
                self.cite_span_map.insert(group.span(), bookmark);
            }
        }
    }

    fn em_pt(&self) -> f64 {
        f64::from(self.defaults.size_half_pt) / 2.0
    }

    fn inline_ctx(&mut self) -> InlineCtx<'_> {
        InlineCtx {
            defaults: &self.defaults,
            normal_par_layout: self.normal_par_layout,
            quoter: SmartQuoter::new(),
            prev_char: None,
            label_map: &self.label_map,
            bib_map: &self.bib_map,
            cite_span_map: &self.cite_span_map,
            cite_render_map: &self.cite_render_map,
            fragment_tags: &mut self.fragment_tags,
            pair_index: self.current_pair,
            footnote_markers: &mut self.footnote_markers,
            degradations: &mut self.degradations,
            source: self.source.as_ref(),
            block_math_as_inline: false,
            footnote_bodies: &mut self.footnote_bodies,
            footnote_circled: &mut self.footnote_circled,
            layouter_index: &self.layouter_index,
            scoped_par: None,
            doc_date_picture: self.doc_date_picture,
        }
    }

    fn collect_blocks(
        &mut self,
        pairs: &[Pair<'_>],
        engine: &mut Engine,
        locator: &mut SplitLocator,
        scope: FlowScope,
    ) -> SourceResult<Vec<IrBlock>> {
        let track_sections = scope == FlowScope::Document;
        let mut out = Vec::new();
        let mut pending_vspace = VerticalSpace::default();
        let mut i = 0;
        while i < pairs.len() {
            let (content, styles) = pairs[i];
            if track_sections {
                self.current_pair = i;
            }
            let page_colbreak = content
                .to_packed::<ColbreakElem>()
                .filter(|_| scope != FlowScope::Nested && styles.get(PageElem::columns).get() == 1);
            if track_sections && page_colbreak.is_none() && !content.is::<PagebreakElem>() {
                let snapshot = PageSnapshot::resolve(styles);
                self.one_sided_asymmetric_margins |= !snapshot.two_sided
                    && snapshot.left_pt.to_bits() != snapshot.right_pt.to_bits();
                self.two_sided_margins |= snapshot.two_sided;
                match &self.page_snapshot {
                    None => {
                        self.page_snapshot = Some(snapshot.clone());
                        self.first_snapshot = Some(snapshot);
                    }
                    Some(prev) => {
                        if *prev != snapshot {
                            self.sections.push(IrSection {
                                start_block: out.len(),
                                settings: snapshot.to_page_settings(),
                                numbering: page::numbering_page_format(snapshot.numbering.as_ref()),
                                numbering_start: None,
                            });
                            if self.section_span.is_none() {
                                let span = content.span();
                                self.section_span = span.id().map(|_| span);
                            }
                            self.page_snapshot = Some(snapshot);
                        }
                    }
                }
            }
            if let Some(tagged) = content.to_packed::<TagElem>() {
                match &tagged.tag {
                    Tag::Start(inner, _) => {
                        let end = find_end(pairs, i + 1, inner.location());
                        let group = &pairs[i + 1..end];
                        if self.block_depth == 0 {
                            self.current_pair = i;
                        }
                        self.block_depth += 1;
                        let collected = self.collect_tag_group(
                            inner,
                            group,
                            styles,
                            engine,
                            locator,
                            scope.within_tag(),
                        );
                        self.block_depth -= 1;
                        let base = out.len();
                        out.extend(collected?);
                        let base = pending_vspace.apply(&mut out, base, styles);
                        if track_sections {
                            self.record_page_anchor(base, &out, inner);
                        }
                        i = end + 1;
                        continue;
                    }
                    Tag::End(_, _, _) => {
                        i += 1;
                        continue;
                    }
                }
            }
            if let Some(v) = content.to_packed::<VElem>() {
                pending_vspace.add(v, styles, &mut self.degradations);
                i += 1;
                continue;
            }
            let base = out.len();
            if let Some(colbreak) = page_colbreak {
                out.push(IrBlock::PageBreak {
                    weak: colbreak.weak.get(styles),
                    to: None,
                });
            } else {
                out.extend(self.collect_standalone(content, styles, engine, locator)?);
            }
            let base = pending_vspace.apply(&mut out, base, styles);
            if track_sections {
                self.record_page_anchor(base, &out, content);
            }
            i += 1;
        }
        Ok(out)
    }

    fn record_page_anchor(&mut self, base: usize, blocks: &[IrBlock], content: &Content) {
        let Some(i) = blocks[base..].iter().position(|b| match b {
            IrBlock::PageBreak { .. } => false,
            IrBlock::Paragraph(p) => p.inlines.iter().any(|inline| match inline {
                IrInline::Text(run) => !run.text.trim().is_empty(),
                _ => true,
            }),
            _ => true,
        }) else {
            return;
        };
        if let Some(span) = first_page_anchor_span(content) {
            self.page_anchors.push((base + i, span));
        }
    }

    fn collect_tag_group(
        &mut self,
        inner: &Content,
        group: &[Pair<'_>],
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
        scope: FlowScope,
    ) -> SourceResult<Vec<IrBlock>> {
        if inner.to_packed::<ContextElem>().is_some() {
            if group.iter().any(|(content, _)| {
                let Some(tagged) = content.to_packed::<TagElem>() else {
                    return false;
                };
                matches!(&tagged.tag, Tag::Start(inner, _)
                    if inner.is::<HeadingElem>()
                        && inner.location().is_some_and(|loc| self.heading_bookmarks.contains_key(&loc)))
            }) {
                return self.collect_blocks(group, engine, locator, scope);
            }
            return self.collect_context_group(inner, styles, engine, locator);
        }
        if let Some(heading) = inner.to_packed::<HeadingElem>() {
            return self.collect_heading(heading, group, styles, engine, locator, scope);
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
            return self.collect_equation(equation, inner, group, styles, engine, locator);
        }
        if inner.to_packed::<HideElem>().is_some() {
            self.degradations.push(Degradation {
                span: Some(inner.span()),
                construct: "hidden-content",
                action: DegradationAction::Skipped,
                message: "`hide()` reserves space with no content; the placeholder is dropped"
                    .to_string(),
            });
            return Ok(Vec::new());
        }
        if inner.to_packed::<BibliographyElem>().is_some() {
            return self.collect_bibliography(inner, Some(group), styles, engine, locator);
        }
        if let Some(raw) = inner.to_packed::<RawElem>() {
            return Ok(vec![self.collect_raw(raw, inner, styles, engine, locator)?]);
        }
        if let Some(quote) = inner.to_packed::<QuoteElem>() {
            return self.collect_quote(quote, styles, engine, locator);
        }
        if let Some(outline) = inner.to_packed::<OutlineElem>() {
            return Ok(vec![
                self.collect_outline(outline, group, styles, engine, locator)?,
            ]);
        }
        if let Some(place) = inner.to_packed::<PlaceElem>() {
            return Ok(vec![
                self.collect_place(place, inner, styles, engine, locator)?,
            ]);
        }
        if let Some(columns) = inner.to_packed::<ColumnsElem>() {
            return self.collect_columns_block(columns, inner, styles, engine, locator);
        }
        if let Some(image) = inner.to_packed::<ImageElem>() {
            return Ok(self.image_paragraph(image, styles, engine));
        }
        if let Some(link) = inner.to_packed::<LinkElem>() {
            return self.collect_link_group(link, group, scope, engine, locator);
        }
        if let Some(par) = inner.to_packed::<ParElem>() {
            let mut ctx = self.inline_ctx();
            let para = collect_paragraph(par, styles, engine, locator, &mut ctx)?;
            return Ok(vec![IrBlock::Paragraph(para)]);
        }
        if let Some(shape) = self.collect_shape(inner, styles, engine, locator)? {
            return Ok(vec![shape]);
        }
        self.collect_blocks(group, engine, locator, scope)
    }

    /// Content lifted out of `context { … }` reaches the pairs only as an
    /// opaque layouter block tagged with the `ContextElem` itself. Applying
    /// `CONTEXT_RULE` evaluates the closure the way realization does, exposing
    /// the elements it displayed (timble's rect-wrapped grid).
    fn collect_context_group(
        &mut self,
        content: &Content,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Vec<IrBlock>> {
        let Some(ctx) = content.to_packed::<ContextElem>() else {
            return Ok(Vec::new());
        };
        if ctx.location().is_none() {
            self.degradations.push(Degradation {
                span: Some(content.span()),
                construct: "context-content",
                action: DegradationAction::Skipped,
                message: "a `context` expression has no location and cannot be evaluated; its content is dropped"
                    .to_string(),
            });
            return Ok(Vec::new());
        }
        let displayed = typst_library::foundations::CONTEXT_RULE(ctx, engine, styles)?;
        self.collect_content_blocks(&displayed, styles, engine, locator)
    }

    fn image_paragraph(
        &mut self,
        image: &Packed<ImageElem>,
        styles: StyleChain<'_>,
        engine: &mut Engine,
    ) -> Vec<IrBlock> {
        use typst_library::layout::FixedAlignment;

        let alignment = match styles.resolve(AlignElem::alignment).x {
            FixedAlignment::Start => Alignment::Left,
            FixedAlignment::Center => Alignment::Center,
            FixedAlignment::End => Alignment::Right,
        };
        let par = IrParStyle {
            first_line_indent: Some(tydocx_ir::IrFirstLineIndent {
                twips: 0,
                chars: None,
            }),
            ..par_style_from_styles(styles)
        };
        let mut ctx = self.inline_ctx();
        match image_inline(image, styles, engine, &mut ctx) {
            Some(inline) => vec![IrBlock::Paragraph(IrParagraph {
                inlines: vec![inline],
                alignment: Some(alignment),
                par,
                suppress_indent: true,
                ..IrParagraph::default()
            })],
            None => Vec::new(),
        }
    }

    /// A `#link` whose body realizes as block-level content (a lone image
    /// lifts out of its paragraph, for example). The blocks are collected
    /// normally, then every paragraph's inline list is wrapped in
    /// `IrInline::Link` so the hyperlink — not just the content — survives.
    fn collect_link_group(
        &mut self,
        link: &Packed<LinkElem>,
        group: &[Pair<'_>],
        scope: FlowScope,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Vec<IrBlock>> {
        let mut blocks = self.collect_blocks(group, engine, locator, scope)?;
        let (url, anchor) = match &link.dest {
            LinkTarget::Dest(Destination::Url(url)) => (Some(url.to_string()), None),
            LinkTarget::Dest(Destination::Location(loc)) => {
                (None, self.label_map.get(loc).cloned())
            }
            LinkTarget::Dest(Destination::Position(_)) => (None, None),
            LinkTarget::Label(label) => (None, Some(label.resolve().to_string())),
        };
        if url.is_none() && anchor.is_none() {
            self.degradations.push(Degradation {
                span: None,
                construct: "unresolved-link",
                action: DegradationAction::Literalized,
                message: "an internal link has no labelled target; kept without the hyperlink"
                    .to_string(),
            });
            return Ok(blocks);
        }
        let mut unlinked = false;
        for block in &mut blocks {
            match block {
                IrBlock::Paragraph(para) => {
                    para.inlines = vec![IrInline::Link {
                        url: url.clone(),
                        anchor: anchor.clone(),
                        body: std::mem::take(&mut para.inlines),
                    }];
                }
                _ => unlinked = true,
            }
        }
        if unlinked {
            self.degradations.push(Degradation {
                span: Some(link.span()),
                construct: "block-link-target",
                action: DegradationAction::Literalized,
                message: "a link wraps non-paragraph content; the content is kept but it is not clickable"
                    .to_string(),
            });
        }
        Ok(blocks)
    }

    fn collect_standalone(
        &mut self,
        content: &Content,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Vec<IrBlock>> {
        if let Some(par) = content.to_packed::<ParElem>() {
            let mut ctx = self.inline_ctx();
            let para = collect_paragraph(par, styles, engine, locator, &mut ctx)?;
            return Ok(vec![IrBlock::Paragraph(para)]);
        }
        if let Some(image) = content.to_packed::<ImageElem>() {
            return Ok(self.image_paragraph(image, styles, engine));
        }
        if let Some(block) = content.to_packed::<BlockElem>() {
            return self
                .collect_block_flow(block, styles, engine, locator)
                .map(BodyBlocks::finish);
        }
        if let Some(pad) = content.to_packed::<PadElem>() {
            return self
                .collect_pad_body_blocks(pad, styles, engine, locator)
                .map(BodyBlocks::finish);
        }
        if let Some(align) = content.to_packed::<AlignElem>() {
            return self
                .collect_align_blocks(align, &ListCtx::default(), styles, engine, locator)
                .map(BodyBlocks::finish);
        }
        if let Some(columns) = content.to_packed::<ColumnsElem>() {
            return self.collect_columns_block(columns, content, styles, engine, locator);
        }
        if let Some(stack) = content.to_packed::<StackElem>() {
            return self.collect_stack_blocks(stack, styles, engine, locator);
        }
        if let Some((construct, transparent)) = transparent_container_body(content) {
            self.degradations.push(Degradation {
                span: Some(content.span()),
                construct,
                action: DegradationAction::Skipped,
                message: "the container's transform or column layout has no OOXML equivalent; \
                          the content is kept without it"
                    .to_string(),
            });
            return self.collect_content_blocks(transparent, styles, engine, locator);
        }
        if content.to_packed::<HideElem>().is_some() {
            self.degradations.push(Degradation {
                span: Some(content.span()),
                construct: "hidden-content",
                action: DegradationAction::Skipped,
                message: "`hide()` reserves space with no content; the placeholder is dropped"
                    .to_string(),
            });
            return Ok(Vec::new());
        }
        if content.to_packed::<VElem>().is_some() {
            return Ok(Vec::new());
        }
        if let Some(pagebreak) = content.to_packed::<PagebreakElem>() {
            return Ok(vec![Self::collect_pagebreak(pagebreak, styles)]);
        }
        if content.to_packed::<ColbreakElem>().is_some() {
            return Ok(vec![IrBlock::Paragraph(IrParagraph {
                inlines: vec![IrInline::ColumnBreak],
                ..IrParagraph::default()
            })]);
        }
        if let Some(place) = content.to_packed::<PlaceElem>() {
            return Ok(vec![
                self.collect_place(place, content, styles, engine, locator)?,
            ]);
        }
        if let Some(text) = content.to_packed::<TextElem>() {
            let mut para = IrParagraph {
                par: par_style_from_styles(styles),
                ..IrParagraph::default()
            };
            let run = run_from_styles(&text.text, styles, &self.defaults, Some(content.span()));
            para.inlines.push(IrInline::Text(run));
            return Ok(vec![IrBlock::Paragraph(para)]);
        }
        if let Some(shape) = self.collect_shape(content, styles, engine, locator)? {
            return Ok(vec![shape]);
        }
        Ok(Vec::new())
    }

    fn collect_layouter_block(
        &mut self,
        content: &Content,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<BodyBlocks> {
        let span = content.span();
        let mut original = self
            .stack_layouter_index
            .as_ref()
            .and_then(|index| index.get(&span))
            .or_else(|| self.layouter_index.get(&span))
            .cloned();
        if let Some(ctx) = original.as_ref()
            && ctx.to_packed::<ContextElem>().is_some()
        {
            let blocks = self.collect_context_group(ctx, styles, engine, locator)?;
            if !blocks.is_empty() {
                return Ok(blocks.into());
            }
            original = None;
        }
        let Some(original) = original else {
            self.degradations.push(Degradation {
                span: Some(content.span()),
                construct: "layouter-body",
                action: DegradationAction::Skipped,
                message:
                    "a layout callback's content could not be traced back to its element; dropped"
                        .to_string(),
            });
            return Ok(BodyBlocks::default());
        };
        if let Some(pad) = original.to_packed::<PadElem>() {
            return self.collect_pad_body_blocks(pad, styles, engine, locator);
        }
        if let Some(shape) = self.collect_shape(&original, styles, engine, locator)? {
            return Ok(vec![shape].into());
        }
        if !original.is::<BlockElem>() && is_block_element(&original) {
            return self.collect_flow_element(&original, styles, engine, locator);
        }
        // `is_block_element` claims `ColumnsElem`, so it never reaches the
        // transparent-container arm here — `collect_content_element`
        // routes it to `collect_columns_block`.
        if let Some((construct, transparent)) = transparent_container_body(&original) {
            self.degradations.push(Degradation {
                span: Some(content.span()),
                construct,
                action: DegradationAction::Skipped,
                message: "the container's transform or column layout has no OOXML equivalent; \
                          the content is kept without it"
                    .to_string(),
            });
            return self.collect_body_flow(
                transparent,
                &ListCtx::default(),
                styles,
                engine,
                locator,
            );
        }
        self.degradations.push(Degradation {
            span: Some(content.span()),
            construct: "layouter-body",
            action: DegradationAction::Skipped,
            message: "a layout callback's content could not be traced back to its element; dropped"
                .to_string(),
        });
        Ok(BodyBlocks::default())
    }

    /// `#columns(n, gutter:)[…]` — an `IrBlock::Columns` carrying the
    /// resolved column count and gutter. The gutter is a `Rel<Length>`
    /// whose ratio part is a share of the enclosing region's width; at
    /// page level that region is the text column, so the basis is the
    /// page's text width (typst-layout resolves it against
    /// `regions.base().x`). Nested placements degrade at lowering.
    fn collect_columns_block(
        &mut self,
        columns: &ColumnsElem,
        content: &Content,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Vec<IrBlock>> {
        let body = self.collect_content_blocks(&columns.body, styles, engine, locator)?;
        let span = content.span();
        Ok(vec![IrBlock::Columns {
            count: u32::try_from(columns.count.get(styles).get()).unwrap_or(u32::MAX),
            gutter_twips: page::columns_gutter_twips(
                columns.gutter.resolve(styles),
                page_content_width_pt(styles),
            ),
            body,
            span: span.id().map(|_| span),
        }])
    }

    fn collect_stack_blocks(
        &mut self,
        stack: &Packed<StackElem>,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Vec<IrBlock>> {
        let span = stack.span();
        self.degradations.push(Degradation {
            span: span.id().map(|_| span),
            construct: "stack-layout",
            action: DegradationAction::Approximated,
            message: "stack items are emitted in source order as in-flow blocks; \
                      stacking direction and inter-item spacing are not preserved"
                .to_string(),
        });
        let mut locator = stack_locator(locator, span);
        let mut out = Vec::new();
        for child in stack_child_bodies(stack, stack.dir.get(styles)) {
            let body = stack_fragment_body(child);
            engine.route.increase();
            let blocks = engine.route.check_layout_depth().at(span).and_then(|()| {
                with_fragment_at(
                    engine,
                    locator.next(&body.span()).split(),
                    &body,
                    styles,
                    |engine, locator, pairs| {
                        let mut index = HashMap::new();
                        index_content_tree(child, &mut index);
                        let parent = self.stack_layouter_index.replace(index);
                        let blocks = self.collect_blocks(pairs, engine, locator, FlowScope::Nested);
                        self.stack_layouter_index = parent;
                        blocks
                    },
                )
            });
            engine.route.decrease();
            let blocks = blocks?;
            if blocks.iter().any(stack_block_has_content) {
                out.push((child, blocks));
            }
        }
        Ok(merge_stack_line_paths(stack, styles, out))
    }

    fn collect_content_blocks(
        &mut self,
        content: &Content,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Vec<IrBlock>> {
        self.collect_body_blocks(content, &ListCtx::default(), styles, engine, locator)
    }

    /// `align(..)` is block-level but keeps its element form inside container
    /// bodies (the show rule only runs inside fragment realization). Descend
    /// into the body with the alignment folded into the chain, the same way
    /// the built-in `ALIGN_RULE` (`body.aligned(alignment)`) applies it.
    fn collect_align_blocks(
        &mut self,
        align: &AlignElem,
        list_ctx: &ListCtx,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<BodyBlocks> {
        let aligned = AlignElem::alignment.set(align.alignment.get(styles)).wrap();
        self.collect_body_flow(
            &align.body,
            list_ctx,
            styles.chain(&aligned),
            engine,
            locator,
        )
    }

    fn collect_body_blocks(
        &mut self,
        content: &Content,
        list_ctx: &ListCtx,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Vec<IrBlock>> {
        self.collect_body_flow(content, list_ctx, styles, engine, locator)
            .map(BodyBlocks::finish)
    }

    /// Collect one content node's block-level children. Root
    /// `StyledElem`/`ParElem`/`BlockElem` wrappers are descended here — not
    /// in `content_children` — so a `StyledElem`'s `styles` stay chained for
    /// everything below it (a `#text(size: …)[…]` cell keeps its size).
    fn collect_body_flow(
        &mut self,
        content: &Content,
        list_ctx: &ListCtx,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<BodyBlocks> {
        if let Some(styled) = content.to_packed::<StyledElem>() {
            return self.collect_body_flow(
                &styled.child,
                list_ctx,
                styles.chain(&styled.styles),
                engine,
                locator,
            );
        }
        if let Some(par) = content.to_packed::<ParElem>() {
            let mut ctx = self.inline_ctx();
            let para = collect_paragraph(par, styles, engine, locator, &mut ctx)?;
            return Ok(vec![IrBlock::Paragraph(para)].into());
        }
        if let Some(block) = content.to_packed::<BlockElem>() {
            return match block.body.as_option() {
                Some(Some(BlockBody::Content(body))) => {
                    self.collect_body_flow(body, list_ctx, styles, engine, locator)
                }
                _ => Ok(BodyBlocks::default()),
            };
        }
        if let Some(align) = content.to_packed::<AlignElem>() {
            return self.collect_align_blocks(align, list_ctx, styles, engine, locator);
        }
        let children = if let Some(sequence) = content.to_packed::<SequenceElem>() {
            flatten_sequences(sequence.children.iter().collect())
        } else {
            vec![content]
        };
        self.collect_body_children(&children, list_ctx, styles, engine, locator)
    }

    /// Walk one level of block children: block elements flush the pending
    /// inline run and collect directly, inline elements accumulate into the
    /// next paragraph, `v`/`pad` attach spacing to the following block.
    fn collect_body_children(
        &mut self,
        children: &[&Content],
        list_ctx: &ListCtx,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<BodyBlocks> {
        let mut out = BodyBlocks::default();
        let mut pending: Vec<Content> = Vec::new();
        let mut pending_vspace = VerticalSpace::default();
        let mut i = 0;
        while i < children.len() {
            let child = children[i];
            let base = out.blocks.len();
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
            } else if let Some((blocks, next)) =
                self.collect_bare_items(children, i, list_ctx, styles, engine, locator)?
            {
                flush_pending(&mut pending, &mut out, self, styles, engine, locator)?;
                out.extend(blocks);
                i = next;
            } else if let Some(shape) = self.collect_shape(child, styles, engine, locator)? {
                flush_pending(&mut pending, &mut out, self, styles, engine, locator)?;
                out.push(shape);
                i += 1;
            } else if let Some(v) = child.to_packed::<VElem>() {
                flush_pending(&mut pending, &mut out, self, styles, engine, locator)?;
                out.apply_vspace(&mut pending_vspace, base, styles);
                pending_vspace.add(v, styles, &mut self.degradations);
                i += 1;
                continue;
            } else if let Some(pad) = child.to_packed::<PadElem>() {
                flush_pending(&mut pending, &mut out, self, styles, engine, locator)?;
                out.extend(self.collect_pad_body_blocks(pad, styles, engine, locator)?);
                i += 1;
            } else if let Some(styled) = child.to_packed::<StyledElem>()
                && styled_holds_block(&styled.child)
            {
                flush_pending(&mut pending, &mut out, self, styles, engine, locator)?;
                out.extend(self.collect_body_flow(child, list_ctx, styles, engine, locator)?);
                i += 1;
            } else if let Some(tagged) = child.to_packed::<TagElem>()
                && let Tag::Start(inner, _) = &tagged.tag
                && inner.to_packed::<HideElem>().is_some()
            {
                flush_pending(&mut pending, &mut out, self, styles, engine, locator)?;
                self.degradations.push(Degradation {
                    span: Some(inner.span()),
                    construct: "hidden-content",
                    action: DegradationAction::Skipped,
                    message: "`hide()` reserves space with no content; the placeholder is dropped"
                        .to_string(),
                });
                i = skip_tag_children(children, i + 1, inner.location());
            } else if let Some(inner) = bibliography_tag(child) {
                flush_pending(&mut pending, &mut out, self, styles, engine, locator)?;
                out.extend(self.collect_bibliography(inner, None, styles, engine, locator)?);
                i = skip_tag_children(children, i + 1, inner.location());
            } else if is_block_element(child) {
                flush_pending(&mut pending, &mut out, self, styles, engine, locator)?;
                out.extend(self.collect_flow_element(child, styles, engine, locator)?);
                i += 1;
            } else if is_inline_element(child) {
                pending.push(child.clone());
                i += 1;
            } else if child.to_packed::<ParbreakElem>().is_some() {
                flush_pending(&mut pending, &mut out, self, styles, engine, locator)?;
                i += 1;
            } else {
                i += 1;
            }
            out.apply_vspace(&mut pending_vspace, base, styles);
        }
        let base = out.blocks.len();
        flush_pending(&mut pending, &mut out, self, styles, engine, locator)?;
        out.apply_vspace(&mut pending_vspace, base, styles);
        Ok(out)
    }

    fn collect_pad_body_blocks(
        &mut self,
        pad: &Packed<PadElem>,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<BodyBlocks> {
        let mut blocks =
            self.collect_body_flow(&pad.body, &ListCtx::default(), styles, engine, locator)?;
        apply_pad_insets(&mut blocks, pad, styles);
        Ok(blocks)
    }

    fn collect_bare_items(
        &mut self,
        children: &[&Content],
        i: usize,
        list_ctx: &ListCtx,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Option<(Vec<IrBlock>, usize)>> {
        let child = children[i];
        if child.to_packed::<ListItem>().is_some() {
            let (run, next) = take_item_run(children, i, true);
            return Ok(Some((
                vec![self.collect_bare_list(&run, list_ctx, styles, engine, locator)?],
                next,
            )));
        }
        if child.to_packed::<EnumItem>().is_some() {
            let (run, next) = take_item_run(children, i, false);
            return Ok(Some((
                vec![self.collect_bare_enum(&run, list_ctx, styles, engine, locator)?],
                next,
            )));
        }
        if child.to_packed::<TermItem>().is_some() {
            let (run, next, wide) = take_term_item_run(children, i);
            return Ok(Some((
                self.collect_bare_terms(&run, !wide, styles, engine, locator)?,
                next,
            )));
        }
        Ok(None)
    }

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
        let marker = styles.get_cloned(ListElem::marker);
        let custom_marker = marker != crate::convert::lists::default_list_marker();
        let em_pt = self.em_pt();
        let (indent_twips, body_indent_twips) = list_layout_twips(
            styles.get(ListElem::indent),
            styles.get(ListElem::body_indent),
            default_list_indents(),
            em_pt,
        );
        Ok(IrBlock::List(IrList {
            items,
            depth,
            custom_marker,
            indent_twips,
            body_indent_twips,
            spacing_twips: list_spacing_twips(styles.get(ListElem::spacing), em_pt),
            marker_texts: literal_marker_texts(&marker),
        }))
    }

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
        let reversed = styles.get(EnumElem::reversed);
        let numbering_fn = styles.get_cloned(EnumElem::numbering);
        let mut items = Vec::new();
        let mut number = u64::from(start);
        let total = u64::try_from(
            run.iter()
                .filter(|c| c.to_packed::<EnumItem>().is_some())
                .count(),
        )
        .unwrap_or(0);
        let mut index = 0u64;
        for child in run {
            let Some(item) = child.to_packed::<EnumItem>() else {
                continue;
            };
            let auto_number = if reversed {
                u64::from(start) + total.saturating_sub(index + 1)
            } else {
                number
            };
            index += 1;
            let (item_number, number_override) = match item.number.get(styles) {
                Smart::Custom(n) => (n, true),
                Smart::Auto => (auto_number, false),
            };
            number = if reversed { number } else { item_number + 1 };
            let number_text =
                enum_func_number_text(&numbering_fn, item_number, styles, engine, child.span())
                    .or_else(|| {
                        if matches!(numbering_fn, Numbering::Func(_)) {
                            self.degradations.push(Degradation {
                                span: Some(item.span()),
                                construct: "enum-numbering",
                                action: DegradationAction::Approximated,
                                message: "the enum numbering function could not be applied; \
                                  the item falls back to decimal numbering"
                                    .to_string(),
                            });
                        }
                        enum_number_text(&pattern, origin, depth, item_number)
                    });
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
            items.push(IrEnumItem {
                number_text,
                number_override,
                body,
            });
        }
        let em_pt = self.em_pt();
        let (indent_twips, body_indent_twips) = list_layout_twips(
            styles.get(EnumElem::indent),
            styles.get(EnumElem::body_indent),
            default_enum_indents(),
            em_pt,
        );
        Ok(IrBlock::Enum(IrEnum {
            items,
            numbering: pattern,
            start,
            depth,
            reversed,
            indent_twips,
            body_indent_twips,
            spacing_twips: list_spacing_twips(styles.get(EnumElem::spacing), em_pt),
        }))
    }

    fn collect_shape(
        &mut self,
        child: &Content,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Option<IrBlock>> {
        let Some((kind, shape_body)) = shape_parts(child) else {
            return Ok(None);
        };
        if self.stack_layouter_index.is_none()
            && let Some(line) = child.to_packed::<LineElem>()
            && is_default_full_width_rule(line, styles)
        {
            return Ok(Some(IrBlock::Paragraph(IrParagraph {
                horizontal_rule: true,
                ..IrParagraph::default()
            })));
        }
        let body = match shape_body {
            Some(content) => self.collect_content_blocks(&content, styles, engine, locator)?,
            None => Vec::new(),
        };
        let span = child.span();
        let resolved = resolve_shape(child, styles, engine, locator);
        Ok(Some(IrBlock::Shape {
            kind,
            span: span.id().map(|_| span),
            body,
            fill_hex: shape_fill_hex(child, styles),
            geometry: resolved.geometry,
            loss: resolved.loss,
            alignment: alignment_from_styles(styles),
        }))
    }

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
        if child.to_packed::<BibliographyElem>().is_some() {
            return self.collect_bibliography(child, None, styles, engine, locator);
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
            return Ok(vec![self.collect_outline(
                outline,
                &[],
                styles,
                engine,
                locator,
            )?]);
        }
        if let Some(place) = child.to_packed::<PlaceElem>() {
            return Ok(vec![
                self.collect_place(place, child, styles, engine, locator)?,
            ]);
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
        if let Some(shape) = self.collect_shape(child, styles, engine, locator)? {
            return Ok(vec![shape]);
        }
        self.collect_container_element(child, styles, engine, locator)
    }

    fn collect_block_flow(
        &mut self,
        block: &Packed<BlockElem>,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<BodyBlocks> {
        if let Some(body) = block_body_content(block, styles) {
            let mut blocks =
                self.collect_body_flow(body, &ListCtx::default(), styles, engine, locator)?;
            apply_block_props(&mut blocks, block, styles);
            report_block_aspects(block, styles, block.span(), &mut self.degradations);
            return Ok(blocks);
        }
        self.collect_layouter_block(block.pack_ref(), styles, engine, locator)
    }

    fn collect_flow_element(
        &mut self,
        child: &Content,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<BodyBlocks> {
        if let Some(block) = child.to_packed::<BlockElem>() {
            return self.collect_block_flow(block, styles, engine, locator);
        }
        if let Some(align) = child.to_packed::<AlignElem>() {
            return self.collect_align_blocks(align, &ListCtx::default(), styles, engine, locator);
        }
        if let Some((construct, transparent)) = transparent_container_body(child) {
            self.degradations.push(Degradation {
                span: Some(child.span()),
                construct,
                action: DegradationAction::Skipped,
                message: "the container's transform or column layout has no OOXML equivalent; \
                          the content is kept without it"
                    .to_string(),
            });
            return self.collect_body_flow(
                transparent,
                &ListCtx::default(),
                styles,
                engine,
                locator,
            );
        }
        self.collect_content_element(child, styles, engine, locator)
            .map(BodyBlocks::from)
    }

    fn collect_container_element(
        &mut self,
        child: &Content,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Vec<IrBlock>> {
        if let Some(columns) = child.to_packed::<ColumnsElem>() {
            return self.collect_columns_block(columns, child, styles, engine, locator);
        }
        if let Some(stack) = child.to_packed::<StackElem>() {
            return self.collect_stack_blocks(stack, styles, engine, locator);
        }
        if child.to_packed::<HideElem>().is_some() {
            self.degradations.push(Degradation {
                span: Some(child.span()),
                construct: "hidden-content",
                action: DegradationAction::Skipped,
                message: "`hide()` reserves space with no content; the placeholder is dropped"
                    .to_string(),
            });
            return Ok(Vec::new());
        }
        if let Some(par) = child.to_packed::<ParElem>() {
            let mut ctx = self.inline_ctx();
            let para = collect_paragraph(par, styles, engine, locator, &mut ctx)?;
            return Ok(vec![IrBlock::Paragraph(para)]);
        }
        Ok(Vec::new())
    }

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
        let scoped_par = ctx.scoped_par;
        if let Some(number) = &numbering_text {
            strip_leading_text(&mut body, number);
            strip_leading_text(&mut body, " ");
        }
        let outlined = heading.outlined.get(styles);
        if !outlined {
            self.degradations.push(Degradation {
                span: Some(child.span()),
                construct: "unoutlined-heading",
                action: DegradationAction::Approximated,
                message: "an `outlined: false` heading lowers to a plain paragraph to stay                           out of the table of contents; its Heading style is dropped"
                    .to_string(),
            });
        }
        Ok(heading_block(HeadingSpec {
            level,
            numbering: heading_numbering(styles),
            numbering_text,
            body,
            labels: self.bookmark_name(child).into_iter().collect(),
            alignment: alignment_from_styles(styles),
            par: scoped_par.unwrap_or_else(|| par_style_from_styles(styles)),
            span: child.span().id().map(|_| child.span()),
            outlined,
        }))
    }

    fn collect_equation_elem(
        &mut self,
        equation: &EquationElem,
        child: &Content,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Vec<IrBlock>> {
        if equation.block.get(styles) {
            return with_fragment(engine, locator, child, styles, |engine, locator, pairs| {
                let locations: HashSet<_> = pairs
                    .iter()
                    .filter_map(|(content, _)| {
                        let Tag::Start(inner, _) = &content.to_packed::<TagElem>()?.tag else {
                            return None;
                        };
                        if inner.is::<EquationElem>() {
                            inner.location()
                        } else {
                            None
                        }
                    })
                    .collect();
                let tags = pairs.iter().filter_map(|(content, _)| {
                    let tag = &content.to_packed::<TagElem>()?.tag;
                    let loc = match tag {
                        Tag::Start(inner, _) => inner.location()?,
                        Tag::End(loc, ..) => *loc,
                    };
                    locations
                        .contains(&loc)
                        .then(|| (tag.clone(), self.current_pair))
                });
                self.fragment_tags.extend(tags);
                self.collect_blocks(pairs, engine, locator, FlowScope::Nested)
            });
        }
        let mut ctx = self.inline_ctx();
        let inlines = collect_inlines(child, styles, engine, locator, &mut ctx)?;
        Ok(vec![IrBlock::Paragraph(IrParagraph {
            inlines,
            par: par_style_from_styles(styles),
            ..IrParagraph::default()
        })])
    }

    fn collect_heading(
        &mut self,
        heading: &Packed<HeadingElem>,
        group: &[Pair<'_>],
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
        scope: FlowScope,
    ) -> SourceResult<Vec<IrBlock>> {
        let Some((index, realized)) = native_heading_body(group, heading.span()) else {
            let mut blocks = self.collect_blocks(group, engine, locator, scope)?;
            self.finish_heading_replacement(heading, styles, &mut blocks);
            return Ok(blocks);
        };
        let mut blocks = self.collect_blocks(&group[..index], engine, locator, scope)?;
        blocks.push(self.collect_native_heading(heading, realized, styles, engine, locator)?);
        blocks.extend(self.collect_blocks(&group[index + 1..], engine, locator, scope)?);
        Ok(blocks)
    }

    fn collect_native_heading(
        &mut self,
        heading: &Packed<HeadingElem>,
        (content, paint): Pair<'_>,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<IrBlock> {
        let numbering_text = heading_numbers(heading);
        let mut ctx = self.inline_ctx();
        let mut body = collect_inlines(content, paint, engine, locator, &mut ctx)?;
        let par = ctx
            .scoped_par
            .unwrap_or_else(|| par_style_from_styles(paint));
        if let Some(number) = &numbering_text {
            strip_leading_text(&mut body, number);
            strip_leading_text(&mut body, " ");
        }
        let outlined = heading.outlined.get(styles);
        if !outlined {
            self.degradations.push(Degradation {
                span: Some(heading.span()),
                construct: "unoutlined-heading",
                action: DegradationAction::Approximated,
                message: "an `outlined: false` heading lowers to a plain paragraph to stay \
                          out of the table of contents; its Heading style is dropped"
                    .to_string(),
            });
        }
        Ok(heading_block(HeadingSpec {
            level: u8::try_from(heading.resolve_level(styles).get()).unwrap_or(u8::MAX),
            numbering: heading_numbering(styles),
            numbering_text,
            body,
            labels: self.bookmark_name(heading.pack_ref()).into_iter().collect(),
            alignment: alignment_from_styles(paint),
            par,
            span: heading.span().id().map(|_| heading.span()),
            outlined,
        }))
    }

    fn finish_heading_replacement(
        &self,
        heading: &Packed<HeadingElem>,
        styles: StyleChain<'_>,
        blocks: &mut [IrBlock],
    ) {
        if let [IrBlock::Paragraph(para)] = blocks
            && has_heading_replacement_content(para)
            && heading.outlined.get(styles)
            && heading.numbering.get_ref(styles).is_none()
        {
            let level = heading.resolve_level(styles).get().min(9);
            para.style = Some(ParagraphStyle::Heading(u8::try_from(level).unwrap_or(9)));
        }
        if let Some(name) = self.bookmark_name(heading.pack_ref())
            && !blocks.iter().any(|block| match block {
                IrBlock::Paragraph(para) => para.bookmarks.contains(&name),
                IrBlock::Heading { labels, .. } => labels.contains(&name),
                _ => false,
            })
            && let Some(target) = blocks.iter_mut().find_map(|block| match block {
                IrBlock::Paragraph(para) if has_heading_replacement_content(para) => {
                    Some(&mut para.bookmarks)
                }
                IrBlock::Heading { labels, .. } => Some(labels),
                _ => None,
            })
        {
            target.push(name);
        }
    }

    fn bookmark_name(&self, inner: &Content) -> Option<String> {
        inner
            .location()
            .and_then(|loc| self.label_map.get(&loc).cloned())
            .or_else(|| inner.label().map(|label| label.resolve().to_string()))
            .or_else(|| self.outline_bookmark_name(inner))
    }

    fn outline_bookmark_name(&self, content: &Content) -> Option<String> {
        content
            .location()
            .and_then(|loc| self.heading_bookmarks.get(&loc).cloned())
            .or_else(|| {
                self.heading_bookmark_spans
                    .get(&(content.func(), content.span()))
                    .cloned()
                    .flatten()
            })
    }

    fn absorbed_labels(&self, group: &[Pair<'_>], labels: &mut Vec<String>) {
        for (content, _) in group {
            if let Some(tagged) = content.to_packed::<TagElem>()
                && let Tag::Start(inner, _) = &tagged.tag
                && let Some(name) = self.bookmark_name(inner)
                && !labels.contains(&name)
            {
                labels.push(name);
            }
        }
    }

    fn collect_figure(
        &mut self,
        figure: &FigureElem,
        inner: &Content,
        group: &[Pair<'_>],
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<IrBlock> {
        let mut labels = Vec::new();
        labels.extend(self.bookmark_name(inner));
        self.absorbed_labels(group, &mut labels);
        let meta = FigureMeta {
            kind: figure_kind(figure, styles),
            labels,
            custom_placement: figure_custom_placement(figure),
            gap_twips: figure_gap_twips(figure, styles, self.em_pt()),
            location_hash: inner.location().map(location_hash64),
        };
        for (content, pair_styles) in group {
            let rendered = if let Some(block) = content.to_packed::<BlockElem>() {
                block_body_content(block, *pair_styles).map(|body| (body, None))
            } else if let Some(place) = content.to_packed::<PlaceElem>()
                && let Some(block) = place.body.to_packed::<BlockElem>()
            {
                block_body_content(block, *pair_styles).map(|body| (body, Some(place)))
            } else {
                None
            };
            if let Some((body_content, place)) = rendered {
                let figure_block =
                    self.collect_figure_body(&meta, body_content, *pair_styles, engine, locator)?;
                // `figure(placement:)` realizes as a floating `PlaceElem`
                // carrying the figure's span; keep that float instead of
                // absorbing it into the flow.
                return Ok(match place {
                    Some(place)
                        if content.span() == inner.span() && place.float.get(*pair_styles) =>
                    {
                        place_block(place, content, *pair_styles, vec![figure_block])
                    }
                    _ => figure_block,
                });
            }
        }
        self.collect_figure_body(&meta, &figure.body, styles, engine, locator)
    }

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
            labels: self.bookmark_name(inner).into_iter().collect(),
            custom_placement: figure_custom_placement(figure),
            gap_twips: figure_gap_twips(figure, styles, self.em_pt()),
            location_hash: inner.location().map(location_hash64),
        };
        with_fragment(engine, locator, inner, styles, |engine, locator, pairs| {
            self.collect_figure_pairs(pairs, &meta, engine, locator)
        })
    }

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

    fn collect_figure_pairs(
        &mut self,
        pairs: &[Pair<'_>],
        meta: &FigureMeta,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<IrBlock> {
        let mut caption: Vec<IrInline> = Vec::new();
        let mut caption_alignment: Option<Alignment> = None;
        let mut caption_par = IrParStyle::default();
        let mut caption_seq: Option<IrCaptionSeq> = None;
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
                            caption_par = par_style_from_styles(*s);
                            let mut ctx = self.inline_ctx();
                            caption = collect_inlines(body_c, *s, engine, locator, &mut ctx)?;
                            if let Some(seq) = caption_number_seq(
                                caption_elem,
                                seq_identifier(meta.kind),
                                *s,
                                engine,
                                &mut self.degradations,
                            ) {
                                // Realize emits `supplement + \u{a0} + numbers
                                // + separator + body`; the numeral's char
                                // offset is right after that supplement.
                                let offset = supplement
                                    .as_ref()
                                    .map_or(0, |text| text.chars().count() + 1);
                                caption_seq = Some(IrCaptionSeq { offset, seq });
                            }
                        }
                    }
                } else {
                    let blocks = self.collect_tag_group(
                        inner,
                        group,
                        pair_styles,
                        engine,
                        locator,
                        FlowScope::Nested,
                    )?;
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
            caption_par,
            caption_seq,
            kind: meta.kind,
            supplement,
            labels: meta.labels.clone(),
            caption_first,
            custom_placement: meta.custom_placement,
            gap_twips: meta.gap_twips,
            location_hash: meta.location_hash,
        })
    }

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
        let marker = list.marker.get_cloned(styles);
        let custom_marker = marker != crate::convert::lists::default_list_marker();
        let em_pt = self.em_pt();
        let (indent_twips, body_indent_twips) = list_layout_twips(
            list.indent.get(styles),
            list.body_indent.get(styles),
            default_list_indents(),
            em_pt,
        );
        Ok(IrBlock::List(IrList {
            items,
            depth,
            custom_marker,
            indent_twips,
            body_indent_twips,
            spacing_twips: list_spacing_twips(list.spacing.get(styles), em_pt),
            marker_texts: literal_marker_texts(&marker),
        }))
    }

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
                par: par_style_from_styles(styles),
                ..IrParagraph::default()
            })];
            out.extend(desc_blocks);
            Ok(out)
        } else {
            let mut out = vec![IrBlock::Paragraph(IrParagraph {
                inlines: term_inlines,
                par: par_style_from_styles(styles),
                ..IrParagraph::default()
            })];
            out.extend(desc_blocks);
            Ok(out)
        }
    }

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
        let reversed = matches!(enumer.reversed.as_option(), Some(true));
        let numbering_fn = enumer.numbering.get_cloned(styles);
        let mut items = Vec::new();
        let mut number = u64::from(start);
        let total = u64::try_from(enumer.children.len()).unwrap_or(0);
        for (index, item) in enumer.children.iter().enumerate() {
            let auto_number = if reversed {
                u64::from(start) + total.saturating_sub(u64::try_from(index).unwrap_or(0) + 1)
            } else {
                number
            };
            let (item_number, number_override) = match item.number.get(styles) {
                Smart::Custom(n) => (n, true),
                Smart::Auto => (auto_number, false),
            };
            number = if reversed { number } else { item_number + 1 };
            let number_text =
                enum_func_number_text(&numbering_fn, item_number, styles, engine, item.span())
                    .or_else(|| {
                        if matches!(numbering_fn, Numbering::Func(_)) {
                            self.degradations.push(Degradation {
                                span: Some(item.span()),
                                construct: "enum-numbering",
                                action: DegradationAction::Approximated,
                                message: "the enum numbering function could not be applied; \
                                  the item falls back to decimal numbering"
                                    .to_string(),
                            });
                        }
                        enum_number_text(&pattern, origin, depth, item_number)
                    });
            let body = self.collect_enum_item_body(
                &item.body,
                depth,
                Some(&(pattern.clone(), origin)),
                styles,
                engine,
                locator,
            )?;
            items.push(IrEnumItem {
                number_text,
                number_override,
                body,
            });
        }
        let em_pt = self.em_pt();
        let (indent_twips, body_indent_twips) = list_layout_twips(
            enumer.indent.get(styles),
            enumer.body_indent.get(styles),
            default_enum_indents(),
            em_pt,
        );
        Ok(IrBlock::Enum(IrEnum {
            items,
            numbering: pattern,
            start,
            depth,
            reversed,
            indent_twips,
            body_indent_twips,
            spacing_twips: list_spacing_twips(enumer.spacing.get(styles), em_pt),
        }))
    }

    fn collect_enum_item_body(
        &mut self,
        content: &Content,
        depth: u8,
        inherited: Option<&(IrNumberingPattern, u8)>,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Vec<IrBlock>> {
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
                cell_margins: None,
                alignment: None,
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
                row_sizes: Vec::new(),
                col_sizes: Vec::new(),
                hlines: Vec::new(),
                vlines: Vec::new(),
            }));
        };
        if let Some(cell_grid) = grid.grid.as_ref()
            && !cell_grid.entries.is_empty()
        {
            return Ok(IrBlock::Grid(self.cell_grid_to_grid(
                cell_grid, grid, content, styles, engine, locator,
            )?));
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
                            inner,
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
            row_sizes: Vec::new(),
            col_sizes: Vec::new(),
            hlines: Vec::new(),
            vlines: Vec::new(),
        }))
    }

    fn cell_grid_to_table(
        &mut self,
        grid: &CellGrid,
        table: &TableElem,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<IrTable> {
        let (mut rows, col_count) = self.cell_grid_rows(grid, false, styles, engine, locator)?;
        fold_lines_into_cells(&mut rows, grid, &mut self.degradations);
        let column_widths_pct = self.table_column_widths(table, col_count, styles);
        Ok(IrTable {
            rows,
            column_widths_pct,
            borders: table_borders_from_grid(grid),
            gutter: table_gutter(table, styles),
            cell_margins: table_cell_margins(table, styles),
            alignment: alignment_from_styles(styles),
        })
    }

    fn cell_grid_to_grid(
        &mut self,
        grid: &CellGrid,
        grid_elem: &GridElem,
        content: &Content,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<IrGrid> {
        let heights = measure_grid_row_heights(grid, content, styles, engine, locator);
        let (mut rows, col_count) = self.cell_grid_rows(grid, true, styles, engine, locator)?;
        fold_lines_into_cells(&mut rows, grid, &mut self.degradations);
        if let Some(heights) = heights {
            for (row, twips) in rows.iter_mut().zip(heights.iter()) {
                row.height_twips = *twips;
            }
        }
        let column_widths_pct = self.grid_column_widths(grid_elem, col_count, styles);
        let n_col_tracks = non_gutter_sizes(&grid.cols, grid.has_gutter).len();
        let n_row_tracks = non_gutter_sizes(&grid.rows, grid.has_gutter).len();
        Ok(IrGrid {
            rows,
            column_widths_pct,
            borders: table_borders_from_grid(grid),
            row_sizes: non_gutter_sizes(&grid.rows, grid.has_gutter),
            col_sizes: non_gutter_sizes(&grid.cols, grid.has_gutter),
            hlines: grid
                .hlines
                .iter()
                .flatten()
                .map(|line| ir_grid_line(line, n_col_tracks))
                .collect(),
            vlines: grid
                .vlines
                .iter()
                .flatten()
                .map(|line| ir_grid_line(line, n_row_tracks))
                .collect(),
        })
    }

    fn cell_grid_rows(
        &mut self,
        grid: &CellGrid,
        keep_empty: bool,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<(Vec<IrTableRow>, usize)> {
        let col_count = grid.non_gutter_column_count();
        let mut rows = Vec::new();
        for y in 0..grid.rows.len() {
            if grid.has_gutter && y % 2 == 1 {
                continue;
            }
            let is_header = grid
                .headers
                .iter()
                .any(|header| header.range.contains(&rows.len()));
            let mut cells = Vec::new();
            let mut x = 0;
            while x < grid.cols.len() {
                if grid.has_gutter && x % 2 == 1 {
                    x += 1;
                    continue;
                }
                let cell_x = if grid.has_gutter { x / 2 } else { x };
                match grid.entry(x, y) {
                    Some(Entry::Cell(cell)) => {
                        cells.push(self.resolved_cell_to_ir(
                            cell, cell_x, is_header, styles, engine, locator,
                        )?);
                    }
                    Some(Entry::Merged { parent }) => {
                        let parent_row = parent.checked_div(col_count).unwrap_or(rows.len());
                        if parent_row < rows.len() {
                            // A vertical-merge continuation must cover the same
                            // grid columns as its spanning parent: group the
                            // parent's consecutive merged columns into ONE cell
                            // carrying that colspan (gridSpan + vMerge continue),
                            // not one `colspan=1` continuation per column.
                            let mut span = 1usize;
                            let mut nx = x + 1;
                            while nx < grid.cols.len() {
                                if grid.has_gutter && nx % 2 == 1 {
                                    nx += 1;
                                    continue;
                                }
                                match grid.entry(nx, y) {
                                    Some(Entry::Merged { parent: other }) if *other == *parent => {
                                        span += 1;
                                        nx += 1;
                                    }
                                    _ => break,
                                }
                            }
                            cells.push(IrTableCell {
                                body: Vec::new(),
                                x: cell_x,
                                colspan: u32::try_from(span).unwrap_or(u32::MAX),
                                rowspan: 1,
                                vmerge: IrVMerge::Continue,
                                fill_hex: None,
                                vertical_align: None,
                                inset_twips: None,
                                borders: tydocx_ir::IrCellBorders::default(),
                                borders_declared: tydocx_ir::IrCellBorderFlags::default(),
                            });
                            x = nx;
                            continue;
                        }
                    }
                    None => {}
                }
                x += 1;
            }
            if keep_empty || !cells.is_empty() {
                rows.push(IrTableRow {
                    cells,
                    height_twips: None,
                    is_header,
                });
            }
        }
        Ok((rows, col_count))
    }

    fn resolved_cell_to_ir(
        &mut self,
        cell: &Cell,
        cell_x: usize,
        is_header: bool,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<IrTableCell> {
        let table_cell = cell.body.to_packed::<TableCell>();
        let grid_cell = cell.body.to_packed::<GridCell>();
        let (fill_hex, vertical_align, horizontal_align, inset_twips) =
            cell_style_props(cell, table_cell, grid_cell, styles);
        let raw_body = table_cell.map_or_else(
            || grid_cell.map_or_else(|| cell.body.clone(), |gc| gc.body.clone()),
            |tc| tc.body.clone(),
        );
        let mut body = if table_cell.is_some() || grid_cell.is_some() {
            with_fragment(
                engine,
                locator,
                &cell.body,
                styles,
                |engine, locator, pairs| {
                    let eff_styles = pairs.first().map_or(styles, |(_, s)| *s);
                    self.collect_content_blocks(&raw_body, eff_styles, engine, locator)
                },
            )?
        } else {
            self.collect_content_blocks(&raw_body, styles, engine, locator)?
        };
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
            x: cell_x,
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
            borders: cell_borders_from_stroke(&cell.stroke),
            borders_declared: tydocx_ir::IrCellBorderFlags {
                top: cell.stroke_overridden.top,
                right: cell.stroke_overridden.right,
                bottom: cell.stroke_overridden.bottom,
                left: cell.stroke_overridden.left,
            },
        })
    }

    fn table_column_widths(
        &self,
        table: &TableElem,
        col_count: usize,
        styles: StyleChain<'_>,
    ) -> Option<Vec<u32>> {
        let tracks = table.columns.get_ref(styles);
        let ctx = self.width_ctx(styles);
        let mut widths = track_widths_pct(&tracks.0, ctx)?;
        if widths.len() < col_count {
            let last = widths.last().copied().unwrap_or(0);
            widths.resize(col_count, last);
        }
        Some(widths)
    }

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

    fn width_ctx(&self, styles: StyleChain<'_>) -> TableWidthCtx {
        TableWidthCtx {
            content_pt: page_content_width_pt(styles),
            body_font_pt: f64::from(self.defaults.size_half_pt) / 2.0,
        }
    }

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
                            height_twips: None,
                            is_header: true,
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
            rows.push(IrTableRow {
                cells: current,
                height_twips: None,
                is_header: false,
            });
        }
        let column_widths_pct = self.table_column_widths(table, col_count, styles);
        Ok(IrTable {
            rows,
            column_widths_pct,
            borders: None,
            gutter: table_gutter(table, styles),
            cell_margins: table_cell_margins(table, styles),
            alignment: alignment_from_styles(styles),
        })
    }

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
            x: 0,
            colspan: u32::try_from(cell.colspan.get(styles).get()).unwrap_or(u32::MAX),
            rowspan: u32::try_from(cell.rowspan.get(styles).get()).unwrap_or(u32::MAX),
            vmerge: IrVMerge::None,
            fill_hex,
            vertical_align: None,
            inset_twips: None,
            borders: IrCellBorders::default(),
            borders_declared: IrCellBorderFlags::default(),
        })
    }

    fn collect_equation(
        &mut self,
        equation: &EquationElem,
        inner: &Content,
        group: &[Pair<'_>],
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Vec<IrBlock>> {
        if !equation.block.get(styles) {
            return Ok(Vec::new());
        }
        let name = self.bookmark_name(inner);
        if group.iter().any(|(pair, styles)| {
            pair.to_packed::<TagElem>().is_some_and(|tag| {
                matches!(&tag.tag, Tag::Start(replacement, _)
                    if replacement.to_packed::<EquationElem>().is_some_and(|eq| eq.block.get(*styles)))
            })
        }) {
            let mut blocks = self.collect_blocks(group, engine, locator, FlowScope::Nested)?;
            if let Some(name) = name
                && let [IrBlock::Equation(replacement)] = blocks.as_mut_slice()
            {
                self.add_equation_bookmark(replacement, name);
            }
            return Ok(blocks);
        }
        let omml = super::equation_omml_ir(inner, &mut self.degradations);
        let number = self.equation_number(equation, inner, styles, engine, locator);
        let mut block = IrEquation {
            omml,
            number,
            bookmarks: Vec::new(),
        };
        if let Some(name) = name {
            self.add_equation_bookmark(&mut block, name);
        }
        Ok(vec![IrBlock::Equation(block)])
    }

    fn add_equation_bookmark(&mut self, equation: &mut IrEquation, label: String) {
        if let Some((name, _)) = self.equations.refs.get(&label)
            && equation.bookmarks.contains(name)
        {
            return;
        }
        let short = truncate_bookmark_name(&label);
        let name = if self.equations.bookmarks.insert(short.to_owned()) {
            short.to_owned()
        } else {
            loop {
                self.equations.next_bookmark += 1;
                let candidate = format!("equation-target-{}", self.equations.next_bookmark);
                if !self
                    .label_map
                    .values()
                    .any(|name| truncate_bookmark_name(name) == candidate)
                    && self.equations.bookmarks.insert(candidate.clone())
                {
                    break candidate;
                }
            }
        };
        let value = equation
            .number
            .as_ref()
            .map(|number| number.value().to_owned());
        self.equations
            .refs
            .entry(label)
            .or_insert_with(|| (name.clone(), value));
        equation.bookmarks.push(name);
    }

    fn equation_number(
        &mut self,
        equation: &EquationElem,
        content: &Content,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> Option<IrEquationNumber> {
        let numbering = equation.numbering.get_ref(styles).as_ref()?;
        let span = content.span();
        let rendered = Counter::of(EquationElem::ELEM)
            .display_at(engine, content.location()?, styles, numbering, span)
            .ok()?;
        let display = with_fragment(engine, locator, &rendered, styles, |_, _, pairs| {
            Ok(pairs.iter().fold(String::new(), |mut text, (content, _)| {
                text.push_str(&content.plain_text());
                text
            }))
        })
        .ok()?;
        if display.is_empty() {
            return None;
        }
        let (mut prefix, mut suffix) = ("", "");
        let mut seq = None;
        if let Numbering::Pattern(pattern) = numbering
            && let Some((first_prefix, _)) = pattern.pieces.first()
        {
            if display
                .strip_prefix(first_prefix.as_str())
                .and_then(|s| s.strip_suffix(pattern.suffix.as_str()))
                .is_some()
            {
                (prefix, suffix) = (first_prefix.as_str(), pattern.suffix.as_str());
            }
            let next = self.equations.seq_count + 1;
            if pattern
                .apply(None, &[next])
                .is_ok_and(|expected| display == expected.as_str())
            {
                seq = single_piece_seq(
                    numbering,
                    &display,
                    next,
                    "equation",
                    span,
                    &mut self.degradations,
                );
                if seq.is_some() {
                    self.equations.seq_count = next;
                }
            }
        }
        Some(IrEquationNumber {
            display,
            prefix: prefix.to_owned(),
            suffix: suffix.to_owned(),
            seq,
        })
    }

    fn collect_raw(
        &mut self,
        raw: &RawElem,
        content: &Content,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<IrBlock> {
        let raw_styles = raw_text_styles(content, styles);
        let mut lines: Vec<Vec<tydocx_ooxml::document::Run>> = Vec::new();
        if let Some(synthesized) = raw.lines.as_ref() {
            lines =
                self.collect_raw_lines(synthesized, styles.chain(&raw_styles), engine, locator)?;
        }
        if lines.is_empty() {
            let frag_lines = with_fragment(
                engine,
                locator,
                content,
                styles,
                |engine, locator, pairs| {
                    for (pair_content, _) in pairs {
                        if let Some(synthesized) = find_synthesized_raw_lines(pair_content) {
                            return self.collect_raw_lines(
                                &synthesized,
                                styles.chain(&raw_styles),
                                engine,
                                locator,
                            );
                        }
                    }
                    Ok(Vec::new())
                },
            )?;
            lines = frag_lines;
        }
        if lines.is_empty() {
            let mut ctx = self.inline_ctx();
            let inlines = collect_inlines(content, styles, engine, locator, &mut ctx)?;
            let mut split: Vec<Vec<tydocx_ooxml::document::Run>> = vec![Vec::new()];
            for inline in inlines {
                match inline {
                    IrInline::Text(run) => {
                        if let Some(line) = split.last_mut() {
                            line.push(run);
                        }
                    }
                    IrInline::LineBreak { .. } => split.push(Vec::new()),
                    _ => {}
                }
            }
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

    fn collect_raw_lines(
        &mut self,
        synthesized: &[Packed<RawLine>],
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Vec<Vec<tydocx_ooxml::document::Run>>> {
        let mut lines: Vec<Vec<tydocx_ooxml::document::Run>> = Vec::new();
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

    fn collect_outline(
        &mut self,
        outline: &Packed<OutlineElem>,
        group: &[Pair<'_>],
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<IrBlock> {
        let resolved = match outline
            .location()
            .and_then(|loc| self.outline_entries.get(&loc))
        {
            Some(resolved) => resolved.clone(),
            None => self.build_outline_entries(outline, styles, engine, locator)?,
        };
        let (title, title_bookmark) =
            self.outline_title(outline, group, styles, engine, locator)?;
        Ok(IrBlock::Outline {
            title,
            title_bookmark,
            indent_twips_per_level: self.outline_indent_twips(outline, styles, resolved.numbered),
            entries: resolved.entries,
            depth: outline_depth(outline, styles),
            custom_target: outline_custom_target(outline, styles),
            figure_caption_id: resolved.figure_caption_id,
        })
    }

    /// Resolve the outline's entries from `outline.target`: the realize pairs
    /// only carry the outline's title — the `OutlineEntry` list is assembled
    /// by a layout-time show rule. Heading targets yield hyperlinked TOC
    /// entries (`target` + `page_loc` for the paged page-number patch);
    /// figure/equation targets yield caption entries; other kinds report a
    /// named degradation. `pass_one` runs this ahead of document collection
    /// so the synthesized bookmarks register before the targets themselves
    /// are collected.
    fn build_outline_entries(
        &mut self,
        outline: &OutlineElem,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<OutlineEntries> {
        let depth = outline_depth(outline, styles);
        let span = Span::detached();
        let mut entries = Vec::new();
        let mut non_heading_kinds: Vec<String> = Vec::new();
        let mut any_heading = false;
        let mut numbered = false;
        let mut caption_id: Option<&'static str> = None;
        let mut caption_kinds_mixed = false;
        let queried: Vec<Content> = engine
            .introspect(typst_library::introspection::QueryIntrospection(
                outline.target.get_ref(styles).0.clone(),
                span,
            ))
            .into_iter()
            .collect();
        for content in &queried {
            if let Some(heading) = content.to_packed::<HeadingElem>() {
                if !heading.outlined.get(styles) {
                    continue;
                }
                let level = heading.resolve_level(styles).get().min(255);
                let Ok(level) = u8::try_from(level) else {
                    continue;
                };
                if level > depth {
                    continue;
                }
                let mut entry_title = Vec::new();
                if let Some(numbers) = heading_numbers(heading) {
                    numbered = true;
                    let mut run = tydocx_ooxml::document::Run::new(format!("{numbers} "));
                    run.bold = true;
                    entry_title.push(IrInline::Text(run));
                }
                let mut ctx = self.inline_ctx();
                entry_title.extend(collect_inlines(
                    &heading.body,
                    styles,
                    engine,
                    locator,
                    &mut ctx,
                )?);
                any_heading = true;
                entries.push(IrOutlineEntry {
                    level,
                    title: entry_title,
                    target: Some(self.outline_target_bookmark(content)),
                    page_number: None,
                    page_loc: content.location().map(typst::introspection::Location::hash),
                });
            } else if let Some(figure) = content.to_packed::<FigureElem>() {
                let Some(id) = self.figure_outline_entry(
                    figure,
                    content,
                    &mut entries,
                    styles,
                    engine,
                    locator,
                )?
                else {
                    continue;
                };
                track_caption_id(&mut caption_id, &mut caption_kinds_mixed, id);
            } else if let Some(equation) = content.to_packed::<EquationElem>() {
                if !self.equation_outline_entry(equation, content, &mut entries, styles, engine) {
                    continue;
                }
                track_caption_id(&mut caption_id, &mut caption_kinds_mixed, "Equation");
            } else {
                let name = content.func().name();
                if !non_heading_kinds.iter().any(|k| k == name) {
                    non_heading_kinds.push(name.to_string());
                }
            }
        }
        let figure_caption_id = self.finish_outline_entries(
            outline,
            styles,
            &non_heading_kinds,
            any_heading,
            caption_id,
            caption_kinds_mixed,
        );
        Ok(OutlineEntries {
            entries,
            figure_caption_id,
            numbered,
        })
    }

    /// The degradations a resolved entry list still owes: unmatched target
    /// kinds, mixed caption kinds a single `\c` switch cannot name, a
    /// heading/figure mix a field update cannot regenerate, and a custom
    /// selector that widens on update. Returns the `SEQ` identifier for the
    /// `\c` switch when the outline lists exactly one caption kind.
    fn finish_outline_entries(
        &mut self,
        outline: &OutlineElem,
        styles: StyleChain<'_>,
        non_heading_kinds: &[String],
        any_heading: bool,
        caption_id: Option<&'static str>,
        caption_kinds_mixed: bool,
    ) -> Option<String> {
        for kind in non_heading_kinds {
            self.degradations.push(Degradation {
                span: None,
                construct: "outline-target-kind",
                action: DegradationAction::Skipped,
                message: format!(
                    "outline target matched `{kind}` elements; these cannot be \
                     listed in the TOC field cache, so those entries are dropped"
                ),
            });
        }
        if caption_kinds_mixed {
            self.degradations.push(Degradation {
                span: None,
                construct: "outline-target-kinds",
                action: DegradationAction::Approximated,
                message: "the outline lists more than one caption kind; the Word TOC field's \
                          \\c switch names only one, so a field update regenerates fewer entries"
                    .to_string(),
            });
        }
        let figure_caption_id = if any_heading || caption_id.is_none() {
            if any_heading && caption_id.is_some() {
                self.degradations.push(Degradation {
                    span: None,
                    construct: "outline-target",
                    action: DegradationAction::Approximated,
                    message: "the outline mixes headings with figure/equation captions; a Word \
                              field update regenerates only the heading entries"
                        .to_string(),
                });
            }
            None
        } else {
            caption_id.map(str::to_string)
        };
        if any_heading && outline_custom_target(outline, styles) && caption_id.is_none() {
            self.degradations.push(Degradation {
                span: None,
                construct: "outline-target",
                action: DegradationAction::Approximated,
                message: "the outline's selector restricts the listed entries; a Word field \
                          update regenerates all headings to the outline depth"
                    .to_string(),
            });
        }
        figure_caption_id
    }

    /// The bookmark an outline entry hyperlinks to: the element's own label,
    /// or a synthesized `outline-target-N` registered so the element emits it
    /// when collected later in document order.
    fn outline_target_bookmark(&mut self, content: &Content) -> String {
        if let Some(label) = content.label() {
            return label.resolve().to_string();
        }
        let location = content.location();
        if let Some(name) = location.and_then(|loc| self.heading_bookmarks.get(&loc)) {
            return name.to_owned();
        }
        let name = loop {
            let candidate = format!("outline-target-{}", self.next_outline_bookmark);
            self.next_outline_bookmark += 1;
            if !self.label_map.values().any(|label| label == &candidate) {
                break candidate;
            }
        };
        if let Some(loc) = location {
            self.heading_bookmarks.insert(loc, name.clone());
        }
        self.heading_bookmark_spans
            .entry((content.func(), content.span()))
            .and_modify(|span_target| *span_target = None)
            .or_insert_with(|| Some(name.clone()));
        name
    }

    /// One figure's outline entry, matching Typst's `Outlinable` for figures:
    /// outlined when `outlined` holds and the figure has a caption or a
    /// numbering; the prefix is `supplement + NBSP + numbers` and the body is
    /// the caption body. Returns the `SEQ` identifier for the TOC `\c` switch.
    fn figure_outline_entry(
        &mut self,
        figure: &Packed<FigureElem>,
        content: &Content,
        entries: &mut Vec<IrOutlineEntry>,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Option<&'static str>> {
        let caption = figure.caption.get_ref(styles);
        if !figure.outlined.get(styles)
            || (caption.is_none() && figure.numbering.get_ref(styles).is_none())
        {
            return Ok(None);
        }
        let id = seq_identifier(figure_kind(figure, styles));
        let mut number_text = String::new();
        if let Some(caption) = caption.as_ref()
            && let (Some(numbering), Some(counter), Some(loc)) = (
                caption.numbering.as_ref().and_then(Option::as_ref),
                caption.counter.as_ref().and_then(Option::as_ref),
                caption.figure_location.as_ref().and_then(Option::as_ref),
            )
            && let Ok(rendered) =
                counter.display_at(engine, *loc, styles, numbering, caption.span())
        {
            number_text = plain_text(&rendered);
        }
        if number_text.is_empty()
            && let Some(numbering) = figure.numbering.get_ref(styles).as_ref()
            && let Some(loc) = content.location()
        {
            let counter = figure
                .counter
                .clone()
                .flatten()
                .unwrap_or_else(|| Counter::of(FigureElem::ELEM));
            if let Ok(rendered) = counter.display_at(engine, loc, styles, numbering, content.span())
            {
                number_text = plain_text(&rendered);
            }
        }
        let supplement = match figure.supplement.get_cloned(styles) {
            Smart::Custom(Some(Supplement::Content(content))) => plain_text(&content),
            _ => String::new(),
        };
        let mut prefix = supplement;
        if !prefix.is_empty() {
            prefix.push('\u{a0}');
        }
        prefix.push_str(&number_text);
        let mut entry_title = Vec::new();
        if !prefix.is_empty() {
            let mut run = tydocx_ooxml::document::Run::new(format!("{prefix} "));
            run.bold = true;
            entry_title.push(IrInline::Text(run));
        }
        if let Some(caption) = caption.as_ref() {
            let mut ctx = self.inline_ctx();
            entry_title.extend(collect_inlines(
                &caption.body,
                styles,
                engine,
                locator,
                &mut ctx,
            )?);
        }
        entries.push(IrOutlineEntry {
            level: 1,
            title: entry_title,
            target: Some(self.outline_target_bookmark(content)),
            page_number: None,
            page_loc: content.location().map(typst::introspection::Location::hash),
        });
        Ok(Some(id))
    }

    /// One equation's outline entry, matching Typst's `Outlinable` for
    /// equations: outlined when block-level and numbered; the body is empty.
    fn equation_outline_entry(
        &mut self,
        equation: &Packed<EquationElem>,
        content: &Content,
        entries: &mut Vec<IrOutlineEntry>,
        styles: StyleChain<'_>,
        engine: &mut Engine,
    ) -> bool {
        let Some(numbering) = equation.numbering.get_ref(styles).as_ref() else {
            return false;
        };
        if !equation.block.get(styles) {
            return false;
        }
        let mut number_text = String::new();
        if let Some(loc) = content.location()
            && let Ok(rendered) = Counter::of(EquationElem::ELEM).display_at(
                engine,
                loc,
                styles,
                numbering,
                content.span(),
            )
        {
            number_text = plain_text(&rendered);
        }
        let supplement = match equation.supplement.get_cloned(styles) {
            Smart::Custom(Some(Supplement::Content(content))) => plain_text(&content),
            _ => String::new(),
        };
        let mut prefix = supplement;
        if !prefix.is_empty() {
            prefix.push('\u{a0}');
        }
        prefix.push_str(&number_text);
        let mut entry_title = Vec::new();
        if !prefix.is_empty() {
            let mut run = tydocx_ooxml::document::Run::new(prefix);
            run.bold = true;
            entry_title.push(IrInline::Text(run));
        }
        entries.push(IrOutlineEntry {
            level: 1,
            title: entry_title,
            target: Some(self.outline_target_bookmark(content)),
            page_number: None,
            page_loc: content.location().map(typst::introspection::Location::hash),
        });
        true
    }

    /// The per-level indent of the outline's entries: a declared `Rel` indent
    /// resolves to its absolute length; `auto` and a `Func` indent fall back
    /// to Typst's 1.2em-per-level default. Function indents cannot be resolved
    /// here, and `auto` follows each level's actual prefix width once entries
    /// are numbered — both named approximations.
    fn outline_indent_twips(
        &mut self,
        outline: &OutlineElem,
        styles: StyleChain<'_>,
        numbered: bool,
    ) -> u32 {
        let em_pt = self.em_pt();
        match outline.indent.get_ref(styles) {
            Smart::Custom(OutlineIndent::Rel(rel)) => {
                if rel.rel != typst_library::layout::Ratio::zero() {
                    self.degradations.push(Degradation {
                        span: None,
                        construct: "outline-indent",
                        action: DegradationAction::Approximated,
                        message: "the outline indent has a relative part that cannot be resolved \
                                  against a container; only its absolute part is used"
                            .to_string(),
                    });
                }
                length_twips(rel.abs, em_pt)
            }
            Smart::Custom(OutlineIndent::Func(_)) => {
                self.degradations.push(Degradation {
                    span: None,
                    construct: "outline-indent",
                    action: DegradationAction::Approximated,
                    message: "the outline indent is a function and cannot be resolved per level; \
                              entries use the 1.2em-per-level default"
                        .to_string(),
                });
                em_indent_twips(em_pt)
            }
            Smart::Auto => {
                if numbered {
                    self.degradations.push(Degradation {
                        span: None,
                        construct: "outline-indent",
                        action: DegradationAction::Approximated,
                        message: "the automatic outline indent follows each level's number width; \
                                  entries use a flat 1.2em per level"
                            .to_string(),
                    });
                }
                em_indent_twips(em_pt)
            }
        }
    }

    fn outline_title(
        &mut self,
        outline: &Packed<OutlineElem>,
        group: &[Pair<'_>],
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<(Vec<IrInline>, Option<String>)> {
        let generated = outline.realize_title(styles);
        let Some(heading) = generated
            .as_ref()
            .and_then(Content::to_packed::<HeadingElem>)
        else {
            return Ok((Vec::new(), None));
        };
        let bookmark = group.iter().find_map(|(content, _)| {
            let Tag::Start(inner, _) = &content.to_packed::<TagElem>()?.tag else {
                return None;
            };
            let candidate = inner.to_packed::<HeadingElem>()?;
            if candidate.span() != heading.span() || candidate.body != heading.body {
                return None;
            }
            self.heading_bookmarks.get(&candidate.location()?).cloned()
        });
        let mut ctx = self.inline_ctx();
        let title = collect_inlines(&heading.body, styles, engine, locator, &mut ctx)?;
        Ok((title, bookmark))
    }

    fn collect_place(
        &mut self,
        place: &PlaceElem,
        content: &Content,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<IrBlock> {
        let body = self.collect_content_blocks(&place.body, styles, engine, locator)?;
        Ok(place_block(place, content, styles, body))
    }

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

    fn collect_bibliography(
        &mut self,
        content: &Content,
        group: Option<&[Pair<'_>]>,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Vec<IrBlock>> {
        let span = content.span();
        let location = content
            .location()
            .filter(|loc| self.pending_bibliographies.contains_key(loc))
            .or_else(|| {
                self.deferred_bibliographies
                    .get(&span)?
                    .iter()
                    .copied()
                    .find(|loc| self.pending_bibliographies.get(loc) == Some(&true))
            });
        let (Some(location), Some(works)) = (location, self.works.clone()) else {
            return Ok(Vec::new());
        };
        if self.pending_bibliographies.insert(location, false) == Some(false) {
            return Ok(Vec::new());
        }
        let rendered = match works.bibliography(location, span) {
            Ok(rendered) => rendered,
            Err(errs) => {
                self.degradations.push(Degradation {
                    span: Some(span),
                    construct: "bibliography-entries",
                    action: DegradationAction::Skipped,
                    message: format!(
                        "rendered bibliography content could not be located ({}); \
                         no entries are exported",
                        diag_text(&errs)
                    ),
                });
                return Ok(Vec::new());
            }
        };
        if let Some(group) = group {
            return self.collect_bibliography_display(group, rendered, styles, engine, locator);
        }
        let located = content.clone().located(location);
        with_fragment(
            engine,
            locator,
            &located,
            styles,
            |engine, locator, pairs| {
                let group = pairs
                    .iter()
                    .position(|(content, _)| {
                        bibliography_tag(content).and_then(Content::location) == Some(location)
                    })
                    .map_or(pairs, |start| {
                        &pairs[start + 1..find_end(pairs, start + 1, Some(location))]
                    });
                self.collect_bibliography_display(group, rendered, styles, engine, locator)
            },
        )
    }

    fn collect_bibliography_display(
        &mut self,
        pairs: &[Pair<'_>],
        rendered: &RenderedBibliography,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Vec<IrBlock>> {
        let Some(index) = pairs.iter().position(|(content, _)| {
            bibliography_marker(content)
                .is_some_and(|tag| matches!(tag.kind, PdfMarkerTagKind::Bibliography(_)))
        }) else {
            let mut blocks = self.collect_blocks(pairs, engine, locator, FlowScope::Nested)?;
            if !rendered.entries.is_empty() {
                let span = pairs.first().map(|(content, _)| content.span());
                self.report_bibliography_display(span);
                blocks.push(IrBlock::Bibliography {
                    entries: self.collect_csl_entries(rendered, styles, engine, locator)?,
                });
            }
            return Ok(blocks);
        };
        let mut blocks =
            self.collect_blocks(&pairs[..index], engine, locator, FlowScope::Nested)?;
        let end = find_end(
            pairs,
            index + 1,
            bibliography_marker(pairs[index].0).and_then(Packed::location),
        );
        let entries = if rendered.entries.is_empty() {
            Vec::new()
        } else {
            let body = &pairs[index + 1..end];
            if let Some(entries) =
                self.collect_bibliography_entries(body, rendered, engine, locator)?
            {
                entries
            } else {
                self.report_bibliography_display(body.first().map(|(content, _)| content.span()));
                self.collect_csl_entries(rendered, styles, engine, locator)?
            }
        };
        if !entries.is_empty() {
            blocks.push(IrBlock::Bibliography { entries });
        }
        if let Some(tail) = pairs.get(end + 1..) {
            blocks.extend(self.collect_blocks(tail, engine, locator, FlowScope::Nested)?);
        }
        Ok(blocks)
    }

    fn collect_bibliography_entries(
        &mut self,
        pairs: &[Pair<'_>],
        rendered: &RenderedBibliography,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Option<Vec<IrBibEntry>>> {
        if !rendered.entries.iter().any(|entry| entry.prefix.is_some()) {
            return self.collect_unmarked_bibliography(pairs, rendered, engine, locator);
        }
        let Some((grid, styles)) = pairs.iter().find_map(|(content, styles)| {
            let block = content.to_packed::<BlockElem>()?;
            if let Some(BlockBody::MultiLayouter(grid)) = block.body.get_ref(*styles) {
                Some((grid, *styles))
            } else {
                None
            }
        }) else {
            return Ok(None);
        };
        let region = Region::new(
            Size::new(Abs::pt(page_content_width_pt(styles)), Abs::inf()),
            Axes::splat(false),
        );
        let Ok(fragment) = grid.call(
            engine,
            locator.next(&Span::detached()),
            styles,
            region.into(),
        ) else {
            return Ok(None);
        };
        let mut cells = BTreeMap::new();
        for frame in fragment.into_frames() {
            bibliography_grid_cells(&frame, Abs::zero(), &mut cells);
        }
        if cells.len() != rendered.entries.len() * 2 {
            return Ok(None);
        }
        let mut entries = Vec::with_capacity(rendered.entries.len());
        for (row, entry) in rendered.entries.iter().enumerate() {
            let Some(((label_x, label), (body_x, body))) =
                cells.get(&(row, 0)).zip(cells.get(&(row, 1)))
            else {
                return Ok(None);
            };
            if !bibliography_cell_marker(label).is_some_and(|tag| {
                tag.kind == PdfMarkerTagKind::ListItemLabel
                    && tag.body.location() == Some(entry.backlink)
            }) || !bibliography_cell_marker(body)
                .is_some_and(|tag| tag.kind == PdfMarkerTagKind::BibEntry)
            {
                return Ok(None);
            }
            let marker = self.collect_bibliography_cell(label, styles, engine, locator)?;
            let body_inlines = self.collect_bibliography_cell(body, styles, engine, locator)?;
            let hanging_twips = Some(page::pt_to_twips((*body_x - *label_x).abs().to_pt()));
            let entry = match (marker, body_inlines) {
                (Some(marker), Some(body_inlines)) => {
                    let Some(bookmark) = self.bib_map.get(&entry.backlink).cloned() else {
                        return Ok(None);
                    };
                    IrBibEntry {
                        bookmark,
                        marker: Some(marker),
                        body: body_inlines,
                        hanging_twips,
                        custom_display: bibliography_show_matches(label, styles)
                            || bibliography_show_matches(body, styles),
                    }
                }
                (marker, body_inlines) => {
                    self.report_bibliography_display(Some(body.span()));
                    let Some(mut fallback) =
                        self.collect_csl_entry(entry, styles, engine, locator)?
                    else {
                        return Ok(None);
                    };
                    if let Some(marker) = marker {
                        fallback.marker = Some(marker);
                        fallback.hanging_twips = hanging_twips;
                    }
                    if let Some(body) = body_inlines {
                        fallback.body = body;
                    }
                    fallback
                }
            };
            entries.push(entry);
        }
        Ok(Some(entries))
    }

    fn collect_bibliography_cell(
        &mut self,
        content: &Content,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Option<Vec<IrInline>>> {
        with_fragment(
            engine,
            locator,
            content,
            styles,
            |engine, locator, pairs| {
                if pairs.iter().any(|(part, _)| is_block_element(part)) {
                    let mut blocks =
                        self.collect_blocks(pairs, engine, locator, FlowScope::Nested)?;
                    if blocks.len() == 1
                        && let Some(IrBlock::Paragraph(para)) = blocks.pop()
                    {
                        return Ok(Some(para.inlines));
                    }
                    return Ok(None);
                }
                let mut ctx = self.inline_ctx();
                Ok(Some(collect_fragment_inlines(
                    pairs, engine, locator, &mut ctx,
                )))
            },
        )
    }

    fn collect_unmarked_bibliography(
        &mut self,
        pairs: &[Pair<'_>],
        rendered: &RenderedBibliography,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Option<Vec<IrBibEntry>>> {
        let blocks: Vec<_> = pairs
            .iter()
            .filter_map(|(content, styles)| Some((content.to_packed::<BlockElem>()?, *styles)))
            .collect();
        if blocks.len() != rendered.entries.len() {
            return Ok(None);
        }
        let mut entries = Vec::with_capacity(blocks.len());
        for ((block, styles), entry) in blocks.into_iter().zip(&rendered.entries) {
            let Some(body) = block_body_content(block, styles)
                .and_then(bibliography_body_marker)
                .filter(|body| body.location() == Some(entry.backlink))
            else {
                return Ok(None);
            };
            let Some(bookmark) = self.bib_map.get(&entry.backlink).cloned() else {
                return Ok(None);
            };
            let inset = block.inset.get(styles);
            let start = styles.resolve(TextElem::dir).start();
            let hanging_twips = rendered
                .hanging_indent
                .then(|| inset.get(start))
                .flatten()
                .map(|inset| rel_side_twips(&inset, styles));
            let mut ctx = self.inline_ctx();
            let body = collect_inlines(body, styles, engine, locator, &mut ctx)?;
            entries.push(IrBibEntry {
                bookmark,
                marker: None,
                body,
                hanging_twips,
                custom_display: false,
            });
        }
        Ok(Some(entries))
    }

    fn collect_csl_entries(
        &mut self,
        rendered: &RenderedBibliography,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Vec<IrBibEntry>> {
        let mut entries = Vec::with_capacity(rendered.entries.len());
        for entry in &rendered.entries {
            entries.extend(self.collect_csl_entry(entry, styles, engine, locator)?);
        }
        Ok(entries)
    }

    fn collect_csl_entry(
        &mut self,
        entry: &RenderedEntry,
        styles: StyleChain<'_>,
        engine: &mut Engine,
        locator: &mut SplitLocator,
    ) -> SourceResult<Option<IrBibEntry>> {
        let Some(bookmark) = self.bib_map.get(&entry.backlink).cloned() else {
            return Ok(None);
        };
        let marker = if let Some(prefix) = &entry.prefix {
            let mut ctx = self.inline_ctx();
            Some(collect_inlines(prefix, styles, engine, locator, &mut ctx)?)
        } else {
            None
        };
        let mut ctx = self.inline_ctx();
        let body = collect_inlines(&entry.body, styles, engine, locator, &mut ctx)?;
        Ok(Some(IrBibEntry {
            bookmark,
            marker,
            body,
            hanging_twips: None,
            custom_display: true,
        }))
    }

    fn report_bibliography_display(&mut self, span: Option<Span>) {
        self.degradations.push(Degradation {
            span: span.filter(|span| span.id().is_some()),
            construct: "bibliography-display",
            action: DegradationAction::Approximated,
            message: "the Paged bibliography entries could not be paired with their displayed cells; the editable CSL entries are retained".to_string(),
        });
    }
}

fn bibliography_marker(content: &Content) -> Option<&Packed<PdfMarkerTag>> {
    let tag = content.to_packed::<TagElem>()?;
    let Tag::Start(inner, _) = &tag.tag else {
        return None;
    };
    inner.to_packed()
}

fn bibliography_cell_marker(content: &Content) -> Option<&Packed<PdfMarkerTag>> {
    content.to_packed::<GridCell>()?.body.to_packed()
}

fn bibliography_body_marker(content: &Content) -> Option<&Content> {
    if let Some(marker) = content.to_packed::<PdfMarkerTag>() {
        return matches!(marker.kind, PdfMarkerTagKind::BibEntry).then_some(&marker.body);
    }
    let sequence = content.to_packed::<SequenceElem>()?;
    sequence.children.iter().find_map(bibliography_body_marker)
}

fn bibliography_show_matches(content: &Content, styles: StyleChain<'_>) -> bool {
    styles
        .recipes()
        .filter_map(typst_library::foundations::Recipe::selector)
        .any(|selector| selector.matches(content, Some(styles)))
}

fn bibliography_grid_cells(
    frame: &Frame,
    offset: Abs,
    cells: &mut BTreeMap<(usize, usize), (Abs, Content)>,
) {
    for (point, item) in frame.items() {
        match item {
            FrameItem::Group(group) => {
                bibliography_grid_cells(&group.frame, offset + point.x, cells);
            }
            FrameItem::Tag(Tag::Start(content, _)) => {
                if let Some(cell) = content.to_packed::<GridCell>()
                    && bibliography_cell_marker(content).is_some()
                    && let (Some(Smart::Custom(x)), Some(Smart::Custom(y))) =
                        (*cell.x.as_option(), *cell.y.as_option())
                {
                    cells
                        .entry((y, x))
                        .or_insert((offset + point.x, content.clone()));
                }
            }
            _ => {}
        }
    }
}

fn flush_pending(
    pending: &mut Vec<Content>,
    out: &mut BodyBlocks,
    collector: &mut Collector<'_>,
    styles: StyleChain<'_>,
    engine: &mut Engine,
    locator: &mut SplitLocator,
) -> SourceResult<()> {
    if pending.is_empty() {
        return Ok(());
    }
    let sequence = Content::sequence(std::mem::take(pending));
    with_fragment(
        engine,
        locator,
        &sequence,
        styles,
        |engine, locator, pairs| {
            if pairs.iter().any(|(content, _)| content.is::<ParElem>()) {
                out.extend(collector.collect_blocks(pairs, engine, locator, FlowScope::Nested)?);
            } else {
                let chain = StyleChain::trunk_from_pairs(pairs).unwrap_or(styles);
                let mut ctx = collector.inline_ctx();
                let para = collect_inline_paragraph(pairs, chain, None, engine, locator, &mut ctx);
                if !para.inlines.is_empty() {
                    out.push(IrBlock::Paragraph(para));
                }
            }
            Ok(())
        },
    )
}

/// `#set table(inset:)` becomes `w:tblCellMar`. `Auto` (never declared) keeps
/// the Word default instead of baking in Typst's 5pt.
fn table_cell_margins(table: &TableElem, styles: StyleChain<'_>) -> Option<TableCellMargins> {
    let Celled::Value(sides) = table.inset.get_cloned(styles) else {
        return None;
    };
    let side = |side: Option<Rel<Length>>| side.map_or(0, |rel| rel_side_twips(&rel, styles));
    Some(TableCellMargins {
        top: side(sides.top),
        left: side(sides.left),
        bottom: side(sides.bottom),
        right: side(sides.right),
    })
}

/// Absolute twips for one `Rel<Length>` side (`em` resolved against the current
/// font size; the parent-relative ratio part is not representable and ignored).
fn rel_side_twips(rel: &Rel<Length>, styles: StyleChain<'_>) -> u32 {
    crate::convert::page::pt_to_twips(rel.abs.at(styles.resolve(TextElem::size)).to_pt())
}

fn apply_pad_insets(body: &mut BodyBlocks, pad: &PadElem, styles: StyleChain<'_>) {
    let blocks = &mut body.blocks;
    let left = rel_side_twips(&pad.left.get_cloned(styles), styles);
    let right = rel_side_twips(&pad.right.get_cloned(styles), styles);
    let top = rel_side_twips(&pad.top.get_cloned(styles), styles);
    let bottom = rel_side_twips(&pad.bottom.get_cloned(styles), styles);
    apply_side_indents(blocks, left, right);
    if top > 0
        && let Some(IrBlock::Paragraph(para)) = blocks.first_mut()
    {
        para.spacing_before = Some(para.spacing_before.unwrap_or(0).saturating_add(top));
    }
    if bottom > 0
        && let Some((IrBlock::Paragraph(para), rest)) = blocks.split_last_mut()
    {
        if para.spacing_after.is_none() {
            let em_pt = styles.resolve(TextElem::size).to_pt();
            let gap = length_twips(styles.get(ParElem::spacing), em_pt)
                .saturating_sub(length_twips(styles.get(ParElem::leading), em_pt));
            body.inherited_after = Some((rest.len(), gap));
        }
        para.spacing_after = Some(
            para.spacing_after
                .unwrap_or_default()
                .saturating_add(bottom),
        );
    }
}

/// `#block(inset:)` narrows the text column on both sides; `breakable: false`
/// keeps the block's paragraphs on one page; `sticky: true` attaches the block
/// to what follows; `above`/`below` (and the `spacing` shorthand folded into
/// them at parse time) set the block's outer spacing.
fn apply_block_props(body: &mut BodyBlocks, block: &BlockElem, styles: StyleChain<'_>) {
    let blocks = &mut body.blocks;
    let inset = block.inset.get_cloned(styles);
    let left = inset.left.map_or(0, |rel| rel_side_twips(&rel, styles));
    let right = inset.right.map_or(0, |rel| rel_side_twips(&rel, styles));
    if left != 0 || right != 0 {
        apply_side_indents(blocks, left, right);
    }
    let em_pt = styles.resolve(TextElem::size).to_pt();
    let above = spacing_rel_twips(block.above.get(styles), em_pt);
    let below = spacing_rel_twips(block.below.get(styles), em_pt);
    let sticky = block.sticky.get(styles);
    let breakable = block.breakable.get(styles);
    if above.is_none() && below.is_none() && !sticky && breakable {
        return;
    }
    let mut paragraph_indices = Vec::new();
    for (i, inner) in blocks.iter().enumerate() {
        if matches!(inner, IrBlock::Paragraph(_)) {
            paragraph_indices.push(i);
        }
    }
    let Some(&first) = paragraph_indices.first() else {
        return;
    };
    let last = *paragraph_indices.last().unwrap_or(&first);
    if let Some(above) = above
        && let IrBlock::Paragraph(para) = &mut blocks[first]
    {
        para.spacing_before = Some(
            para.spacing_before
                .map_or(above, |current| current.saturating_add(above)),
        );
    }
    if let Some(below) = below
        && let IrBlock::Paragraph(para) = &mut blocks[last]
    {
        body.inherited_after = None;
        para.spacing_after = Some(
            para.spacing_after
                .map_or(below, |current| current.saturating_add(below)),
        );
    }
    if !breakable {
        for (idx, inner) in blocks.iter_mut().enumerate() {
            if let IrBlock::Paragraph(para) = inner {
                para.keep_lines = true;
                para.keep_next = idx != last;
            }
        }
    }
    if sticky && let IrBlock::Paragraph(para) = &mut blocks[last] {
        para.keep_next = true;
    }
}

fn place_block(
    place: &PlaceElem,
    content: &Content,
    styles: StyleChain<'_>,
    body: Vec<IrBlock>,
) -> IrBlock {
    let wrap = match place.alignment.get(styles) {
        Smart::Custom(align) if align.y() == Some(VAlignment::Horizon) => IrPlaceWrap::Square,
        _ => IrPlaceWrap::TopAndBottom,
    };
    IrBlock::Place {
        float: place.float.get(styles),
        wrap,
        clearance_emu: tydocx_ir::pt_to_emu(place.clearance.resolve(styles).to_pt()),
        location_hash: content.location().map(location_hash64),
        anchor: None,
        body,
    }
}

/// The block properties the paragraph C1 form drops: a `width`/`height`
/// constraint (`block(width: 1fr)` right-push, fixed/percentage widths) and
/// the fill/stroke/radius/clip decoration.
fn report_block_aspects(
    block: &Packed<BlockElem>,
    styles: StyleChain<'_>,
    span: Span,
    degradations: &mut Vec<Degradation>,
) {
    if block.width.get(styles).is_custom() || !block.height.get(styles).is_auto() {
        degradations.push(Degradation {
            span: Some(span),
            construct: "block-width",
            action: DegradationAction::Skipped,
            message: "a block width/height constraint is dropped; the content flows \
                      full-width"
                .to_string(),
        });
    }
    let stroked = block
        .stroke
        .get_cloned(styles)
        .iter()
        .any(|side| side.as_ref().is_some_and(Option::is_some));
    let rounded = block.radius.get_cloned(styles).iter().any(Option::is_some);
    if block.fill.get_cloned(styles).is_some() || stroked || rounded || block.clip.get(styles) {
        degradations.push(Degradation {
            span: Some(span),
            construct: "block-decoration",
            action: DegradationAction::Skipped,
            message: "a block fill/stroke/radius/clip decoration is dropped".to_string(),
        });
    }
}

fn apply_side_indents(blocks: &mut [IrBlock], left: u32, right: u32) {
    for block in blocks {
        if let IrBlock::Paragraph(para) = block {
            if left > 0 {
                para.left_indent = Some(para.left_indent.unwrap_or(0) + left);
            }
            if right > 0 {
                para.right_indent = Some(para.right_indent.unwrap_or(0) + right);
            }
        }
    }
}

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
    for child in shallow_children(content) {
        if std::ptr::eq(child, content) {
            continue;
        }
        collect_raw_lines_in(child, out);
    }
}

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

fn shallow_children(content: &Content) -> Vec<&Content> {
    if let Some(sequence) = content.to_packed::<SequenceElem>() {
        return sequence.children.iter().collect();
    }
    if let Some(styled) = content.to_packed::<StyledElem>() {
        return vec![&styled.child];
    }
    if let Some(par) = content.to_packed::<ParElem>() {
        return shallow_children(&par.body);
    }
    if let Some(block) = content.to_packed::<BlockElem>() {
        if let Some(Some(BlockBody::Content(body))) = block.body.as_option() {
            return shallow_children(body);
        }
        return Vec::new();
    }
    vec![content]
}

fn shallow_plain_text(content: &Content) -> String {
    let mut out = String::new();
    shallow_plain_text_into(content, &mut out);
    out
}

fn shallow_plain_text_into(content: &Content, out: &mut String) {
    if let Some(text) = content.to_packed::<TextElem>() {
        out.push_str(&text.text);
        return;
    }
    for child in shallow_children(content) {
        if !std::ptr::eq(child, content) {
            shallow_plain_text_into(child, out);
        }
    }
}

pub(super) fn content_children(content: &Content) -> Vec<&Content> {
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
        if let Some(Some(BlockBody::Content(body))) = block.body.as_option() {
            return content_children(body);
        }
        return Vec::new();
    }
    if let Some(tagged) = content.to_packed::<TagElem>() {
        return match &tagged.tag {
            Tag::Start(inner, _) => vec![inner],
            Tag::End(..) => Vec::new(),
        };
    }
    container_children(content).unwrap_or_else(|| vec![content])
}

fn container_children(content: &Content) -> Option<Vec<&Content>> {
    if let Some(list) = content.to_packed::<ListElem>() {
        return Some(list.children.iter().map(Packed::pack_ref).collect());
    }
    if let Some(enumer) = content.to_packed::<EnumElem>() {
        return Some(enumer.children.iter().map(Packed::pack_ref).collect());
    }
    if let Some(terms) = content.to_packed::<TermsElem>() {
        return Some(terms.children.iter().map(Packed::pack_ref).collect());
    }
    if let Some(item) = content.to_packed::<TermItem>() {
        return Some(vec![&item.term, &item.description]);
    }
    if let Some(item) = content.to_packed::<ListItem>() {
        return Some(vec![&item.body]);
    }
    if let Some(item) = content.to_packed::<EnumItem>() {
        return Some(vec![&item.body]);
    }
    if let Some(cell) = content.to_packed::<TableCell>() {
        return Some(vec![&cell.body]);
    }
    if let Some(cell) = content.to_packed::<GridCell>() {
        return Some(vec![&cell.body]);
    }
    if let Some(table) = content.to_packed::<TableElem>() {
        return Some(
            table
                .children
                .iter()
                .filter_map(table_child_content)
                .collect(),
        );
    }
    if let Some(header) = content.to_packed::<TableHeader>() {
        return Some(table_items(&header.children));
    }
    if let Some(footer) = content.to_packed::<TableFooter>() {
        return Some(table_items(&footer.children));
    }
    if let Some(grid) = content.to_packed::<GridElem>() {
        return Some(
            grid.children
                .iter()
                .filter_map(grid_child_content)
                .collect(),
        );
    }
    if let Some(header) = content.to_packed::<GridHeader>() {
        return Some(grid_items(&header.children));
    }
    if let Some(footer) = content.to_packed::<GridFooter>() {
        return Some(grid_items(&footer.children));
    }
    if let Some(figure) = content.to_packed::<FigureElem>() {
        let mut children = vec![&figure.body];
        children.extend(
            figure
                .caption
                .as_option()
                .as_ref()
                .and_then(Option::as_ref)
                .map(Packed::pack_ref),
        );
        return Some(children);
    }
    if let Some(footnote) = content.to_packed::<FootnoteElem>() {
        return Some(match &footnote.body {
            FootnoteBody::Content(body) => vec![body],
            FootnoteBody::Reference(_) => Vec::new(),
        });
    }
    if let Some(stack) = content.to_packed::<StackElem>() {
        return Some(
            stack
                .children
                .iter()
                .filter_map(|child| match child {
                    StackChild::Block(body) => Some(body),
                    StackChild::Spacing(_) => None,
                })
                .collect(),
        );
    }
    if let Some(body) = container_body(content) {
        return Some(vec![body]);
    }
    if let Some(body) = optional_body(content) {
        return Some(vec![body]);
    }
    None
}

fn table_child_content(child: &TableChild) -> Option<&Content> {
    match child {
        TableChild::Header(header) => Some(header.pack_ref()),
        TableChild::Footer(footer) => Some(footer.pack_ref()),
        TableChild::Item(item) => table_item_content(item),
    }
}

fn table_items(items: &[TableItem]) -> Vec<&Content> {
    items.iter().filter_map(table_item_content).collect()
}

fn grid_items(items: &[GridItem]) -> Vec<&Content> {
    items.iter().filter_map(grid_item_content).collect()
}

fn table_item_content(item: &TableItem) -> Option<&Content> {
    match item {
        TableItem::Cell(cell) => Some(cell.pack_ref()),
        TableItem::HLine(_) | TableItem::VLine(_) => None,
    }
}

fn grid_child_content(child: &GridChild) -> Option<&Content> {
    match child {
        GridChild::Header(header) => Some(header.pack_ref()),
        GridChild::Footer(footer) => Some(footer.pack_ref()),
        GridChild::Item(item) => grid_item_content(item),
    }
}

fn grid_item_content(item: &GridItem) -> Option<&Content> {
    match item {
        GridItem::Cell(cell) => Some(cell.pack_ref()),
        GridItem::HLine(_) | GridItem::VLine(_) => None,
    }
}

fn container_body(content: &Content) -> Option<&Content> {
    macro_rules! body_of {
        ($($ty:ty),+ $(,)?) => {
            $(if let Some(elem) = content.to_packed::<$ty>() {
                return Some(&elem.body);
            })+
        };
    }
    body_of!(
        DirectLinkElem,
        LinkElem,
        HeadingElem,
        QuoteElem,
        AlignElem,
        PadElem,
        PlaceElem,
        ColumnsElem,
        MoveElem,
        RotateElem,
        ScaleElem,
        SkewElem,
        RepeatElem,
        StrongElem,
        EmphElem,
        UnderlineElem,
        OverlineElem,
        StrikeElem,
        HighlightElem,
        SmallcapsElem,
        SuperElem,
        SubElem,
    );
    None
}

fn optional_body(content: &Content) -> Option<&Content> {
    macro_rules! body_of {
        ($($ty:ty),+ $(,)?) => {
            $(if let Some(elem) = content.to_packed::<$ty>()
                && let Some(body) = elem.body.as_option().as_ref().and_then(Option::as_ref)
            {
                return Some(body);
            })+
        };
    }
    body_of!(BoxElem, RectElem, SquareElem, EllipseElem, CircleElem);
    None
}

fn index_content_tree(content: &Content, out: &mut HashMap<Span, Content>) {
    let span = content.span();
    if span.id().is_some() {
        out.entry(span).or_insert_with(|| content.clone());
    }
    for (_, value) in content.fields() {
        index_value(&value, out);
    }
}

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
    if content.to_packed::<LineElem>().is_some() {
        return Some((IrShapeKind::Line, None));
    }
    None
}

pub(super) fn shape_fill_hex(content: &Content, styles: StyleChain<'_>) -> Option<String> {
    let fill = if let Some(rect) = content.to_packed::<RectElem>() {
        rect.fill.get_cloned(styles)
    } else if let Some(square) = content.to_packed::<SquareElem>() {
        square.fill.get_cloned(styles)
    } else if let Some(ellipse) = content.to_packed::<EllipseElem>() {
        ellipse.fill.get_cloned(styles)
    } else if let Some(circle) = content.to_packed::<CircleElem>() {
        circle.fill.get_cloned(styles)
    } else if let Some(polygon) = content.to_packed::<PolygonElem>() {
        polygon.fill.get_cloned(styles)
    } else if let Some(curve) = content.to_packed::<CurveElem>() {
        curve.fill.get_cloned(styles)
    } else {
        None
    };
    fill.as_ref().and_then(paint_to_shd_hex)
}

fn styled_holds_block(content: &Content) -> bool {
    if let Some(styled) = content.to_packed::<StyledElem>() {
        return styled_holds_block(&styled.child);
    }
    if let Some(sequence) = content.to_packed::<SequenceElem>() {
        return sequence.children.iter().any(styled_holds_block);
    }
    is_block_element(content)
}

fn stack_block_has_content(block: &IrBlock) -> bool {
    let IrBlock::Paragraph(para) = block else {
        return true;
    };
    para.horizontal_rule
        || !para.bookmarks.is_empty()
        || para.inlines.iter().any(|inline| match inline {
            IrInline::Space => false,
            IrInline::Text(run) => !run.text.trim().is_empty(),
            _ => true,
        })
}

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
        || content
            .to_packed::<EquationElem>()
            .is_some_and(|eq| matches!(eq.block.as_option(), Some(true)))
        || content.to_packed::<RawElem>().is_some()
        || content.to_packed::<QuoteElem>().is_some()
        || content.to_packed::<OutlineElem>().is_some()
        || content.to_packed::<BibliographyElem>().is_some()
        || content.to_packed::<PlaceElem>().is_some()
        || content.to_packed::<PagebreakElem>().is_some()
        || content.to_packed::<ColbreakElem>().is_some()
        || content.to_packed::<BlockElem>().is_some()
        || content.to_packed::<AlignElem>().is_some()
        || content.to_packed::<ParElem>().is_some()
        || content.to_packed::<ColumnsElem>().is_some()
        || content.to_packed::<StackElem>().is_some()
        || shape_parts(content).is_some()
        || content.to_packed::<LineElem>().is_some()
}

fn is_inline_element(content: &Content) -> bool {
    !is_block_element(content)
        && content.to_packed::<TagElem>().is_none()
        && content
            .to_packed::<typst_library::layout::VElem>()
            .is_none()
        && content.to_packed::<ParbreakElem>().is_none()
}

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
            i += 1;
        } else {
            break;
        }
    }
    (run, i)
}

pub(super) fn block_body_content<'a>(
    block: &'a BlockElem,
    styles: StyleChain<'a>,
) -> Option<&'a Content> {
    match block.body.get_ref(styles) {
        Some(BlockBody::Content(content)) => Some(content),
        _ => None,
    }
}

fn native_heading_body<'a>(pair_group: &[Pair<'a>], span: Span) -> Option<(usize, Pair<'a>)> {
    let mut i = 0;
    while let Some(&(content, styles)) = pair_group.get(i) {
        if let Some(tag) = content.to_packed::<TagElem>()
            && let Tag::Start(inner, _) = &tag.tag
        {
            i = find_end(pair_group, i + 1, inner.location());
        } else if content.span() == span
            && let Some(block) = content.to_packed::<BlockElem>()
            && let Some(body) = block_body_content(block, styles)
        {
            return Some((i, (body, styles)));
        }
        i += 1;
    }
    None
}

fn has_heading_replacement_content(para: &IrParagraph) -> bool {
    para.inlines.iter().any(|inline| match inline {
        IrInline::Text(run) => !run.text.trim().is_empty(),
        IrInline::ColumnBreak | IrInline::PageBreak | IrInline::Space => false,
        _ => true,
    })
}

/// An unlocated `Start` yields an empty group: swallowing the rest of
/// the document for one unlocated tag would be worse than dropping it.
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

fn bibliography_tag(content: &Content) -> Option<&Content> {
    let Tag::Start(inner, _) = &content.to_packed::<TagElem>()?.tag else {
        return None;
    };
    if inner.to_packed::<BibliographyElem>().is_some() {
        return Some(inner);
    }
    None
}

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

fn table_gutter(table: &TableElem, styles: StyleChain<'_>) -> bool {
    !table.column_gutter.get_ref(styles).0.is_empty()
        || !table.row_gutter.get_ref(styles).0.is_empty()
}

fn figure_custom_placement(figure: &FigureElem) -> bool {
    matches!(figure.placement.as_option(), Some(Some(Smart::Custom(_))))
}

fn outline_custom_target(outline: &OutlineElem, styles: StyleChain<'_>) -> bool {
    outline.target.get_ref(styles) != &LocatableSelector(HeadingElem::ELEM.select())
}

/// Accumulate the `SEQ` identifier of a caption entry — `mixed` flips once
/// two different identifiers appear.
fn track_caption_id(caption_id: &mut Option<&'static str>, mixed: &mut bool, id: &'static str) {
    match caption_id {
        None => *caption_id = Some(id),
        Some(known) if *known != id => *mixed = true,
        Some(_) => {}
    }
}

/// The `1.2em`-per-level indent `outline(indent: auto)` falls back to.
fn em_indent_twips(em_pt: f64) -> u32 {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "1.2em is small and positive"
    )]
    {
        (1.2 * em_pt * 20.0).round() as u32
    }
}

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

fn heading_numbers(heading: &HeadingElem) -> Option<String> {
    match heading.numbers.as_ref() {
        Some(numbers) if !numbers.is_empty() => Some(numbers.to_string()),
        _ => None,
    }
}

/// The heading's declared `set heading(numbering:)`: `Mappable` when the
/// pattern's numeral systems all have an OOXML `w:numFmt`, `Unmappable` for a
/// function numbering or an unrepresentable system. `None` = no numbering.
fn heading_numbering(styles: StyleChain<'_>) -> Option<IrHeadingNumbering> {
    let numbering = styles.get_cloned(HeadingElem::numbering)?;
    Some(match &numbering {
        Numbering::Func(_) => IrHeadingNumbering::Unmappable,
        Numbering::Pattern(_) => pattern_from_numbering(&numbering, true)
            .map_or(IrHeadingNumbering::Unmappable, IrHeadingNumbering::Mappable),
    })
}

fn length_twips(length: Length, em_pt: f64) -> u32 {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "declared lengths are small and clamped at zero"
    )]
    {
        ((length.abs.to_pt() + length.em.get() * em_pt) * 20.0)
            .round()
            .max(0.0) as u32
    }
}

fn spacing_rel_twips(spacing: Smart<Spacing>, em_pt: f64) -> Option<u32> {
    match spacing {
        Smart::Custom(Spacing::Rel(rel)) if rel.rel == typst_library::layout::Ratio::zero() => {
            Some(length_twips(rel.abs, em_pt))
        }
        _ => None,
    }
}

/// Container elements whose own chrome is dropped but whose body still
/// converts (`#move`/`#rotate`/`#scale`/`#skew`). The first tuple
/// element is the degradation construct to report.
pub(super) fn transparent_container_body(content: &Content) -> Option<(&'static str, &Content)> {
    if let Some(elem) = content.to_packed::<MoveElem>() {
        return Some(("moved-content", &elem.body));
    }
    if let Some(elem) = content.to_packed::<RotateElem>() {
        return Some(("rotated-content", &elem.body));
    }
    if let Some(elem) = content.to_packed::<ScaleElem>() {
        return Some(("scaled-content", &elem.body));
    }
    if let Some(elem) = content.to_packed::<SkewElem>() {
        return Some(("skewed-content", &elem.body));
    }
    None
}

/// Skip past a `Tag::End` matching `location` in a children list (used to drop
/// `#hide[..]` bodies inside block content).
fn skip_tag_children(children: &[&Content], start: usize, location: Option<Location>) -> usize {
    let Some(location) = location else {
        return start;
    };
    let mut i = start;
    while i < children.len() {
        if let Some(tagged) = children[i].to_packed::<TagElem>()
            && let Tag::End(loc, _, _) = tagged.tag
            && loc == location
        {
            return i + 1;
        }
        i += 1;
    }
    i
}

fn default_list_indents() -> (Length, Length) {
    let defaults = StyleChain::default();
    (
        defaults.get(ListElem::indent),
        defaults.get(ListElem::body_indent),
    )
}

fn default_enum_indents() -> (Length, Length) {
    let defaults = StyleChain::default();
    (
        defaults.get(EnumElem::indent),
        defaults.get(EnumElem::body_indent),
    )
}

/// `list(indent:|body-indent:)` → resolved twips pair when either differs
/// from the element default. The pair travels together so the lowerer can
/// compute `w:left`/`w:hanging` without re-resolving.
fn list_layout_twips(
    indent: Length,
    body_indent: Length,
    defaults: (Length, Length),
    em_pt: f64,
) -> (Option<u32>, Option<u32>) {
    if indent == defaults.0 && body_indent == defaults.1 {
        return (None, None);
    }
    (
        Some(length_twips(indent, em_pt)),
        Some(length_twips(body_indent, em_pt)),
    )
}

fn list_spacing_twips(spacing: Smart<Length>, em_pt: f64) -> Option<u32> {
    match spacing {
        Smart::Custom(length) => Some(length_twips(length, em_pt)),
        Smart::Auto => None,
    }
}

/// `list(marker: ([a], [b], …))` literal markers for the numbering override;
/// `None` when the marker is the default or a function.
fn literal_marker_texts(marker: &ListMarker) -> Option<Vec<String>> {
    let ListMarker::Content(markers) = marker else {
        return None;
    };
    if *marker == crate::convert::lists::default_list_marker() {
        return None;
    }
    let texts: Vec<String> = markers.iter().map(plain_text).collect();
    if texts.iter().any(String::is_empty) {
        return None;
    }
    Some(texts)
}

fn figure_gap_twips(figure: &FigureElem, styles: StyleChain<'_>, em_pt: f64) -> Option<u32> {
    let gap = figure.gap.get(styles);
    (gap != StyleChain::default().get(FigureElem::gap)).then(|| length_twips(gap, em_pt))
}

struct HeadingSpec {
    level: u8,
    numbering: Option<IrHeadingNumbering>,
    numbering_text: Option<String>,
    body: Vec<IrInline>,
    labels: Vec<String>,
    alignment: Option<Alignment>,
    par: IrParStyle,
    span: Option<Span>,
    outlined: bool,
}

/// `outlined: false` headings leave the document outline, so they lower to a
/// plain paragraph (numbering prefix and label kept) instead of a Heading
/// style that would re-enter the TOC.
fn heading_block(spec: HeadingSpec) -> IrBlock {
    if !spec.outlined {
        let mut para = IrParagraph {
            alignment: spec.alignment,
            par: spec.par,
            bookmarks: spec.labels,
            ..IrParagraph::default()
        };
        if let Some(number) = spec.numbering_text {
            let mut prefix = spec
                .body
                .iter()
                .find_map(|inline| match inline {
                    IrInline::Text(run) => Some(run.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| Run::new(""));
            prefix.text = format!("{number} ");
            para.inlines.push(IrInline::Text(prefix));
        }
        para.inlines.extend(spec.body);
        return IrBlock::Paragraph(para);
    }
    IrBlock::Heading {
        level: spec.level,
        numbering_text: spec.numbering_text,
        numbering: spec.numbering,
        body: spec.body,
        labels: spec.labels,
        par: spec.par,
        alignment: spec.alignment,
        span: spec.span,
    }
}

/// `outline(depth:)` — `auto`/`none` means every level, pinned as `u8::MAX`
/// rather than a silent level-3 cap.
fn outline_depth(outline: &OutlineElem, styles: StyleChain<'_>) -> u8 {
    outline.depth.get(styles).map_or(u8::MAX, |depth| {
        u8::try_from(depth.get()).unwrap_or(u8::MAX)
    })
}

fn bold_inlines(inlines: &mut [IrInline]) {
    for inline in inlines {
        match inline {
            IrInline::Text(run) => run.bold = true,
            IrInline::Link { body, .. } | IrInline::Field { body, .. } => bold_inlines(body),
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
            let boundary = line_boundary_index(line);
            if boundary == 0 {
                top = thickness;
            } else if boundary == rows {
                bottom = thickness;
            } else if boundary == header_rows && header_rows > 0 {
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
            let boundary = line_boundary_index(line);
            if boundary == 0 {
                left = thickness;
            } else if boundary == cols {
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

fn line_spans_full(line: &Line, tracks: usize) -> bool {
    line.start == 0 && line.end.is_none_or(|end| end.get() >= tracks)
}

/// Two lines placed `after` track `k` and `before` track `k + 1` draw at the
/// two ends of the gutter between those tracks — distinct rules in Typst
/// that collapse onto one DOCX edge. Report the collapse when they carry
/// different strokes; same-stroke pairs paint identically anyway.
fn report_collapsed_line_boundaries<'a>(
    lines: impl Iterator<Item = &'a Line>,
    degradations: &mut Vec<Degradation>,
) {
    let mut by_boundary: BTreeMap<usize, Vec<(usize, Option<IrBorder>)>> = BTreeMap::new();
    for line in lines {
        let stroke = line.stroke.as_ref().and_then(|s| ir_border(s));
        by_boundary
            .entry(line_boundary_index(line))
            .or_default()
            .push((line.index, stroke));
    }
    for (boundary, group) in &by_boundary {
        let distinct_positions = group.iter().any(|(i, _)| *i != group[0].0);
        let distinct_strokes = group.iter().any(|(_, s)| *s != group[0].1);
        if distinct_positions && distinct_strokes {
            degradations.push(Degradation {
                span: None,
                construct: "grid-line",
                action: DegradationAction::Approximated,
                message: format!(
                    "lines placed at both ends of the gutter next to boundary {boundary} \
                     collapse onto a single table edge; the last-declared stroke wins"
                ),
            });
        }
    }
}

/// Fold `hline`/`vline` strokes into the cells they bound. A line always
/// wins over the cell's own edge (Typst's `ExplicitLine` priority), including
/// `stroke: none` lines, which fold to a `nil` edge that also clears the
/// cell's declared stroke. Full-span lines already reach `tblBorders` via
/// [`borders_from_lines`], so they only need folding where the table-level
/// border cannot speak: a declared cell edge it must beat, or a colour/dash
/// `tblBorders` cannot carry. Partial lines fold everywhere — Word has no
/// per-segment edges — and a line covering only part of a spanning cell's
/// edge widens to the whole edge, which is reported. Vertical-merge
/// continuation cells are skipped: Typst draws no line inside a merge.
fn fold_lines_into_cells(
    rows: &mut [IrTableRow],
    grid: &CellGrid,
    degradations: &mut Vec<Degradation>,
) {
    report_native_stroke_losses(grid, degradations);
    let n_cols = grid.non_gutter_column_count();
    let n_rows = grid.non_gutter_row_count();
    report_collapsed_line_boundaries(grid.hlines.iter().flatten(), degradations);
    report_collapsed_line_boundaries(grid.vlines.iter().flatten(), degradations);
    fold_hlines_into_cells(rows, grid, n_cols, degradations);
    fold_vlines_into_cells(rows, grid, n_rows, degradations);
}

fn report_native_stroke_losses(grid: &CellGrid, degradations: &mut Vec<Degradation>) {
    let rules = grid
        .hlines
        .iter()
        .chain(&grid.vlines)
        .flatten()
        .filter_map(|line| line.stroke.as_deref());
    let cell_edges = grid
        .entries
        .iter()
        .filter_map(Entry::as_cell)
        .flat_map(|cell| {
            [
                &cell.stroke.top,
                &cell.stroke.right,
                &cell.stroke.bottom,
                &cell.stroke.left,
            ]
        })
        .filter_map(|stroke| stroke.as_deref());
    if rules.chain(cell_edges).any(native_border_style_is_lossy) {
        degradations.push(Degradation {
            span: None,
            construct: "table-border-style",
            action: DegradationAction::Approximated,
            message: "a cell or grid edge has paint, width, cap or dash details not carried by the native border".to_string(),
        });
    }
}

/// Horizontal lines fold onto the top/bottom edges of the cells they bound.
fn fold_hlines_into_cells(
    rows: &mut [IrTableRow],
    grid: &CellGrid,
    n_cols: usize,
    degradations: &mut Vec<Degradation>,
) {
    for group in &grid.hlines {
        for line in group {
            let full = line_spans_full(line, n_cols);
            let boundary = line_boundary_index(line);
            let border = line.stroke.as_ref().and_then(|s| ir_border(s));
            // `tblBorders` cannot express a `stroke: none` full-span line at
            // an inside boundary (`insideH` would still paint it), a colour,
            // or a dash — those fold onto every cell edge regardless.
            let needs_fold = !full
                || border.is_none()
                || border
                    .as_ref()
                    .is_some_and(|b| b.color_hex.is_some() || b.dashed);
            let end = line.end.map_or(n_cols, std::num::NonZero::get);
            let mut widened = false;
            for (ri, row) in rows.iter_mut().enumerate() {
                for cell in &mut row.cells {
                    if cell.vmerge == IrVMerge::Continue {
                        continue;
                    }
                    let colspan = usize::try_from(cell.colspan).unwrap_or(usize::MAX);
                    if cell.x >= end || cell.x.saturating_add(colspan) <= line.start {
                        continue;
                    }
                    let rowspan = usize::try_from(cell.rowspan).unwrap_or(usize::MAX);
                    let mut folded = false;
                    if boundary == ri && (needs_fold || cell.borders_declared.top) {
                        cell.borders.top.clone_from(&border);
                        cell.borders_declared.top = true;
                        folded = true;
                    }
                    if boundary == ri.saturating_add(rowspan)
                        && (needs_fold || cell.borders_declared.bottom)
                    {
                        cell.borders.bottom.clone_from(&border);
                        cell.borders_declared.bottom = true;
                        folded = true;
                    }
                    widened |= folded && (cell.x < line.start || cell.x + colspan > end);
                }
            }
            if widened {
                degradations.push(Degradation {
                    span: None,
                    construct: "grid-line",
                    action: DegradationAction::Approximated,
                    message: "a partial horizontal line covers only part of a cell edge; \
                              the cell's whole edge takes the line's stroke"
                        .to_string(),
                });
            }
        }
    }
}

/// Vertical lines fold onto the left/right edges of the cells they bound.
fn fold_vlines_into_cells(
    rows: &mut [IrTableRow],
    grid: &CellGrid,
    n_rows: usize,
    degradations: &mut Vec<Degradation>,
) {
    for group in &grid.vlines {
        for line in group {
            let full = line_spans_full(line, n_rows);
            let boundary = line_boundary_index(line);
            let border = line.stroke.as_ref().and_then(|s| ir_border(s));
            let needs_fold = !full
                || border.is_none()
                || border
                    .as_ref()
                    .is_some_and(|b| b.color_hex.is_some() || b.dashed);
            let end = line.end.map_or(n_rows, std::num::NonZero::get);
            let mut widened = false;
            for (ri, row) in rows.iter_mut().enumerate() {
                for cell in &mut row.cells {
                    if cell.vmerge == IrVMerge::Continue {
                        continue;
                    }
                    let rowspan = usize::try_from(cell.rowspan).unwrap_or(usize::MAX);
                    if ri >= end || ri.saturating_add(rowspan) <= line.start {
                        continue;
                    }
                    let colspan = usize::try_from(cell.colspan).unwrap_or(usize::MAX);
                    let mut folded = false;
                    if boundary == cell.x && (needs_fold || cell.borders_declared.left) {
                        cell.borders.left.clone_from(&border);
                        cell.borders_declared.left = true;
                        folded = true;
                    }
                    if boundary == cell.x.saturating_add(colspan)
                        && (needs_fold || cell.borders_declared.right)
                    {
                        cell.borders.right.clone_from(&border);
                        cell.borders_declared.right = true;
                        folded = true;
                    }
                    widened |= folded && (ri < line.start || ri + rowspan > end);
                }
            }
            if widened {
                degradations.push(Degradation {
                    span: None,
                    construct: "grid-line",
                    action: DegradationAction::Approximated,
                    message: "a partial vertical line covers only part of a cell edge; \
                              the cell's whole edge takes the line's stroke"
                        .to_string(),
                });
            }
        }
    }
}

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

fn list_marker_at(list: &ListElem, depth: u8, styles: StyleChain<'_>) -> Option<String> {
    match list.marker.get_cloned(styles) {
        ListMarker::Content(markers) => marker_text_at(&markers, depth),
        ListMarker::Func(_) => None,
    }
}

fn list_marker_from_styles(styles: StyleChain<'_>, depth: u8) -> Option<String> {
    match styles.get_cloned(ListElem::marker) {
        ListMarker::Content(markers) => marker_text_at(&markers, depth),
        ListMarker::Func(_) => None,
    }
}

fn marker_text_at(markers: &[Content], depth: u8) -> Option<String> {
    let idx = (depth as usize).min(markers.len().saturating_sub(1));
    markers.get(idx).map(plain_text).filter(|t| !t.is_empty())
}

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

/// The prefix/numeral/suffix split a single-piece rendered numeral gives —
/// what a `SEQ` field needs: the numeral is the field's cached result,
/// prefix+suffix stay literal. A multi-piece *pattern* still yields a
/// single-piece rendered numeral when the counter is single-level (`"1.1"`
/// on a figure renders `1`); a genuinely multi-piece rendered numeral is
/// reported `seq-multi-piece` and stays literal.
fn single_piece_seq(
    numbering: &Numbering,
    display: &str,
    number: u64,
    identifier: &str,
    span: Span,
    degradations: &mut Vec<Degradation>,
) -> Option<IrSeqSplit> {
    let pattern = pattern_from_numbering(numbering, false)?;
    let [piece] = pattern.pieces.as_slice() else {
        // `display` minus the first piece's prefix and the pattern's suffix
        // is the rendered numeral; it is single-piece when none of the
        // later pieces' prefixes rendered.
        let [first, ..] = pattern.pieces.as_slice() else {
            return None;
        };
        let single = display
            .strip_prefix(first.prefix.as_str())
            .and_then(|d| d.strip_suffix(pattern.suffix.as_str()))
            .filter(|inner| rendered_is_single_piece(&pattern.pieces, inner));
        let Some(inner) = single else {
            degradations.push(Degradation {
                span: Some(span),
                construct: "seq-multi-piece",
                action: DegradationAction::Approximated,
                message: format!(
                    "the rendered numeral {display:?} has multiple pieces; \
                     it stays literal"
                ),
            });
            return None;
        };
        return Some(IrSeqSplit {
            prefix: first.prefix.clone(),
            numeral: inner.to_string(),
            suffix: pattern.suffix.clone(),
            system: first.system,
            identifier: identifier.to_string(),
        });
    };
    Some(IrSeqSplit {
        prefix: piece.prefix.clone(),
        numeral: piece.system.render(number),
        suffix: pattern.suffix.clone(),
        system: piece.system,
        identifier: identifier.to_string(),
    })
}

/// `true` when `rendered` is a single-piece numeral for `pieces` — no later
/// piece's prefix appears in it, and every later piece has a non-empty
/// prefix (a prefixless later piece can't be detected, so it counts as
/// rendered).
fn rendered_is_single_piece(pieces: &[IrNumberingPiece], rendered: &str) -> bool {
    pieces.get(1..).is_none_or(|rest| {
        rest.iter()
            .all(|p| !p.prefix.is_empty() && !rendered.contains(p.prefix.as_str()))
    })
}

/// Word's SEQ identifier for a figure kind — matches Word's own caption
/// numbering (` SEQ Figure `, ` SEQ Table `).
fn seq_identifier(kind: IrFigureKind) -> &'static str {
    match kind {
        IrFigureKind::Table => "Table",
        IrFigureKind::Image | IrFigureKind::Other => "Figure",
    }
}

/// The SEQ split of a figure caption's number — `None` unless the caption
/// carries a synthesized counter/numbering/location AND the rendered
/// numeral is a single piece. The numeral text is the counter's own
/// display, so the C3 field's cached result is exactly the C2 literal.
fn caption_number_seq(
    caption: &Packed<FigureCaption>,
    identifier: &str,
    styles: StyleChain<'_>,
    engine: &mut Engine,
    degradations: &mut Vec<Degradation>,
) -> Option<IrSeqSplit> {
    let (Some(Some(numbering)), Some(Some(counter)), Some(Some(location))) = (
        &caption.numbering,
        &caption.counter,
        &caption.figure_location,
    ) else {
        return None;
    };
    let numbers = counter
        .display_at(engine, *location, styles, numbering, caption.span())
        .ok()?;
    let numeral = plain_text(&numbers);
    if numeral.is_empty() {
        return None;
    }
    let pattern = pattern_from_numbering(numbering, false)?;
    let system = match pattern.pieces.as_slice() {
        [] => return None,
        [first, ..] if rendered_is_single_piece(&pattern.pieces, &numeral) => first.system,
        _ => {
            degradations.push(Degradation {
                span: Some(caption.span()),
                construct: "seq-multi-piece",
                action: DegradationAction::Approximated,
                message: format!(
                    "the rendered numeral {numeral:?} has multiple pieces; \
                     it stays literal"
                ),
            });
            return None;
        }
    };
    Some(IrSeqSplit {
        prefix: String::new(),
        numeral,
        suffix: String::new(),
        system,
        identifier: identifier.to_string(),
    })
}

fn enum_numbering(enumer: &EnumElem, styles: StyleChain<'_>) -> Option<IrNumberingPattern> {
    pattern_from_numbering(
        &enumer.numbering.get_cloned(styles),
        enumer.full.get(styles),
    )
}

fn enum_numbering_from_styles(styles: StyleChain<'_>) -> Option<IrNumberingPattern> {
    pattern_from_numbering(
        &styles.get_cloned(EnumElem::numbering),
        styles.get(EnumElem::full),
    )
}

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
    let rendered = piece.system.render(number);
    Some(format!("{}{}{}", piece.prefix, rendered, pattern.suffix))
}

/// The rendered numeral of a `Numbering::Func` applied to one item number —
/// e.g. `n => "§" + str(n) + "."` on 1 → `§1.`. `None` when the function
/// fails (a function reading location state has no location in this
/// location-less context) or returns something that is not text.
fn enum_func_number_text(
    numbering: &Numbering,
    number: u64,
    styles: StyleChain<'_>,
    engine: &mut Engine,
    span: Span,
) -> Option<String> {
    if !matches!(numbering, Numbering::Func(_)) {
        return None;
    }
    let context = typst_library::foundations::Context::new(None, Some(styles));
    let value = numbering
        .apply(
            engine,
            typst::comemo::Track::track(&context),
            span,
            &[number],
        )
        .ok()?;
    match value {
        Value::Str(text) => Some(text.as_str().to_string()),
        Value::Content(body) => {
            let plain = plain_text(&body);
            (!plain.is_empty()).then_some(plain)
        }
        _ => None,
    }
}

/// Text width in pt (page width minus the resolved left/right margins) —
/// the region base `columns` gutters resolve against.
pub(super) fn page_content_width_pt(styles: StyleChain<'_>) -> f64 {
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
    margin_side_resolved(side, page_width_pt, auto_margin)
}

fn margin_side_resolved(side: Option<Smart<Rel<Length>>>, dim_pt: f64, auto_margin: f64) -> f64 {
    let Some(side) = side else {
        return auto_margin;
    };
    match side {
        Smart::Custom(rel) => rel.abs.at(Abs::pt(11.0)).to_pt() + rel.rel.get() * dim_pt,
        Smart::Auto => auto_margin,
    }
}

#[derive(Clone, PartialEq)]
struct PageSnapshot {
    width_pt: f64,
    height_pt: f64,
    top_pt: f64,
    bottom_pt: f64,
    left_pt: f64,
    right_pt: f64,
    /// `margin.two_sided` — inside/outside margins swap on even pages.
    two_sided: bool,
    columns: u32,
    /// `columns(gutter:)` resolved on the chain — `Some` only while the
    /// page is multi-column, so a `#set columns(gutter:)` change on a
    /// single-column page does not spawn a spurious section.
    column_gutter: Option<Rel<Abs>>,
    header_pt: f64,
    footer_pt: f64,
    flipped: bool,
    numbering: Option<Numbering>,
    footer_suppressed: bool,
}

impl PageSnapshot {
    fn resolve(styles: StyleChain<'_>) -> Self {
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
        let Smart::Custom(custom_margin) = &margin else {
            return Self::fixed(width_pt, height_pt, auto_margin, styles);
        };
        let columns = u32::try_from(styles.get(PageElem::columns).get()).unwrap_or(u32::MAX);
        Self {
            width_pt,
            height_pt,
            top_pt: margin_side_resolved(custom_margin.sides.top, height_pt, auto_margin),
            bottom_pt: margin_side_resolved(custom_margin.sides.bottom, height_pt, auto_margin),
            left_pt: margin_side_resolved(custom_margin.sides.left, width_pt, auto_margin),
            right_pt: margin_side_resolved(custom_margin.sides.right, width_pt, auto_margin),
            two_sided: custom_margin.two_sided.unwrap_or(false),
            columns,
            column_gutter: (columns > 1).then(|| styles.get(ColumnsElem::gutter).resolve(styles)),
            header_pt: styles.resolve(PageElem::header_ascent).abs.to_pt(),
            footer_pt: styles.resolve(PageElem::footer_descent).abs.to_pt(),
            flipped: styles.get(PageElem::flipped),
            numbering: styles.get_cloned(PageElem::numbering),
            footer_suppressed: footer_suppressed(styles),
        }
    }

    fn fixed(width_pt: f64, height_pt: f64, auto_margin: f64, styles: StyleChain<'_>) -> Self {
        let columns = u32::try_from(styles.get(PageElem::columns).get()).unwrap_or(u32::MAX);
        Self {
            width_pt,
            height_pt,
            top_pt: auto_margin,
            bottom_pt: auto_margin,
            left_pt: auto_margin,
            right_pt: auto_margin,
            two_sided: false,
            columns,
            column_gutter: (columns > 1).then(|| styles.get(ColumnsElem::gutter).resolve(styles)),
            header_pt: styles.resolve(PageElem::header_ascent).abs.to_pt(),
            footer_pt: styles.resolve(PageElem::footer_descent).abs.to_pt(),
            flipped: styles.get(PageElem::flipped),
            numbering: styles.get_cloned(PageElem::numbering),
            footer_suppressed: footer_suppressed(styles),
        }
    }

    fn to_page_settings(&self) -> PageSettings {
        let (width_pt, height_pt) = if self.flipped {
            (self.height_pt, self.width_pt)
        } else {
            (self.width_pt, self.height_pt)
        };
        PageSettings {
            width_twips: page::pt_to_twips(width_pt),
            height_twips: page::pt_to_twips(height_pt),
            margin_top: page::pt_to_twips(self.top_pt),
            margin_bottom: page::pt_to_twips(self.bottom_pt),
            margin_left: page::pt_to_twips(self.left_pt),
            margin_right: page::pt_to_twips(self.right_pt),
            columns: (self.columns > 1).then_some(self.columns),
            column_spacing: self.column_gutter.map(|gutter| {
                page::columns_gutter_twips(gutter, width_pt - self.left_pt - self.right_pt)
            }),
            header_twips: Some(page::pt_to_twips(self.header_pt)),
            footer_twips: Some(page::pt_to_twips(self.footer_pt)),
            footer_reference: if self.footer_suppressed {
                tydocx_ooxml::document::SectionFooter::Omitted
            } else {
                tydocx_ooxml::document::SectionFooter::Shared
            },
            ..PageSettings::default()
        }
    }
}

fn footer_suppressed(styles: StyleChain<'_>) -> bool {
    styles.get_ref(PageElem::numbering).is_some()
        && matches!(styles.get_ref(PageElem::footer), Smart::Custom(None))
}

fn first_page_anchor_span(content: &Content) -> Option<Span> {
    let span = content.span();
    if span.id().is_some() {
        return Some(span);
    }
    if let Some(par) = content.to_packed::<ParElem>() {
        return first_page_anchor_span(&par.body);
    }
    if let Some(sequence) = content.to_packed::<SequenceElem>() {
        return sequence.children.iter().find_map(first_page_anchor_span);
    }
    let styled = content.to_packed::<StyledElem>()?;
    first_page_anchor_span(&styled.child)
}

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

/// Deepest-first search for a `LinkElem` whose destination is a `Location`.
fn dest_location_in(content: &Content) -> Option<Location> {
    if let Some(direct) = content.to_packed::<DirectLinkElem>() {
        return Some(direct.loc);
    }
    if let Some(link) = content.to_packed::<LinkElem>()
        && let LinkTarget::Dest(Destination::Location(loc)) = &link.dest
    {
        return Some(*loc);
    }
    for child in content_children(content) {
        if !std::ptr::eq(child, content)
            && let Some(loc) = dest_location_in(child)
        {
            return Some(loc);
        }
    }
    None
}

/// Non-gutter track sizings for the IR: `CellGrid` keeps gutter tracks
/// interleaved at odd indices when `has_gutter`.
fn non_gutter_sizes(sizes: &[Sizing], has_gutter: bool) -> Vec<IrTrackSize> {
    sizes
        .iter()
        .enumerate()
        .filter(|(index, _)| !has_gutter || index % 2 == 0)
        .map(|(_, sizing)| ir_track_size(sizing))
        .collect()
}

fn ir_track_size(sizing: &Sizing) -> IrTrackSize {
    match sizing {
        Sizing::Auto => IrTrackSize::Auto,
        Sizing::Fr(fr) => IrTrackSize::Fr(fr.get()),
        Sizing::Rel(rel) => {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "track sizes are small non-negative lengths"
            )]
            let abs_twips = (rel.abs.abs.to_pt() * 20.0).round().max(0.0) as u32;
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "ratios are non-negative"
            )]
            let permille = (rel.rel.get() * 1000.0).round().max(0.0) as u32;
            if permille == 0 {
                IrTrackSize::Fixed(abs_twips)
            } else if abs_twips == 0 {
                IrTrackSize::Rel {
                    abs_twips: 0,
                    permille,
                }
            } else {
                IrTrackSize::Rel {
                    abs_twips,
                    permille,
                }
            }
        }
    }
}

/// A resolved cell's own fill/alignment/inset properties, read from the
/// `table.cell`/`grid.cell` element it was resolved from (falling back to
/// the `Cell`'s resolved fill).
fn cell_style_props(
    cell: &Cell,
    table_cell: Option<&Packed<TableCell>>,
    grid_cell: Option<&Packed<GridCell>>,
    styles: StyleChain<'_>,
) -> (
    Option<String>,
    Option<VerticalAlign>,
    Option<Alignment>,
    Option<u32>,
) {
    let fill_hex = cell.fill.as_ref().and_then(paint_to_shd_hex).or_else(|| {
        table_cell
            .and_then(|tc| match tc.fill.get_cloned(styles) {
                Smart::Custom(Some(paint)) => paint_to_shd_hex(&paint),
                _ => None,
            })
            .or_else(|| {
                grid_cell.and_then(|gc| match gc.fill.get_cloned(styles) {
                    Smart::Custom(Some(paint)) => paint_to_shd_hex(&paint),
                    _ => None,
                })
            })
    });
    let cell_align = table_cell
        .map(|tc| tc.align.get_cloned(styles))
        .or_else(|| grid_cell.map(|gc| gc.align.get_cloned(styles)));
    let vertical_align = cell_align
        .and_then(|a| match a {
            Smart::Custom(align) => align.y(),
            Smart::Auto => None,
        })
        .map(|v| match v {
            VAlignment::Top => VerticalAlign::Top,
            VAlignment::Horizon => VerticalAlign::Center,
            VAlignment::Bottom => VerticalAlign::Bottom,
        });
    let horizontal_align = cell_align
        .and_then(|a| match a {
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
    (fill_hex, vertical_align, horizontal_align, inset_twips)
}

fn ir_border(stroke: &typst_library::visualize::Stroke<Abs>) -> Option<IrBorder> {
    let pt = stroke.thickness.custom().map_or(1.0, Abs::to_pt);
    if pt <= 0.0 {
        return None;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "stroke thickness is positive"
    )]
    let sz_eighths = (pt * 8.0).round().max(1.0) as u32;
    Some(IrBorder {
        sz_eighths,
        color_hex: match &stroke.paint {
            Smart::Custom(paint) => paint_to_shd_hex(paint),
            Smart::Auto => None,
        },
        dashed: matches!(&stroke.dash, Smart::Custom(Some(_))),
    })
}

fn cell_borders_from_stroke(
    stroke: &Sides<Option<std::sync::Arc<typst_library::visualize::Stroke<Abs>>>>,
) -> IrCellBorders {
    IrCellBorders {
        top: stroke.top.as_ref().and_then(|s| ir_border(s)),
        right: stroke.right.as_ref().and_then(|s| ir_border(s)),
        bottom: stroke.bottom.as_ref().and_then(|s| ir_border(s)),
        left: stroke.left.as_ref().and_then(|s| ir_border(s)),
    }
}

/// The non-gutter boundary index a resolved `Line` draws on: `Before` sits
/// above track `index`, `After` (which only survives Typst's own
/// normalization on a guttered grid) sits below track `index` — the same
/// DOCX edge as `index + 1`.
fn line_boundary_index(line: &Line) -> usize {
    line.index + usize::from(line.position == LinePosition::After)
}

/// Convert a resolved `Line`, keeping a `stroke: none` line as an explicit
/// removal (`border: None`) rather than dropping it.
fn ir_grid_line(line: &Line, track_count: usize) -> IrGridLine {
    IrGridLine {
        index: line_boundary_index(line),
        start: line.start,
        end: line.end.map_or(track_count, std::num::NonZero::get),
        border: line.stroke.as_ref().and_then(|s| ir_border(s)),
    }
}

/// Lay the grid out once against the page content area and derive row
/// heights from the declared track sizings plus the laid-out frame's total
/// height: `fixed`/`rel` tracks keep their declared size, `fr` tracks share
/// whatever remains, and `auto` tracks keep their own measured content
/// height — a tall auto row (like a timetable title) would otherwise
/// collapse to the fr share and inflate every slot by the difference. Rows
/// whose height can't be measured fall back to a weight-1 share. Heights
/// floor to whole twips so slots never inflate.
fn measure_grid_row_heights(
    grid: &CellGrid,
    content: &Content,
    styles: StyleChain<'_>,
    engine: &mut Engine,
    locator: &mut SplitLocator,
) -> Option<Vec<Option<u32>>> {
    if grid.rows.is_empty() {
        return None;
    }
    let sizes = non_gutter_sizes(&grid.rows, grid.has_gutter);
    if sizes.is_empty() {
        return None;
    }
    let width_pt = page_content_width_pt(styles);
    let height_pt = page_content_height_pt(styles);
    if !(width_pt.is_finite() && height_pt.is_finite() && width_pt > 0.0 && height_pt > 0.0) {
        return None;
    }
    let region = Region::new(
        Size::new(Abs::pt(width_pt), Abs::pt(height_pt)),
        Axes::splat(true),
    );
    let frame = typst_layout::layout_frame(
        engine,
        content,
        locator.next(&content.span()),
        styles,
        region,
    )
    .ok()?;
    let total_pt = frame.size().y.to_pt();
    let mut declared: Vec<Option<f64>> = Vec::with_capacity(sizes.len());
    let mut fixed_total = 0.0;
    for sizing in &sizes {
        let pt = match sizing {
            IrTrackSize::Fixed(t) => Some(f64::from(*t) / 20.0),
            IrTrackSize::Rel {
                abs_twips,
                permille,
            } => Some(f64::from(*abs_twips) / 20.0 + height_pt * f64::from(*permille) / 1000.0),
            _ => None,
        };
        declared.push(pt);
        fixed_total += pt.unwrap_or(0.0);
    }
    let mut measured_auto: Vec<Option<f64>> = vec![None; sizes.len()];
    let mut fr_total = 0.0;
    let col_widths = grid_col_widths_pt(grid, width_pt);
    for (index, sizing) in sizes.iter().enumerate() {
        match sizing {
            IrTrackSize::Auto => {
                let y = if grid.has_gutter { index * 2 } else { index };
                measured_auto[index] =
                    measure_auto_row_pt(grid, y, &col_widths, styles, engine, locator);
                fr_total += f64::from(u8::from(measured_auto[index].is_none()));
            }
            IrTrackSize::Fr(w) => fr_total += *w,
            _ => {}
        }
    }
    let remaining = (total_pt - fixed_total - measured_auto.iter().flatten().sum::<f64>()).max(0.0);
    let mut heights = Vec::with_capacity(sizes.len());
    for (index, sizing) in sizes.iter().enumerate() {
        let pt = match sizing {
            IrTrackSize::Fr(w) if fr_total > 0.0 => Some(remaining * w / fr_total),
            IrTrackSize::Auto => {
                measured_auto[index].or_else(|| (fr_total > 0.0).then_some(remaining / fr_total))
            }
            IrTrackSize::Fr(_) => None,
            _ => declared[index],
        };
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "row heights floor to non-negative twips"
        )]
        let twips = pt.map(|p| (p.max(0.0) * 20.0).floor() as u32);
        heights.push(twips);
    }
    Some(heights)
}

/// Resolved widths in points for the grid's non-gutter columns: `fr` tracks
/// share what the fixed and relative tracks leave, `auto` tracks are
/// content-sized and can't be resolved here.
fn grid_col_widths_pt(grid: &CellGrid, width_pt: f64) -> Vec<Option<f64>> {
    let sizings: Vec<&Sizing> = grid
        .cols
        .iter()
        .enumerate()
        .filter(|(index, _)| !grid.has_gutter || index % 2 == 0)
        .map(|(_, sizing)| sizing)
        .collect();
    let fixed: f64 = sizings
        .iter()
        .map(|sizing| match sizing {
            Sizing::Rel(rel) => rel.abs.abs.to_pt(),
            _ => 0.0,
        })
        .sum();
    let rel_share: f64 = sizings
        .iter()
        .map(|sizing| match sizing {
            Sizing::Rel(rel) => rel.rel.get() * width_pt,
            _ => 0.0,
        })
        .sum();
    let fr_total: f64 = sizings
        .iter()
        .map(|sizing| match sizing {
            Sizing::Fr(fr) => fr.get(),
            _ => 0.0,
        })
        .sum();
    let remaining = (width_pt - fixed - rel_share).max(0.0);
    sizings
        .iter()
        .map(|sizing| match sizing {
            Sizing::Auto => None,
            Sizing::Rel(rel) => Some(rel.abs.abs.to_pt() + rel.rel.get() * width_pt),
            Sizing::Fr(fr) if fr_total > 0.0 => Some(remaining * fr.get() / fr_total),
            Sizing::Fr(_) => Some(0.0),
        })
        .collect()
}

/// One `auto` row's natural height in points: the tallest laid-out body
/// among its cells plus the cell's own top/bottom insets. `None` when a
/// `rowspan > 1` cell starts here (its content would measure against one row
/// instead of spreading over the span) or when a spanned column's width is
/// content-sized and unresolvable — the caller keeps the share fallback.
fn measure_auto_row_pt(
    grid: &CellGrid,
    y: usize,
    col_widths: &[Option<f64>],
    styles: StyleChain<'_>,
    engine: &mut Engine,
    locator: &mut SplitLocator,
) -> Option<f64> {
    let mut height = 0.0_f64;
    for x in 0..grid.cols.len() {
        let Some(Entry::Cell(cell)) = grid.entry(x, y) else {
            continue;
        };
        if cell.rowspan.get() > 1 {
            return None;
        }
        let col = if grid.has_gutter { x / 2 } else { x };
        let end = (col + cell.colspan.get()).min(col_widths.len());
        let mut width_pt = 0.0;
        for c in col..end {
            width_pt += col_widths.get(c).copied().flatten()?;
        }
        let grid_cell = cell.body.to_packed::<GridCell>();
        let table_cell = cell.body.to_packed::<TableCell>();
        let body = table_cell.map_or_else(
            || grid_cell.map_or_else(|| cell.body.clone(), |gc| gc.body.clone()),
            |tc| tc.body.clone(),
        );
        let region = Region::new(
            Size::new(Abs::pt(width_pt.max(1.0)), Abs::pt(96_000.0)),
            Axes::new(true, false),
        );
        let frame =
            typst_layout::layout_frame(engine, &body, locator.next(&body.span()), styles, region)
                .ok()?;
        let inset_pt = cell_vertical_inset_pt(table_cell, grid_cell, styles);
        height = height.max(frame.size().y.to_pt() + inset_pt);
    }
    Some(height)
}

/// A cell's top plus bottom insets in points, read from the cell element's
/// own `inset` property; default/auto insets contribute nothing.
fn cell_vertical_inset_pt(
    table_cell: Option<&Packed<TableCell>>,
    grid_cell: Option<&Packed<GridCell>>,
    styles: StyleChain<'_>,
) -> f64 {
    let sides = table_cell
        .map(|tc| tc.inset.get_cloned(styles))
        .or_else(|| grid_cell.map(|gc| gc.inset.get_cloned(styles)));
    let Some(Smart::Custom(sides)) = sides else {
        return 0.0;
    };
    let em = styles.resolve(TextElem::size);
    let side = |side: Option<Rel<Length>>| side.map_or(0.0, |rel| rel.abs.at(em).to_pt());
    side(sides.top) + side(sides.bottom)
}

/// The realize and Paged compilations run in the same process, where
/// `Location::hash` is deterministic — the paged frame's figure brackets and
/// the collected `IrBlock::Figure` meet through this hash.
pub(crate) fn location_hash64(location: Location) -> u64 {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "hash key truncates the 128-bit location hash"
    )]
    let hash = location.hash() as u64;
    hash
}

pub(super) fn page_content_height_pt(styles: StyleChain<'_>) -> f64 {
    let snapshot = PageSnapshot::resolve(styles);
    let height_pt = if snapshot.flipped {
        snapshot.width_pt
    } else {
        snapshot.height_pt
    };
    (height_pt - snapshot.top_pt - snapshot.bottom_pt).max(0.0)
}

/// Join source diagnostics for a degradation message.
fn diag_text(errs: &[typst::diag::SourceDiagnostic]) -> String {
    errs.iter()
        .map(|d| d.message.as_str())
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn piece(prefix: &str) -> IrNumberingPiece {
        IrNumberingPiece {
            prefix: prefix.to_owned(),
            system: IrNumeralSystem::Arabic,
        }
    }

    #[test]
    fn rendered_numeral_arity_decides_the_seq_guard() {
        let one = [piece("")];
        let two = [piece(""), piece(".")];
        let two_prefixless = [piece(""), piece("")];
        // A single-piece pattern renders a single-piece numeral.
        assert!(rendered_is_single_piece(&one, "1"));
        // A "1.1"-pattern on a single-level counter renders "1" — the second
        // piece's prefix never appears, so the numeral is single-piece.
        assert!(rendered_is_single_piece(&two, "1"));
        assert!(rendered_is_single_piece(&two, "(1)"));
        // A genuinely multi-piece rendered numeral.
        assert!(!rendered_is_single_piece(&two, "1.1"));
        // A prefixless later piece can't be detected — count it as rendered.
        assert!(!rendered_is_single_piece(&two_prefixless, "1"));
    }
}
