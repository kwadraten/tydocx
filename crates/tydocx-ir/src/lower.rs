use std::collections::HashMap;
use std::fmt::Write;

use tydocx_ooxml::document::{
    AnchorWrap, AnchoredContent, AnchoredFloat, BlockElement, CellBorders, CellContent, Document,
    EquationNumber, FirstLineIndent, FootnoteBookmark, FootnoteFormat, ImageData, ImageFormat,
    InlineElement, LevelOverride, ListInfo, PageNumberFormat, PageSettings, Paragraph, RelFrom,
    SectionBreak, SectionBreakType, Table, TableBorders, TableCell, TableCellMargins, TableFloat,
    TableRow, VMerge, truncate_bookmark_name,
};

use crate::degradation::{Degradation, DegradationAction};
use crate::geometry::GeometryInput;
use crate::grid_plan::{MAX_COLUMNS, PlannedCell, PlannedCellKind, PlannedTable, plan_grid};
use crate::mode::CompatMode;
use crate::types::{
    Alignment, HangingIndent, InlineShape, IrAnchor, IrBibEntry, IrBlock, IrCaptionSeq,
    IrCellBorderFlags, IrCellBorders, IrDocument, IrEnum, IrEquation, IrField, IrFootnote, IrGrid,
    IrHeadingNumbering, IrImage, IrInline, IrLevelDef, IrList, IrNumberingPattern, IrOutlineEntry,
    IrParStyle, IrParagraph, IrPlaceWrap, IrRaw, IrSectionBreakType, IrShapeGeometry, IrShapeKind,
    IrTable, IrTableCell, IrTableRow, IrVMerge, ParagraphStyle, Run,
};

#[derive(Debug, Default)]
pub struct LowerCtx {
    pub compat: CompatMode,
    pub body_size_half_pt: u32,
    pub heading_sizes: [u32; 5],
    pub footnote_ids: HashMap<u32, u32>,
    pub footnote_marks: HashMap<u32, String>,
    pub next_bookmark_id: u32,
    pub degradations: Vec<Degradation>,
    /// Simulated heading-numbering counters per num instance (`numId`), used
    /// to compare Word's expected number against the number Typst rendered —
    /// a source-side `#counter(heading).update` shifts the two apart.
    pub heading_counters: HashMap<u32, HeadingNumState>,
    /// Page content width in twips — the width a top-level grid fills, used
    /// to resolve `fr` track widths for fixed-layout tables.
    pub content_width_twips: Option<u32>,
    /// Whether an unrepresentable heading numbering was already reported.
    pub unmappable_heading_reported: bool,
    /// The Normal style's first-line indent — the comparison baseline for
    /// per-paragraph `par.first-line-indent` overrides.
    pub normal_first_line_twips: i64,
    /// The Normal style's char-based first-line indent, if it has one.
    pub normal_first_line_chars: Option<i64>,
    /// The Normal style's justification (`body_alignment == "both"`).
    pub normal_justify: bool,
    /// Running z-order for `wp:anchor@relativeHeight` — each anchored
    /// drawing is numbered in lowering (document) order so later `#place`
    /// content stacks over earlier content, matching Typst's paint order.
    pub next_anchor_z: u32,
    /// Page width in EMU — the span an overlay text box is grown to so its
    /// (already positioned) text can never re-wrap.
    pub page_width_emu: i64,
    /// Page height in EMU — likewise grown so top-anchored text is never
    /// clipped by the measured ink height.
    pub page_height_emu: i64,
    /// Text-area left edge in EMU — the `relativeFrom="margin"` origin; a
    /// placed box fully left of it is a margin note.
    pub text_left_emu: i64,
    /// Text-area right edge in EMU — a placed box fully right of it sits in
    /// the right margin.
    pub text_right_emu: i64,
    /// Labels attached to the document's footnotes — the set a `NOTEREF`
    /// field may target (only labelled footnotes get a bookmark).
    pub footnote_labels: std::collections::HashSet<String>,
    /// `SEQ` identifiers for literal enums are document-global (`enum-1`,
    /// `enum-2`, …) — allocated in lowering order.
    pub next_enum_seq: u32,
}

#[derive(Debug, Default)]
pub struct HeadingNumState {
    /// Counter values for heading levels 0..=8 (index = level - 1).
    pub counters: [u32; 9],
    /// Whether a counter divergence was already reported for this instance.
    pub reported: bool,
    pub diverged: bool,
}

impl LowerCtx {
    fn next_bookmark(&mut self) -> u32 {
        let id = self.next_bookmark_id;
        self.next_bookmark_id = self.next_bookmark_id.saturating_add(1);
        id
    }

    fn degrade(&mut self, construct: &'static str, action: DegradationAction, message: String) {
        self.degrade_at(None, construct, action, message);
    }

    fn degrade_at(
        &mut self,
        span: Option<typst_syntax::Span>,
        construct: &'static str,
        action: DegradationAction,
        message: String,
    ) {
        self.degradations.push(Degradation {
            span,
            construct,
            action,
            message,
        });
    }
}

#[derive(Debug, Clone, Copy)]
struct ListAlloc {
    num_id: u32,
    depth: u32,
}

#[must_use]
pub fn lower(
    ir: &IrDocument,
    geometry: &GeometryInput,
    compat: CompatMode,
) -> (Document, Vec<Degradation>) {
    let mut doc = Document::new();
    doc.page_settings = ir.page.clone();
    doc.style = ir.style.clone();
    doc.metadata.title.clone_from(&ir.info.title);
    doc.metadata.author.clone_from(&ir.info.author);
    doc.metadata.description.clone_from(&ir.info.description);
    doc.metadata.created.clone_from(&ir.info.created);
    if !ir.info.keywords.is_empty() {
        doc.metadata.keywords = Some(ir.info.keywords.join(", "));
    }
    doc.citation_sources.clone_from(&ir.citation_sources);
    doc.header.clone_from(&geometry.header);
    doc.header_even.clone_from(&geometry.header_even);
    doc.header_first.clone_from(&geometry.header_first);
    doc.footer.clone_from(&geometry.footer);
    doc.footer_even.clone_from(&geometry.footer_even);
    doc.footer_first.clone_from(&geometry.footer_first);
    doc.page_numbering = geometry.page_numbering;
    doc.page_number_start = geometry.page_number_start;
    doc.page_number_footers = geometry.page_number_footers.clone();
    doc.even_and_odd_headers = geometry.even_and_odd_headers;
    // C1 keeps the report-and-no-flag contract for inside/outside margins.
    doc.mirror_margins = geometry.mirror_margins && !compat.is_c1();
    doc.title_page = geometry.title_page;
    doc.update_fields = compat.is_c3();

    let mut ctx = LowerCtx {
        compat,
        body_size_half_pt: ir.style.body_size_half_pt,
        heading_sizes: ir.style.heading_sizes,
        footnote_ids: HashMap::new(),
        footnote_marks: HashMap::new(),
        next_bookmark_id: doc.bookmark_counter,
        degradations: Vec::new(),
        heading_counters: HashMap::new(),
        unmappable_heading_reported: false,
        content_width_twips: ir
            .page
            .width_twips
            .checked_sub(ir.page.margin_left + ir.page.margin_right),
        normal_first_line_twips: i64::from(ir.style.first_line_indent_twips),
        normal_first_line_chars: ir.style.first_line_indent_chars.map(i64::from),
        normal_justify: ir.style.body_alignment == "both",
        next_anchor_z: 0,
        page_width_emu: i64::from(ir.page.width_twips) * 635,
        page_height_emu: i64::from(ir.page.height_twips) * 635,
        text_left_emu: i64::from(ir.page.margin_left) * 635,
        text_right_emu: i64::from(ir.page.width_twips - ir.page.margin_right) * 635,
        footnote_labels: ir
            .footnotes
            .iter()
            .filter_map(|f| f.label.clone())
            .collect(),
        next_enum_seq: 0,
    };

    lower_footnotes(ir, compat, &mut doc, &mut ctx);

    lower_body(ir, geometry, &mut doc, &mut ctx);

    if compat.is_c1() && !ir.page_furniture.is_empty() {
        ctx.degrade(
            "page-foreground",
            DegradationAction::Literalized,
            "page foreground/background content has no Word equivalent; its text is kept as \
             literal body paragraphs"
                .to_string(),
        );
        let mut furniture: Vec<BlockElement> = ir
            .page_furniture
            .iter()
            .map(|text| {
                let mut para = Paragraph::new();
                para.push_run(Run::new(text.clone()));
                BlockElement::Paragraph(para)
            })
            .collect();
        furniture.append(&mut doc.body.elements);
        doc.body.elements = furniture;
    }

    doc.bookmark_counter = ctx.next_bookmark_id;
    let degradations = ctx.degradations;
    (doc, degradations)
}

fn lower_footnotes(ir: &IrDocument, compat: CompatMode, doc: &mut Document, ctx: &mut LowerCtx) {
    let mark_plan = footnote_mark_plan(
        &ir.footnotes
            .iter()
            .map(|f| f.rendered.as_str())
            .collect::<Vec<_>>(),
        ir.style.footnote_restart_each_page,
    );
    let custom_marks = mark_plan == FootnoteFormat::CustomMarks;
    doc.style.footnote_format = mark_plan;
    if custom_marks {
        ctx.degrade(
            "footnote-numbering",
            DegradationAction::Approximated,
            "the evaluated footnote markers are not a numbering Word can regenerate; each mark \
             is kept as literal text on a customMarkFollows reference"
                .to_string(),
        );
    }
    for footnote in &ir.footnotes {
        let content = lower_footnote_body(footnote, ctx);
        let id = doc.add_footnote(content);
        if let Some(entry) = doc.footnotes.last_mut() {
            if custom_marks {
                entry.custom_mark = Some(footnote.rendered.clone());
                ctx.footnote_marks.insert(id, footnote.rendered.clone());
            }
            if compat.is_c3()
                && let Some(label) = &footnote.label
            {
                entry.bookmark = Some(FootnoteBookmark {
                    id: ctx.next_bookmark(),
                    name: truncate_bookmark_name(label).to_string(),
                });
            }
        }
        ctx.footnote_ids.insert(footnote.ordinal, id);
    }
}

/// Sections after the first end the previous section with a `w:sectPr` on
/// its last paragraph; the final section's settings close the body.
///
/// A `w:sectPr` describes the section that *ends* at it: its `w:type`
/// records how that section started. `lower_body` therefore tracks
/// `current_section_start` — the start type of the section currently
/// collecting paragraphs — and stamps it on the break that ends the
/// section. `IrSection` boundaries always open on a fresh page; a
/// `#columns` block instead opens and closes `continuous` sections
/// mid-page, so the ambient section that follows it starts `continuous`
/// too.
fn lower_body(ir: &IrDocument, geometry: &GeometryInput, doc: &mut Document, ctx: &mut LowerCtx) {
    let mut sections = ir.sections.iter().peekable();
    let mut state = SectionState {
        settings: ir.page.clone(),
        numbering: if ir.sections.is_empty() {
            geometry.page_numbering
        } else {
            ir.first_numbering.or(geometry.page_numbering)
        },
        numbering_start: geometry.page_number_start,
        start_type: SectionBreakType::NextPage,
    };
    let mut first_break_emitted = false;
    // Once a columns block has run, `state.settings` is the ambient
    // section restored after it — assign the tracked values even when no
    // `IrSection` exists.
    let mut columns_lowered = false;
    let mut page = FreshPage::Armed;
    for (index, block) in ir.body.iter().enumerate() {
        while let Some(next) = sections.next_if(|s| s.start_block == index) {
            if !doc.body.elements.is_empty() {
                attach_section_break(
                    &mut doc.body.elements,
                    state.ending_break(!first_break_emitted && geometry.title_page),
                    true,
                );
                first_break_emitted = true;
                page = FreshPage::Unarmed;
            }
            state.settings = next.settings.clone();
            state.numbering = next.numbering.or(state.numbering);
            state.numbering_start = next.numbering_start;
            state.start_type = SectionBreakType::NextPage;
        }
        if let IrBlock::Columns {
            count,
            gutter_twips,
            body,
            ..
        } = block
        {
            let title_page = !first_break_emitted && geometry.title_page;
            lower_columns_block(
                *count,
                *gutter_twips,
                body,
                &mut state,
                title_page,
                doc,
                ctx,
            );
            first_break_emitted = true;
            columns_lowered = true;
            page = FreshPage::No;
            continue;
        }
        if let IrBlock::PageBreak { weak, to } = block {
            let next_transition = match ir.body.get(index + 1) {
                None | Some(IrBlock::PageBreak { .. }) => true,
                _ => sections.peek().is_some_and(|s| s.start_block == index + 1),
            };
            page = lower_body_break(*weak, *to, page, next_transition, &mut doc.body.elements);
            continue;
        }
        let lowered = lower_block(block, doc, ctx);
        if !lowered.is_empty() {
            page = FreshPage::No;
        }
        doc.body.elements.extend(lowered);
    }
    if !ir.sections.is_empty() || columns_lowered {
        doc.page_settings = state.settings;
        doc.page_numbering = state.numbering;
        doc.page_number_start = state.numbering_start;
    }
    if columns_lowered {
        // `nextPage` is OOXML's default — only a continuous start needs a
        // `w:type` on the body-final sectPr.
        doc.final_section_type =
            (state.start_type != SectionBreakType::NextPage).then_some(state.start_type);
    }
}

/// The section currently collecting paragraphs in `lower_body` — the
/// page settings and numbering a closing `w:sectPr` stamps, plus how the
/// section started (a `nextPage` or `continuous` `w:type`).
struct SectionState {
    settings: PageSettings,
    numbering: Option<PageNumberFormat>,
    numbering_start: Option<u32>,
    /// How this section opened — the `w:type` written on the break that
    /// ends it.
    start_type: SectionBreakType,
}

impl SectionState {
    /// The `w:sectPr` ending this section.
    fn ending_break(&self, title_page: bool) -> SectionBreak {
        SectionBreak {
            break_type: self.start_type,
            page_settings: Some(self.settings.clone()),
            title_page,
            page_numbering: self.numbering,
            page_number_start: self.numbering_start,
        }
    }
}

/// Lower a top-level `#columns` block as a `continuous` section pair: a
/// break ending the ambient section, the block body under a section that
/// carries `w:cols`, then a break ending the columns section. The ambient
/// section resumes afterwards and starts `continuous`, so `state` tracks
/// that for whatever break eventually ends it. A document-final ambient
/// section never emits a break — its settings and `w:type` close the body.
fn lower_columns_block(
    count: u32,
    gutter_twips: u32,
    body: &[IrBlock],
    state: &mut SectionState,
    title_page: bool,
    doc: &mut Document,
    ctx: &mut LowerCtx,
) {
    // When the block opens the document there is no earlier section to
    // close; `title_page` then belongs to the columns section, which the
    // outgoing break ends.
    let mut pending_title_page = title_page;
    if !doc.body.elements.is_empty() {
        // A page-setting boundary or a previous block's out-break already
        // stamped a `w:sectPr` on the last paragraph — a paragraph carries
        // one, so the empty ambient stretch gets a marker rather than
        // overwriting it. An unstamped odd/even `pagebreak(to:)` keeps
        // merging normally.
        let ambient_is_empty = matches!(
            doc.body.elements.last(),
            Some(BlockElement::Paragraph(p))
                if p.section_break.as_ref().is_some_and(|b| {
                    matches!(
                        b.break_type,
                        SectionBreakType::NextPage | SectionBreakType::Continuous
                    )
                })
        );
        if ambient_is_empty {
            let mut marker = Paragraph::new();
            marker.mark_size_half_pt = Some(2);
            marker.spacing_before = Some(0);
            marker.spacing_after = Some(0);
            doc.body.elements.push(BlockElement::Paragraph(marker));
        }
        attach_section_break(
            &mut doc.body.elements,
            state.ending_break(title_page),
            false,
        );
        // The restart belongs to the OOXML section that began it — the one
        // the in-break just ended. The columns section and the resumed
        // ambient section continue numbering, so they must not re-stamp
        // `w:start` and re-initiate the restart.
        state.numbering_start = None;
        pending_title_page = false;
    }
    let mut col_settings = state.settings.clone();
    col_settings.columns = Some(count);
    col_settings.column_spacing = Some(gutter_twips);
    // Fixed-layout grids inside the block fill one column, not the text
    // column — narrow `fr` resolution to the per-column width.
    let column_width_twips = state
        .settings
        .width_twips
        .saturating_sub(state.settings.margin_left + state.settings.margin_right)
        .saturating_sub(gutter_twips.saturating_mul(count.saturating_sub(1)))
        / count;
    let saved_content_width = ctx.content_width_twips;
    ctx.content_width_twips = Some(column_width_twips);
    for inner in body {
        let lowered = lower_block(inner, doc, ctx);
        doc.body.elements.extend(lowered);
    }
    ctx.content_width_twips = saved_content_width;
    attach_section_break(
        &mut doc.body.elements,
        SectionBreak {
            break_type: SectionBreakType::Continuous,
            page_settings: Some(col_settings),
            title_page: pending_title_page,
            page_numbering: state.numbering,
            // `take()` — a columns block opening the document has no
            // in-break, so its section carries the initial restart itself;
            // either way the value is consumed by the first sectPr emitted.
            page_number_start: state.numbering_start.take(),
        },
        false,
    );
    state.start_type = SectionBreakType::Continuous;
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FreshPage {
    No,
    Armed,
    Unarmed,
}

fn lower_body_break(
    weak: bool,
    to: Option<IrSectionBreakType>,
    page: FreshPage,
    next_transition: bool,
    elements: &mut Vec<BlockElement>,
) -> FreshPage {
    if weak {
        if page != FreshPage::No || next_transition {
            return page;
        }
        elements.extend(lower_pagebreak(false, to));
        return FreshPage::Unarmed;
    }
    if to.is_none() && page == FreshPage::Unarmed {
        return FreshPage::Armed;
    }
    elements.extend(lower_pagebreak(false, to));
    FreshPage::Armed
}

/// Attach the section-break `w:sectPr` that ends a section to the last
/// element emitted so far. A trailing paragraph takes the break in its
/// `w:pPr`; any other trailing element (table, image) leaves the section
/// boundary itself unmarked, so an empty marker paragraph carries it.
/// `pop_page_breaks` clears trailing `w:br` page breaks the boundary
/// subsumes — a `nextPage` transition replaces them; a `continuous`
/// columns boundary does not, so a `#pagebreak()` beside it is kept.
fn attach_section_break(
    elements: &mut Vec<BlockElement>,
    section: SectionBreak,
    pop_page_breaks: bool,
) {
    if let Some(BlockElement::Paragraph(para)) = elements.last_mut() {
        if pop_page_breaks {
            while matches!(para.inlines.last(), Some(InlineElement::PageBreak)) {
                para.inlines.pop();
            }
        }
        if let Some(existing) = &mut para.section_break
            && matches!(
                existing.break_type,
                SectionBreakType::OddPage | SectionBreakType::EvenPage
            )
        {
            existing.page_settings = section.page_settings;
            existing.title_page = section.title_page;
            existing.page_numbering = section.page_numbering;
            existing.page_number_start = section.page_number_start;
            return;
        }
        para.section_break = Some(section);
    } else {
        let mut para = Paragraph::new();
        para.section_break = Some(section);
        para.mark_size_half_pt = Some(2);
        para.spacing_before = Some(0);
        para.spacing_after = Some(0);
        elements.push(BlockElement::Paragraph(para));
    }
}

fn shade_shape_block(elements: &mut Vec<BlockElement>, fill_hex: &str) {
    let mut shaded = false;
    for el in elements.iter_mut() {
        if let BlockElement::Paragraph(para) = el {
            para.shading = Some(fill_hex.to_string());
            shaded = true;
        }
    }
    if !shaded {
        let mut para = Paragraph::new();
        para.shading = Some(fill_hex.to_string());
        para.mark_size_half_pt = Some(2);
        para.spacing_before = Some(0);
        para.spacing_after = Some(0);
        elements.push(BlockElement::Paragraph(para));
    }
}

fn lower_footnote_body(footnote: &IrFootnote, ctx: &mut LowerCtx) -> Vec<Paragraph> {
    footnote
        .body
        .iter()
        .map(|para| lower_paragraph(para, ctx))
        .collect()
}

fn lower_block(block: &IrBlock, doc: &mut Document, ctx: &mut LowerCtx) -> Vec<BlockElement> {
    lower_block_in(block, doc, ctx, None)
}

fn lower_block_in(
    block: &IrBlock,
    doc: &mut Document,
    ctx: &mut LowerCtx,
    inherited: Option<ListAlloc>,
) -> Vec<BlockElement> {
    match block {
        IrBlock::Paragraph(para) => vec![BlockElement::Paragraph(lower_paragraph(para, ctx))],
        IrBlock::Heading { .. } => lower_heading_block(block, doc, ctx),
        IrBlock::Table(table) => lower_table_block(table, doc, ctx),
        IrBlock::Grid(grid) => {
            if ctx.compat.is_c1() {
                ctx.degrade(
                    "grid-layout",
                    DegradationAction::Skipped,
                    "grid layout dropped; cell text is kept as paragraphs".to_string(),
                );
                lower_grid_flat(grid, doc, ctx)
            } else {
                lower_grid(grid, doc, ctx)
            }
        }
        IrBlock::Figure { .. } => lower_figure_block(block, doc, ctx),
        IrBlock::List(list) => lower_list(list, doc, ctx, inherited),
        IrBlock::Enum(enumer) => lower_enum(enumer, doc, ctx, inherited),
        IrBlock::Equation(eq) => lower_equation(eq, ctx),
        IrBlock::Quote { body } => lower_quote(body, doc, ctx),
        IrBlock::Raw(raw) => lower_raw(raw),
        IrBlock::Outline {
            title,
            entries,
            depth,
            custom_target,
            indent_twips_per_level,
            figure_caption_id,
        } => lower_outline(
            title,
            entries,
            *depth,
            *custom_target,
            *indent_twips_per_level,
            figure_caption_id.as_deref(),
            doc,
            ctx,
        ),
        IrBlock::Bibliography { title, entries } => lower_bibliography(title, entries, doc, ctx),
        IrBlock::Place {
            float,
            wrap,
            clearance_emu,
            location_hash: _,
            anchor,
            body,
        } => lower_place(
            &PlaceSpec {
                float: *float,
                wrap: *wrap,
                clearance_emu: *clearance_emu,
                anchor: anchor.clone(),
                body,
            },
            doc,
            ctx,
        ),
        IrBlock::PageBreak { weak, to } => lower_pagebreak(*weak, *to),
        IrBlock::Shape {
            kind,
            span,
            body,
            fill_hex,
            geometry,
            loss,
            alignment,
        } => lower_shape_block(
            &ShapeSpec {
                kind: *kind,
                span: *span,
                body,
                fill_hex: fill_hex.as_deref(),
                geometry: geometry.as_ref(),
                loss: loss.as_deref(),
                alignment: *alignment,
            },
            doc,
            ctx,
        ),
        // Nested bodies only — `lower_body` intercepts top-level blocks.
        IrBlock::Columns { body, span, .. } => lower_nested_columns(body, *span, doc, ctx),
    }
}

fn lower_figure_block(
    block: &IrBlock,
    doc: &mut Document,
    ctx: &mut LowerCtx,
) -> Vec<BlockElement> {
    let IrBlock::Figure {
        body,
        caption,
        caption_alignment,
        caption_par,
        caption_seq,
        kind: _,
        supplement: _,
        labels,
        caption_first,
        custom_placement,
        gap_twips,
        location_hash: _,
    } = block
    else {
        return Vec::new();
    };
    if ctx.compat.is_c1() && *custom_placement {
        ctx.degrade(
            "figure-placement",
            DegradationAction::Skipped,
            "figure placement is custom; placed as a normal block figure".to_string(),
        );
    }
    lower_figure(
        &FigureSpec {
            body,
            caption,
            caption_alignment: *caption_alignment,
            caption_par: *caption_par,
            caption_seq: caption_seq.as_ref(),
            labels,
            caption_first: *caption_first,
            gap_twips: *gap_twips,
        },
        doc,
        ctx,
    )
}

/// OOXML sections cannot scope inside a table cell, figure, quote, … — a
/// nested `#columns` block keeps its content in single-column flow.
fn lower_nested_columns(
    body: &[IrBlock],
    span: Option<typst_syntax::Span>,
    doc: &mut Document,
    ctx: &mut LowerCtx,
) -> Vec<BlockElement> {
    ctx.degrade_at(
        span,
        "column-block",
        DegradationAction::Skipped,
        "a columns block nested inside another element is not representable; \
         its content is kept single-column"
            .to_string(),
    );
    let mut out = Vec::new();
    for inner in body {
        out.extend(lower_block(inner, doc, ctx));
    }
    out
}

struct ShapeSpec<'a> {
    kind: IrShapeKind,
    span: Option<typst_syntax::Span>,
    body: &'a [IrBlock],
    fill_hex: Option<&'a str>,
    geometry: Option<&'a IrShapeGeometry>,
    loss: Option<&'a str>,
    alignment: Option<Alignment>,
}

/// A bare shape at C2/C3: resolved geometry becomes a `wp:inline` +
/// `wps:wsp` drawing whose `wps:txbx` carries the lowered body, in a
/// carrier paragraph that takes the shape's alignment. An unresolved or
/// partially-lost shape reports `loss` as `shape` Approximated and keeps
/// the shaded-body fallback; C1 reports Skipped and keeps the body, both
/// unchanged contracts.
fn lower_shape_block(
    spec: &ShapeSpec<'_>,
    doc: &mut Document,
    ctx: &mut LowerCtx,
) -> Vec<BlockElement> {
    if ctx.compat.is_c1() {
        ctx.degrade_at(
            spec.span,
            "shape",
            DegradationAction::Skipped,
            format!("{} dropped", spec.kind),
        );
    } else if let Some(loss) = spec.loss {
        ctx.degrade_at(
            spec.span,
            "shape",
            DegradationAction::Approximated,
            format!("{}: {loss}", spec.kind),
        );
    }
    if !ctx.compat.is_c1()
        && let Some(geometry) = spec.geometry
    {
        let mut blocks = Vec::new();
        for inner in spec.body {
            blocks.extend(lower_block(inner, doc, ctx));
        }
        let mut para = Paragraph::new();
        para.alignment = spec.alignment;
        para.inlines.push(InlineElement::Shape(InlineShape {
            width_emu: geometry.width_emu,
            height_emu: geometry.height_emu,
            outline: geometry.outline.clone(),
            fill_hex: spec.fill_hex.map(str::to_string),
            stroke: geometry.stroke.clone(),
            inset_emu: geometry.inset_emu,
            blocks,
        }));
        return vec![BlockElement::Paragraph(para)];
    }
    let mut out = Vec::new();
    for inner in spec.body {
        out.extend(lower_block(inner, doc, ctx));
    }
    if !ctx.compat.is_c1()
        && let Some(hex) = spec.fill_hex
    {
        shade_shape_block(&mut out, hex);
    }
    out
}

#[allow(
    clippy::too_many_arguments,
    reason = "one outline block's fields; the match arm passes them through"
)]
fn lower_outline(
    title: &[IrBlock],
    entries: &[IrOutlineEntry],
    depth: u8,
    custom_target: bool,
    indent_twips_per_level: u32,
    caption_id: Option<&str>,
    doc: &mut Document,
    ctx: &mut LowerCtx,
) -> Vec<BlockElement> {
    if ctx.compat.is_c1() && custom_target && entries.is_empty() {
        ctx.degrade(
            "outline-target",
            DegradationAction::Skipped,
            "outline target is not headings; entries are dropped and only the title is kept"
                .to_string(),
        );
    }
    // A heading TOC field would list targets the source did not ask for;
    // with no resolvable entries only the outline's own title survives.
    if custom_target && entries.is_empty() {
        return title
            .iter()
            .flat_map(|block| lower_block_in(block, doc, ctx, None))
            .collect();
    }
    let mut out = Vec::new();
    for block in title {
        out.extend(lower_block_in(block, doc, ctx, None));
    }
    if !entries.is_empty() {
        let mut entry_paras = Vec::with_capacity(entries.len());
        for entry in entries {
            let mut para = Paragraph::new();
            para.alignment = entry.alignment;
            let indent =
                indent_twips_per_level.saturating_mul(u32::from(entry.level.saturating_sub(1)));
            if indent > 0 {
                para.left_indent = Some(indent);
            }
            let mut body: Vec<InlineElement> = lower_inlines(&entry.title, ctx);
            body.push(InlineElement::Tab);
            if let Some(target) = &entry.target {
                body.push(InlineElement::FieldPageRef {
                    bookmark_name: target.clone(),
                    display_text: entry.page_number.clone().unwrap_or_default(),
                });
                para.inlines = vec![InlineElement::InternalLink {
                    anchor: target.clone(),
                    body,
                }];
            } else {
                para.inlines = body;
            }
            entry_paras.push(para);
        }
        out.push(BlockElement::TocBlock {
            max_depth: depth,
            caption_id: caption_id.map(str::to_string),
            paragraphs: entry_paras,
        });
        return out;
    }
    let mut para = Paragraph::new();
    para.add_toc(depth, caption_id.map(str::to_string));
    out.push(BlockElement::Paragraph(para));
    out
}

fn lower_heading(
    spec: &HeadingSpec<'_>,
    doc: &mut Document,
    ctx: &mut LowerCtx,
) -> Vec<BlockElement> {
    let mut para = Paragraph::new();
    para.style = Some(ParagraphStyle::Heading(spec.level));
    para.alignment = spec.alignment;
    para.toc_exclude = spec.toc_exclude;
    apply_par_overrides(&mut para, spec.par, ctx);
    let num_id = match spec.numbering {
        Some(IrHeadingNumbering::Mappable(pattern))
            if !ctx.compat.is_c1() && spec.numbering_text.is_some() =>
        {
            let levels = heading_level_overrides(pattern);
            let (num_id, _) = doc.find_or_allocate_heading_numbering(&levels);
            let rendered = spec.numbering_text.map_or("", String::as_str);
            check_heading_number(ctx, num_id, pattern, spec.level, rendered, spec.span)
                .then_some(num_id)
        }
        Some(IrHeadingNumbering::Unmappable) => {
            if !ctx.unmappable_heading_reported {
                ctx.unmappable_heading_reported = true;
                ctx.degrade_at(
                    spec.span,
                    "heading-numbering",
                    DegradationAction::Literalized,
                    "heading numbering is a function or a numeral system without an OOXML \
                     mapping; the number is kept as literal text"
                        .to_string(),
                );
            }
            None
        }
        _ => None,
    };
    if let Some(num_id) = num_id {
        para.num_pr = Some(ListInfo {
            level: u32::from(spec.level).saturating_sub(1),
            id: num_id,
        });
    } else if let Some(number) = spec.numbering_text {
        para.push_run(Run::new(format!("{number} ")));
    }
    para.inlines.extend(lower_inlines(spec.body, ctx));
    let idx = usize::from(spec.level).saturating_sub(1).min(4);
    let style_size = ctx.heading_sizes[idx];
    for inline in &mut para.inlines {
        if let InlineElement::Text(run) = inline {
            strip_redundant_heading_run(run, style_size);
        }
    }
    for name in spec.labels {
        let id = ctx.next_bookmark();
        para.add_bookmark_wrapping(id, name.clone());
    }
    vec![BlockElement::Paragraph(para)]
}

fn strip_redundant_heading_run(run: &mut Run, style_size: u32) {
    run.bold = false;
    if run.size_half_pt == Some(style_size) {
        run.size_half_pt = None;
    }
}

/// Build the full `w:lvl` set (ilvl 0..=8) for a heading numbering pattern.
fn heading_level_overrides(pattern: &IrNumberingPattern) -> Vec<LevelOverride> {
    (0..9)
        .filter_map(|ilvl| {
            let def = numbering_level_def(pattern, ilvl)?;
            Some(LevelOverride {
                ilvl,
                num_fmt: num_fmt_static(&def.num_fmt),
                lvl_text: def.lvl_text,
            })
        })
        .collect()
}

/// Advance the simulated counters for one heading and compare the number
/// Word will render against the one Typst already rendered. Returns `true`
/// when `w:numPr` reproduces the evaluated number exactly. A divergence — a
/// source `#counter(heading).update`, a numbering switched mid-document —
/// poisons the instance (a literal heading does not advance Word's counter)
/// and is reported once per numbering instance.
fn check_heading_number(
    ctx: &mut LowerCtx,
    num_id: u32,
    pattern: &IrNumberingPattern,
    level: u8,
    rendered: &str,
    span: Option<typst_syntax::Span>,
) -> bool {
    let idx = usize::from(level).saturating_sub(1).min(8);
    let state = ctx.heading_counters.entry(num_id).or_default();
    state.counters[idx] = state.counters[idx].saturating_add(1);
    for counter in state.counters.iter_mut().skip(idx + 1) {
        *counter = 0;
    }
    let expected = expected_heading_number(pattern, &state.counters, idx);
    if !state.diverged && expected == rendered {
        return true;
    }
    let first_divergence = !state.reported;
    state.reported = true;
    state.diverged = true;
    if first_divergence {
        ctx.degrade_at(
            span,
            "heading-numbering",
            DegradationAction::Approximated,
            "heading numbering moved off the sequence Word can regenerate (a counter update or \
             a numbering switch); the evaluated number is kept as literal text"
                .to_string(),
        );
    }
    false
}

/// Render the number a Word `w:numPr` would produce for the given counters,
/// following `NumberingPattern::apply`'s piece/last-piece cycling rule.
fn expected_heading_number(
    pattern: &IrNumberingPattern,
    counters: &[u32; 9],
    level_idx: usize,
) -> String {
    let pieces = &pattern.pieces;
    let mut out = String::new();
    for j in 0..=level_idx {
        let piece = &pieces[j.min(pieces.len() - 1)];
        if j < pieces.len() {
            out.push_str(&piece.prefix);
        } else {
            out.push_str(if piece.prefix.is_empty() {
                &pattern.suffix
            } else {
                &piece.prefix
            });
        }
        out.push_str(&piece.system.render(u64::from(counters[j])));
    }
    out.push_str(&pattern.suffix);
    out
}

/// The `w:instrText` for an [`IrInline::Field`].
fn field_instruction(kind: &IrField) -> String {
    match kind {
        IrField::NumPages => " NUMPAGES ".to_string(),
        IrField::NoteRef { label } => {
            format!(" NOTEREF {} \\h ", truncate_bookmark_name(label))
        }
        IrField::Date { picture } => date_instruction("DATE", picture.as_deref()),
        IrField::Time => " TIME ".to_string(),
        IrField::StyleRef { level } => format!(" STYLEREF \"Heading {level}\" "),
        IrField::DocProperty { name, picture } => {
            date_instruction(&format!("DOCPROPERTY {name}"), picture.as_deref())
        }
        IrField::Seq { identifier, system } => seq_instruction(identifier, *system),
    }
}

/// ` FIELD \@ "picture" ` for a date/time field — the picture is already a
/// Word date/time picture; a bare field is emitted when it is absent.
fn date_instruction(code: &str, picture: Option<&str>) -> String {
    match picture {
        Some(picture) => format!(" {code} \\@ \"{picture}\" "),
        None => format!(" {code} "),
    }
}

fn seq_instruction(identifier: &str, system: crate::types::IrNumeralSystem) -> String {
    match system.seq_switch() {
        Some(switch) => format!(" SEQ {identifier} \\* {switch} "),
        None => format!(" SEQ {identifier} "),
    }
}

fn lower_equation(eq: &IrEquation, ctx: &mut LowerCtx) -> Vec<BlockElement> {
    let mut para = Paragraph::new();
    para.alignment = Some(Alignment::Center);
    let equation_number = eq.number.as_ref().map(|number| {
        if ctx.compat.is_c3()
            && let Some(seq) = &number.seq
        {
            return EquationNumber::Seq {
                prefix: seq.prefix.clone(),
                instruction: seq_instruction(&seq.identifier, seq.system),
                display: seq.numeral.clone(),
                suffix: seq.suffix.clone(),
            };
        }
        EquationNumber::Text(number.display.clone())
    });
    para.inlines.push(InlineElement::Math {
        omml_xml: eq.omml.clone(),
        equation_number,
    });
    for name in &eq.bookmarks {
        let id = ctx.next_bookmark();
        para.add_bookmark_wrapping(id, name.clone());
    }
    vec![BlockElement::Paragraph(para)]
}

fn lower_quote(body: &[IrBlock], doc: &mut Document, ctx: &mut LowerCtx) -> Vec<BlockElement> {
    let indent_twips = ctx.body_size_half_pt * 10;
    let mut out = Vec::new();
    for inner in body {
        for mut element in lower_block(inner, doc, ctx) {
            if let BlockElement::Paragraph(para) = &mut element {
                para.left_indent = Some(indent_twips);
                para.suppress_indent = true;
            }
            out.push(element);
        }
    }
    out
}

fn lower_raw(raw: &IrRaw) -> Vec<BlockElement> {
    let mut out = Vec::new();
    if raw.lines.is_empty() {
        let mut para = Paragraph::new();
        para.code_block = true;
        out.push(BlockElement::Paragraph(para));
    } else {
        for line in &raw.lines {
            let mut para = Paragraph::new();
            para.code_block = true;
            for run in line {
                para.inlines.push(InlineElement::Text(run.clone()));
            }
            out.push(BlockElement::Paragraph(para));
        }
    }
    out
}

fn lower_bibliography(
    title: &[IrBlock],
    entries: &[IrBibEntry],
    doc: &mut Document,
    ctx: &mut LowerCtx,
) -> Vec<BlockElement> {
    let mut out: Vec<BlockElement> = title
        .iter()
        .flat_map(|block| lower_block_in(block, doc, ctx, None))
        .collect();
    let mut paragraphs = Vec::new();
    for entry in entries {
        let mut para = Paragraph::new();
        para.hanging_indent = Some(HangingIndent::Default);
        if let Some(marker) = &entry.marker {
            para.push_run(Run::new(format!("{marker} ")));
        }
        para.inlines.extend(lower_inlines(&entry.body, ctx));
        let id = ctx.next_bookmark();
        para.add_bookmark_at_start(id, entry.bookmark.clone());
        paragraphs.push(para);
    }
    if ctx.compat.is_c3() {
        out.push(BlockElement::BibliographyBlock { paragraphs });
    } else {
        out.extend(paragraphs.into_iter().map(BlockElement::Paragraph));
    }
    out
}

struct PlaceSpec<'a> {
    float: bool,
    wrap: IrPlaceWrap,
    clearance_emu: u64,
    anchor: Option<IrAnchor>,
    body: &'a [IrBlock],
}

fn lower_place(spec: &PlaceSpec<'_>, doc: &mut Document, ctx: &mut LowerCtx) -> Vec<BlockElement> {
    if !ctx.compat.is_c1()
        && let Some(anchor) = spec.anchor.as_ref()
    {
        if let Some(blocks) = floating_table(spec, anchor, doc, ctx) {
            return blocks;
        }
        if let Some(content) = anchored_content(
            spec.body,
            spec.float,
            anchor.backdrop_hex.as_deref(),
            doc,
            ctx,
        ) {
            let mut para = Paragraph::new();
            para.mark_size_half_pt = Some(2);
            para.spacing_before = Some(0);
            para.spacing_after = Some(0);
            // `wrap="none"`/`wrapNone` plus `a:noAutofit` let the box carry
            // the measured ink extent: Word lays the text out on one line
            // regardless, and the true extent is what Word needs to cover
            // the underlay without a page-sized invisible patch.
            let (x_emu, width_emu, height_emu) =
                (anchor.x_emu, anchor.width_emu, anchor.height_emu);
            // A placed box that sits entirely inside a side margin is a
            // margin note — anchor it to the text-area edge so it tracks the
            // margin if the layout changes.
            let right = anchor
                .x_emu
                .saturating_add(i64::try_from(anchor.width_emu).unwrap_or(i64::MAX));
            let (horz_rel, x_emu) =
                if right <= ctx.text_left_emu || anchor.x_emu >= ctx.text_right_emu {
                    (RelFrom::Margin, x_emu - ctx.text_left_emu)
                } else {
                    (RelFrom::Page, x_emu)
                };
            para.inlines
                .push(InlineElement::AnchoredFloat(AnchoredFloat {
                    x_emu,
                    horz_rel,
                    y_emu: anchor.y_emu,
                    width_emu,
                    height_emu,
                    clearance_emu: if spec.float { spec.clearance_emu } else { 0 },
                    z_order: {
                        ctx.next_anchor_z += 1;
                        ctx.next_anchor_z
                    },
                    wrap: if spec.float {
                        match spec.wrap {
                            IrPlaceWrap::TopAndBottom => AnchorWrap::TopAndBottom,
                            IrPlaceWrap::Square => AnchorWrap::Square,
                        }
                    } else {
                        // A non-floating `#place` does not displace flow text;
                        // paint order (behindDoc) carries the layering instead.
                        AnchorWrap::None
                    },
                    behind_doc: !spec.float && anchor.behind_doc,
                    content,
                }));
            return vec![BlockElement::Paragraph(para)];
        }
    }
    let mut out = Vec::new();
    for inner in spec.body {
        out.extend(lower_block(inner, doc, ctx));
    }
    let position = if spec.float { "floating" } else { "placed" };
    ctx.degrade(
        "placed-content",
        DegradationAction::Approximated,
        format!("{position} content is kept as normal paragraphs; its position is not preserved"),
    );
    out
}

/// A measured `#place`/`figure(placement:)` whose body is a lone `table`
/// lowers to a `w:tblpPr` floating table — the table stays a real Word table
/// (editable cells, not a text box) and keeps the measured page position. A
/// figure caption becomes a borderless grid-spanning row inside the table so
/// it travels with the float.
fn floating_table(
    spec: &PlaceSpec<'_>,
    anchor: &IrAnchor,
    doc: &mut Document,
    ctx: &mut LowerCtx,
) -> Option<Vec<BlockElement>> {
    let figure = match spec.body {
        [IrBlock::Table(table)] => {
            let mut out = lower_table(table, doc, ctx);
            out.float = Some(table_float(spec, anchor));
            return Some(vec![BlockElement::Table(out)]);
        }
        [
            IrBlock::Figure {
                body,
                caption,
                caption_alignment,
                caption_par,
                caption_seq,
                labels,
                caption_first,
                gap_twips,
                ..
            },
        ] => FigureSpec {
            body,
            caption,
            caption_alignment: *caption_alignment,
            caption_par: *caption_par,
            caption_seq: caption_seq.as_ref(),
            labels,
            caption_first: *caption_first,
            gap_twips: *gap_twips,
        },
        _ => return None,
    };
    let [IrBlock::Table(table)] = figure.body else {
        return None;
    };
    let mut out = lower_table(table, doc, ctx);
    out.float = Some(table_float(spec, anchor));
    if let Some(caption) = figure_caption_para(&figure, ctx) {
        let cols = out
            .rows
            .first()
            .map_or(1, |row| {
                row.cells.iter().map(|cell| cell.colspan).sum::<u32>()
            })
            .max(1);
        let row = TableRow {
            cells: vec![TableCell {
                paragraphs: vec![caption],
                content: Vec::new(),
                colspan: cols,
                vmerge: VMerge::None,
                width_pct: Some(5000),
                vertical_align: None,
                shading: None,
                width_twips: None,
                borders: Some(CellBorders::default()),
                zero_margins: false,
            }],
            height_twips: None,
            is_header: false,
        };
        if figure.caption_first {
            out.rows.insert(0, row);
        } else {
            out.rows.push(row);
        }
    }
    Some(vec![BlockElement::Table(out)])
}

fn table_float(spec: &PlaceSpec<'_>, anchor: &IrAnchor) -> TableFloat {
    TableFloat {
        x_twips: i32::try_from(anchor.x_emu / 635).unwrap_or(i32::MAX),
        y_twips: i32::try_from(anchor.y_emu / 635).unwrap_or(i32::MAX),
        distance_twips: u32::try_from(spec.clearance_emu / 635).unwrap_or(u32::MAX),
    }
}

fn anchored_content(
    body: &[IrBlock],
    float: bool,
    backdrop_hex: Option<&str>,
    doc: &mut Document,
    ctx: &mut LowerCtx,
) -> Option<AnchoredContent> {
    if let [IrBlock::Paragraph(para)] = body
        && let [IrInline::Image(image)] = para.inlines.as_slice()
        && let InlineElement::Image(data) = lower_image(image, ctx)
    {
        return Some(AnchoredContent::Image(data));
    }
    // A placed `rect(fill:)` becomes a solid-filled text box, not a shaded
    // marker line — the box itself carries the fill. Any other box borrows
    // its underlay's colour so it covers what Typst shows through — a Word
    // `wps:txbx` has no fill unless one is painted on.
    let (inner, declared_fill) = match body {
        [
            IrBlock::Shape {
                kind: IrShapeKind::Rectangle,
                body: shape_body,
                fill_hex: Some(fill),
                ..
            },
        ] => (shape_body.as_slice(), Some(fill.clone())),
        _ => (body, None),
    };
    let fill_hex =
        declared_fill.or_else(|| (!float).then(|| backdrop_hex.map(str::to_owned)).flatten());
    let mut lowered = Vec::new();
    for block in inner {
        lowered.extend(lower_block(block, doc, ctx));
    }
    if lowered.is_empty() && fill_hex.is_some() {
        // `w:txbxContent` needs at least one paragraph; an empty one at 1pt
        // keeps the box a pure fill with no visible text line.
        let mut para = Paragraph::new();
        para.mark_size_half_pt = Some(2);
        para.spacing_before = Some(0);
        para.spacing_after = Some(0);
        lowered.push(BlockElement::Paragraph(para));
    }
    if lowered.is_empty() {
        return None;
    }
    if !float {
        // A placed paragraph's justification must never inherit Normal's:
        // Word stretches `w:br`-terminated lines under `both` where Typst
        // leaves them unjustified (`linebreak(justify:)` defaults to
        // `false`). Every placed paragraph gets an explicit `w:jc`; an
        // undeclared alignment resolves to `left`.
        for element in &mut lowered {
            force_paragraph_alignment(element);
        }
    }
    Some(AnchoredContent::Text {
        blocks: lowered,
        fill_hex,
    })
}

/// Pin `w:jc` on every paragraph in a placed text box: `left` unless the
/// paragraph already declares center/right/justify.
fn force_paragraph_alignment(element: &mut BlockElement) {
    match element {
        BlockElement::Paragraph(para) => {
            para.alignment = para.alignment.or(Some(Alignment::Left));
        }
        BlockElement::Table(table) => {
            for row in &mut table.rows {
                for cell in &mut row.cells {
                    for para in &mut cell.paragraphs {
                        para.alignment = para.alignment.or(Some(Alignment::Left));
                    }
                    for item in &mut cell.content {
                        if let CellContent::Paragraph(para) = item {
                            para.alignment = para.alignment.or(Some(Alignment::Left));
                        }
                    }
                }
            }
        }
        BlockElement::BibliographyBlock { .. } | BlockElement::TocBlock { .. } => {}
    }
}

fn lower_pagebreak(weak: bool, to: Option<IrSectionBreakType>) -> Vec<BlockElement> {
    if weak {
        Vec::new()
    } else if let Some(target) = to {
        let break_type = match target {
            IrSectionBreakType::Odd => SectionBreakType::OddPage,
            IrSectionBreakType::Even => SectionBreakType::EvenPage,
        };
        let mut para = Paragraph::new();
        para.section_break = Some(SectionBreak {
            break_type,
            page_settings: None,
            title_page: false,
            page_numbering: None,
            page_number_start: None,
        });
        vec![BlockElement::Paragraph(para)]
    } else {
        let mut para = Paragraph::new();
        para.inlines.push(InlineElement::PageBreak);
        vec![BlockElement::Paragraph(para)]
    }
}

fn lower_table_block(table: &IrTable, doc: &mut Document, ctx: &mut LowerCtx) -> Vec<BlockElement> {
    vec![BlockElement::Table(lower_table(table, doc, ctx))]
}

struct FigureSpec<'a> {
    body: &'a [IrBlock],
    caption: &'a [IrInline],
    caption_alignment: Option<Alignment>,
    caption_par: IrParStyle,
    caption_seq: Option<&'a IrCaptionSeq>,
    labels: &'a [String],
    caption_first: bool,
    gap_twips: Option<u32>,
}

struct HeadingSpec<'a> {
    level: u8,
    numbering_text: Option<&'a String>,
    numbering: Option<&'a IrHeadingNumbering>,
    body: &'a [IrInline],
    labels: &'a [String],
    par: IrParStyle,
    alignment: Option<Alignment>,
    span: Option<typst_syntax::Span>,
    toc_exclude: bool,
}

fn lower_heading_block(
    block: &IrBlock,
    doc: &mut Document,
    ctx: &mut LowerCtx,
) -> Vec<BlockElement> {
    let IrBlock::Heading {
        level,
        numbering_text,
        numbering,
        body,
        labels,
        par,
        alignment,
        span,
        toc_exclude,
    } = block
    else {
        return Vec::new();
    };
    lower_heading(
        &HeadingSpec {
            level: *level,
            numbering_text: numbering_text.as_ref(),
            numbering: numbering.as_ref(),
            body,
            labels,
            par: *par,
            alignment: *alignment,
            span: *span,
            toc_exclude: *toc_exclude,
        },
        doc,
        ctx,
    )
}

fn figure_caption_para(fig: &FigureSpec<'_>, ctx: &mut LowerCtx) -> Option<Paragraph> {
    if fig.caption.is_empty() && fig.labels.is_empty() {
        return None;
    }
    let mut para = Paragraph::new();
    para.alignment = fig.caption_alignment;
    apply_par_overrides(&mut para, fig.caption_par, ctx);
    if let Some(gap) = fig.gap_twips {
        if fig.caption_first {
            para.spacing_after = Some(gap);
        } else {
            para.spacing_before = Some(gap);
        }
    }
    let mut caption = fig.caption.to_vec();
    if ctx.compat.is_c3()
        && let Some(seq) = fig.caption_seq
    {
        splice_caption_seq(&mut caption, seq);
    }
    para.inlines.extend(lower_inlines(&caption, ctx));
    for name in fig.labels {
        let id = ctx.next_bookmark();
        para.add_bookmark_wrapping(id, name.clone());
    }
    Some(para)
}

fn lower_figure(fig: &FigureSpec<'_>, doc: &mut Document, ctx: &mut LowerCtx) -> Vec<BlockElement> {
    let mut caption_para = figure_caption_para(fig, ctx);
    let mut out = Vec::new();
    if fig.caption_first
        && let Some(para) = caption_para.take()
    {
        out.push(BlockElement::Paragraph(para));
    }
    if fig.body.is_empty()
        || fig
            .body
            .iter()
            .all(|block| matches!(block, IrBlock::Paragraph(para) if para.inlines.is_empty()))
    {
        ctx.degrade(
            "figure-body-empty",
            DegradationAction::Skipped,
            "the figure body produced no content in the output; drawn/vector bodies need rasterization"
                .to_string(),
        );
    }
    for inner in fig.body {
        out.extend(lower_block(inner, doc, ctx));
    }
    if !fig.caption_first {
        if let Some(para) = caption_para.take() {
            out.push(BlockElement::Paragraph(para));
        } else {
            for name in fig.labels {
                let id = ctx.next_bookmark();
                for element in &mut out {
                    if let BlockElement::Paragraph(para) = element {
                        para.add_bookmark_wrapping(id, name.clone());
                        break;
                    }
                }
            }
        }
    }
    out
}

/// Copy a resolved `par` style onto the lowered paragraph as explicit
/// `w:ind`/`w:jc` overrides — but only where it differs from the document's
/// Normal style. A value equal to Normal inherits silently, exactly as
/// before; this keeps overrides surgical instead of pinning every paragraph.
fn apply_par_overrides(out: &mut Paragraph, par: IrParStyle, ctx: &LowerCtx) {
    if let Some(fl) = par.first_line_indent
        && (fl.twips != ctx.normal_first_line_twips || fl.chars != ctx.normal_first_line_chars)
    {
        out.first_line_indent = Some(FirstLineIndent {
            twips: fl.twips,
            chars: fl.chars,
        });
    }
    if let Some(justify) = par.justify
        && justify != ctx.normal_justify
    {
        out.justify = Some(justify);
    }
}

/// Scan the paragraph's line structure for `#linebreak`s. Returns
/// `(has_break, has_justified_break)` — a justified break tolerates the
/// inherited `w:jc="both"`, a plain one does not.
fn linebreak_state(inlines: &[IrInline]) -> (bool, bool) {
    let (mut has_break, mut has_justified) = (false, false);
    for inline in inlines {
        match inline {
            IrInline::LineBreak { justify } => {
                if *justify {
                    has_justified = true;
                } else {
                    has_break = true;
                }
            }
            IrInline::Link { body, .. }
            | IrInline::Field { body, .. }
            | IrInline::Shape { body, .. } => {
                let (b, j) = linebreak_state(body);
                has_break |= b;
                has_justified |= j;
            }
            _ => {}
        }
    }
    (has_break, has_justified)
}

fn lower_paragraph(para: &IrParagraph, ctx: &mut LowerCtx) -> Paragraph {
    let mut out = Paragraph::new();
    out.style.clone_from(&para.style);
    out.alignment = para.alignment;
    apply_par_overrides(&mut out, para.par, ctx);
    // A paragraph that ends some lines with a plain `#linebreak` must not sit
    // under an inherited `w:jc="both"`: Word stretches the line up to the
    // break. Pin `left` unless an explicit `linebreak(justify: true)` or an
    // explicit alignment already governs the line ends.
    let (has_break, has_justified) = linebreak_state(&para.inlines);
    if has_break
        && !has_justified
        && out.alignment.is_none()
        && out.justify.unwrap_or(ctx.normal_justify)
    {
        out.justify = Some(false);
    }
    out.suppress_indent = para.suppress_indent;
    out.hanging_indent = para.hanging_indent;
    out.left_indent = para.left_indent;
    out.right_indent = para.right_indent;
    out.keep_lines = para.keep_lines;
    out.keep_next = para.keep_next;
    out.code_block = para.code_block;
    out.horizontal_rule = para.horizontal_rule;
    out.tab_stops.clone_from(&para.tab_stops);
    out.spacing_before = para.spacing_before;
    out.spacing_after = para.spacing_after;
    out.keep_next = para.keep_next;
    out.inlines = merge_runs(lower_inlines(&para.inlines, ctx));
    for name in &para.bookmarks {
        let id = ctx.next_bookmark();
        out.add_bookmark_wrapping(id, name.clone());
    }
    out
}

fn lower_inlines(inlines: &[IrInline], ctx: &mut LowerCtx) -> Vec<InlineElement> {
    let mut out = Vec::new();
    for inline in inlines {
        match inline {
            IrInline::Text(run) => out.push(InlineElement::Text(lower_run(run, ctx))),
            IrInline::Link { url, anchor, body } => {
                if url.is_none() && anchor.is_none() {
                    out.extend(lower_inlines(body, ctx));
                } else {
                    let body = lower_link_body(body, ctx);
                    if let Some(url) = url {
                        out.push(InlineElement::Hyperlink {
                            url: url.clone(),
                            body,
                        });
                    } else if let Some(anchor) = anchor {
                        out.push(InlineElement::InternalLink {
                            anchor: anchor.clone(),
                            body,
                        });
                    }
                }
            }
            IrInline::Ref {
                target_label,
                display_text,
                page_ref,
                ..
            } => {
                if *page_ref {
                    out.push(InlineElement::FieldPageRef {
                        bookmark_name: target_label.clone(),
                        display_text: display_text.clone(),
                    });
                } else {
                    out.push(InlineElement::FieldRef {
                        bookmark_name: target_label.clone(),
                        display_text: display_text.clone(),
                    });
                }
            }
            IrInline::Cite {
                keys,
                rendered,
                target,
            } => out.push(lower_cite(keys, rendered, target.as_ref(), ctx)),
            IrInline::Field {
                kind, body, span, ..
            } => {
                out.extend(lower_field(kind, body, *span, ctx));
            }
            IrInline::FootnoteMarker { ordinal, rendered } => match ctx.footnote_ids.get(ordinal) {
                Some(id) => out.push(InlineElement::FootnoteRef {
                    id: *id,
                    custom_mark: ctx.footnote_marks.get(id).cloned(),
                }),
                None => out.push(InlineElement::Text(Run::new(rendered))),
            },
            IrInline::LineBreak { .. } => {
                let mut run = Run::new(String::new());
                run.line_break = true;
                out.push(InlineElement::Text(run));
            }
            IrInline::Space => out.push(InlineElement::Text(Run::new(" "))),
            IrInline::Image(image) => out.push(lower_image(image, ctx)),
            IrInline::Math { omml } => out.push(InlineElement::Math {
                omml_xml: omml.clone(),
                equation_number: None,
            }),
            IrInline::PageBreak => out.push(InlineElement::PageBreak),
            IrInline::ColumnBreak => out.push(InlineElement::ColumnBreak),
            IrInline::Tab => out.push(InlineElement::Tab),
            IrInline::PageField { display_text, .. } => {
                out.push(InlineElement::FieldPage {
                    display_text: display_text.clone(),
                    bold: false,
                    size_half_pt: None,
                });
            }
            IrInline::Shape {
                kind,
                span,
                body,
                fill_hex,
                geometry,
                loss,
            } => out.extend(lower_shape_inlines(
                *kind,
                *span,
                body,
                fill_hex.as_deref(),
                geometry.as_ref(),
                loss.as_deref(),
                ctx,
            )),
            IrInline::Overline(line) => out.push(InlineElement::AnchoredLine(line.clone())),
        }
    }
    out
}

/// Lower a `#link` body node-by-node so images, OMML math, tabs, and fields
/// stay inside the hyperlink instead of being flattened through `Vec<Run>`
/// (which silently dropped them). A nested link cannot exist inside
/// `w:hyperlink`, so its lowered content is spliced into the outer link and
/// the inner target loss is reported.
fn lower_link_body(body: &[IrInline], ctx: &mut LowerCtx) -> Vec<InlineElement> {
    let mut out = Vec::new();
    for element in lower_inlines(body, ctx) {
        match element {
            InlineElement::Hyperlink { body: inner, .. }
            | InlineElement::InternalLink { body: inner, .. } => {
                ctx.degrade_at(
                    None,
                    "link-in-link",
                    DegradationAction::Skipped,
                    "a link inside a link is not representable in OOXML; its content \
                     stays inside the outer link but the inner link's target is dropped"
                        .to_string(),
                );
                out.extend(inner);
            }
            other => out.push(other),
        }
    }
    out
}

/// An inline bare shape at C2/C3: the same `wp:inline` drawing as the
/// block form, inside the run. The `wps:txbx` body is a single paragraph
/// of the lowered inlines. Unresolved geometry keeps the shaded-run
/// fallback; `loss` reports `shape` Approximated; C1 reports Skipped.
fn lower_shape_inlines(
    kind: IrShapeKind,
    span: Option<typst_syntax::Span>,
    body: &[IrInline],
    fill_hex: Option<&str>,
    geometry: Option<&IrShapeGeometry>,
    loss: Option<&str>,
    ctx: &mut LowerCtx,
) -> Vec<InlineElement> {
    if ctx.compat.is_c1() {
        ctx.degrade_at(
            span,
            "shape",
            DegradationAction::Skipped,
            format!("{kind} dropped"),
        );
    } else if let Some(loss) = loss {
        ctx.degrade_at(
            span,
            "shape",
            DegradationAction::Approximated,
            format!("{kind}: {loss}"),
        );
    }
    let mut inner = lower_inlines(body, ctx);
    if !ctx.compat.is_c1()
        && let Some(geometry) = geometry
    {
        let blocks = if inner.is_empty() {
            Vec::new()
        } else {
            let mut para = Paragraph::new();
            para.inlines = inner;
            vec![BlockElement::Paragraph(para)]
        };
        return vec![InlineElement::Shape(InlineShape {
            width_emu: geometry.width_emu,
            height_emu: geometry.height_emu,
            outline: geometry.outline.clone(),
            fill_hex: fill_hex.map(str::to_string),
            stroke: geometry.stroke.clone(),
            inset_emu: geometry.inset_emu,
            blocks,
        })];
    }
    if !ctx.compat.is_c1()
        && let Some(hex) = fill_hex
    {
        for el in &mut inner {
            if let InlineElement::Text(run) = el {
                run.shading = Some(hex.to_string());
            }
        }
    }
    inner
}

fn lower_image(image: &IrImage, ctx: &mut LowerCtx) -> InlineElement {
    if ctx.compat.is_c1() {
        if image.vector {
            ctx.degrade_at(
                image.span,
                "vector-image",
                DegradationAction::Approximated,
                "SVG image rasterized to PNG; vector aspect dropped".to_string(),
            );
        }
        if image.non_cover_fit {
            ctx.degrade_at(
                image.span,
                "image-fit",
                DegradationAction::Skipped,
                "image fit is not cover; the fit behavior is dropped".to_string(),
            );
        }
        if image.alt.is_some() {
            ctx.degrade_at(
                image.span,
                "image-alt",
                DegradationAction::Skipped,
                "image alt text is dropped".to_string(),
            );
        }
    }
    let format = match image.mime.as_deref() {
        Some("image/jpeg") => ImageFormat::Jpeg,
        _ => ImageFormat::Png,
    };
    InlineElement::Image(ImageData {
        bytes: image.bytes.clone(),
        format,
        svg: if ctx.compat.is_c1() {
            None
        } else {
            image.svg.clone()
        },
        width_emu: image.width_emu,
        height_emu: image.height_emu,
        src_rect: image.src_rect,
        alt: if ctx.compat.is_c1() {
            None
        } else {
            image.alt.clone()
        },
    })
}

fn lower_run(run: &Run, ctx: &mut LowerCtx) -> Run {
    let mut run = run.clone();
    if run.overline {
        run.overline = false;
        // In C2/C3 a measured overline is already replaced by an
        // `IrInline::Overline` shape and this flag cleared; a still-flagged
        // run is one the paged-frame match could not cover.
        ctx.degrade_at(
            run.span,
            "overline",
            DegradationAction::Skipped,
            "overline dropped; text kept without decoration".to_string(),
        );
    }
    run
}

/// Splice the caption numeral at `seq.offset` out of `inlines` into a
/// `Field { Seq }` inline, so the C3 ` SEQ ` field wraps the numeral. The
/// offset is verified against the actual text — a mismatch (styled
/// supplement, custom show rule that reordered the caption) leaves the
/// caption literal.
fn splice_caption_seq(inlines: &mut Vec<IrInline>, seq: &IrCaptionSeq) {
    let numeral_chars: Vec<char> = seq.seq.numeral.chars().collect();
    let mut pos = 0usize;
    for (idx, inline) in inlines.iter().enumerate() {
        let IrInline::Text(run) = inline else {
            return;
        };
        let run_chars: Vec<char> = run.text.chars().collect();
        if pos > seq.offset {
            return;
        }
        let start = seq.offset - pos;
        if start + numeral_chars.len() <= run_chars.len() {
            if run_chars[start..start + numeral_chars.len()] != numeral_chars[..] {
                return;
            }
            let before: String = run_chars[..start].iter().collect();
            let after: String = run_chars[start + numeral_chars.len()..].iter().collect();
            let mut numeral_run = run.clone();
            numeral_run.text.clone_from(&seq.seq.numeral);
            let mut replacement = Vec::new();
            if !before.is_empty() {
                let mut run = run.clone();
                run.text = before;
                replacement.push(IrInline::Text(run));
            }
            replacement.push(IrInline::Field {
                kind: IrField::Seq {
                    identifier: seq.seq.identifier.clone(),
                    system: seq.seq.system,
                },
                body: vec![IrInline::Text(numeral_run)],
                span: None,
                location: None,
            });
            if !after.is_empty() {
                let mut run = run.clone();
                run.text = after;
                replacement.push(IrInline::Text(run));
            }
            inlines.splice(idx..=idx, replacement);
            return;
        }
        pos += run_chars.len();
    }
}

/// One citation marker: a `CITATION` field at C3, the internal entry link
/// or literal marker otherwise.
fn lower_cite(
    keys: &[String],
    rendered: &str,
    target: Option<&String>,
    ctx: &LowerCtx,
) -> InlineElement {
    if ctx.compat.is_c3() {
        return InlineElement::Citation {
            keys: keys.to_vec(),
            display_text: rendered.to_owned(),
            anchor: target.cloned(),
        };
    }
    if let Some(anchor) = target {
        return InlineElement::InternalLink {
            anchor: anchor.clone(),
            body: vec![InlineElement::Text(Run::new(rendered))],
        };
    }
    InlineElement::Text(Run::new(rendered))
}

/// Lower one `IrInline::Field`: at C3 the field code wraps the cached body
/// runs; otherwise the wrapper unwraps to the same literal (a `NOTEREF`
/// whose label never landed on a `FootnoteElem` — it has no bookmark to
/// target — also stays literal). Only the two constructs that reported at
/// C2 before the field layer keep their degradation.
fn lower_field(
    kind: &IrField,
    body: &[IrInline],
    span: Option<typst_syntax::Span>,
    ctx: &mut LowerCtx,
) -> Vec<InlineElement> {
    let emit = ctx.compat.is_c3()
        && field_body_is_literal(body)
        && match kind {
            IrField::NoteRef { label } => ctx.footnote_labels.contains(label),
            _ => true,
        };
    if emit {
        return vec![InlineElement::Field {
            instruction: field_instruction(kind),
            result: inline_to_runs(body, ctx),
        }];
    }
    match kind {
        IrField::NumPages => ctx.degrade_at(
            span,
            "page-counter-value",
            DegradationAction::Literalized,
            "page-counter value Word cannot recompute (e.g. arithmetic around counter(page)) \
             kept as literal text"
                .to_string(),
        ),
        IrField::NoteRef { .. } => ctx.degrade_at(
            span,
            "footnote-counter-reference",
            DegradationAction::Literalized,
            "footnote-counter reference kept as literal text (NOTEREF is a C3 field)".to_string(),
        ),
        IrField::Date { .. }
        | IrField::Time
        | IrField::StyleRef { .. }
        | IrField::DocProperty { .. }
        | IrField::Seq { .. } => {}
    }
    lower_inlines(body, ctx)
}

/// Whether a field's cached body converts to runs losslessly — content
/// [`inline_to_runs`] cannot carry (an image, an equation, a tab) would
/// silently vanish from the cached result, so the field stays literal.
fn field_body_is_literal(inlines: &[IrInline]) -> bool {
    inlines.iter().all(|inline| match inline {
        IrInline::Image(_)
        | IrInline::Math { .. }
        | IrInline::PageBreak
        | IrInline::ColumnBreak
        | IrInline::FootnoteMarker { .. }
        | IrInline::Overline(_)
        | IrInline::Tab => false,
        IrInline::Link { body, .. }
        | IrInline::Shape { body, .. }
        | IrInline::Field { body, .. } => field_body_is_literal(body),
        _ => true,
    })
}

fn inline_to_runs(inlines: &[IrInline], ctx: &mut LowerCtx) -> Vec<Run> {
    let mut runs = Vec::new();
    for inline in inlines {
        match inline {
            IrInline::Text(run) => runs.push(lower_run(run, ctx)),
            IrInline::Ref { display_text, .. }
            | IrInline::Cite {
                rendered: display_text,
                ..
            }
            | IrInline::PageField { display_text, .. } => {
                runs.push(Run::new(display_text));
            }
            IrInline::FootnoteMarker { rendered, .. } => runs.push(Run::new(rendered)),
            IrInline::LineBreak { .. } => {
                let mut run = Run::new(String::new());
                run.line_break = true;
                runs.push(run);
            }
            IrInline::Space => runs.push(Run::new(" ")),
            IrInline::Link { body, .. }
            | IrInline::Shape { body, .. }
            | IrInline::Field { body, .. } => {
                runs.extend(inline_to_runs(body, ctx));
            }
            IrInline::Overline(_) => {
                ctx.degrade(
                    "overline",
                    DegradationAction::Skipped,
                    "measured overline reached a flattened text context; line dropped".to_string(),
                );
            }
            IrInline::Image(_)
            | IrInline::Math { .. }
            | IrInline::PageBreak
            | IrInline::ColumnBreak
            | IrInline::Tab => {}
        }
    }
    runs
}

fn lower_table(table: &IrTable, doc: &mut Document, ctx: &mut LowerCtx) -> Table {
    if ctx.compat.is_c1() {
        let filled = table
            .rows
            .iter()
            .flat_map(|row| &row.cells)
            .filter(|cell| cell.fill_hex.is_some())
            .count();
        if filled > 0 {
            ctx.degrade(
                "table-cell-fill",
                DegradationAction::Skipped,
                format!("{filled} table cells had fills; cell fills are not emitted"),
            );
        }
        if table.gutter {
            ctx.degrade(
                "table-gutter",
                DegradationAction::Skipped,
                "table gutter is not representable; cells are placed adjacently".to_string(),
            );
        }
    }
    let rows = table
        .rows
        .iter()
        .map(|row| lower_row(row, table.column_widths_pct.as_ref(), doc, ctx))
        .collect();
    Table {
        rows,
        width_pct: Some(5000),
        border_size: None,
        borders: table.borders.clone(),
        cell_margins: table.cell_margins,
        width_twips: None,
        col_widths_twips: None,
        alignment: table.alignment,
        float: None,
    }
}

fn lower_grid_flat(grid: &IrGrid, doc: &mut Document, ctx: &mut LowerCtx) -> Vec<BlockElement> {
    let mut out = Vec::new();
    for row in &grid.rows {
        for cell in &row.cells {
            for block in &cell.body {
                out.extend(lower_block(block, doc, ctx));
            }
        }
    }
    out
}

fn lower_grid(grid: &IrGrid, doc: &mut Document, ctx: &mut LowerCtx) -> Vec<BlockElement> {
    let content_width = ctx.content_width_twips;
    let plan = plan_grid(grid, content_width);
    if plan.changed {
        if plan.tables.len() > 1 {
            ctx.degrade(
                "grid-column-split",
                DegradationAction::Approximated,
                format!(
                    "a grid wider than {MAX_COLUMNS} columns was split into {} tables",
                    plan.tables.len()
                ),
            );
        }
        return plan
            .tables
            .iter()
            .map(|table| BlockElement::Table(lower_planned_table(table, grid, doc, ctx)))
            .collect();
    }
    let rows = grid
        .rows
        .iter()
        .filter(|row| !row.cells.is_empty())
        .map(|row| {
            let mut lowered = lower_row(row, grid.column_widths_pct.as_ref(), doc, ctx);
            // Measured heights exist to feed the planner's merged bands; on the
            // generic path an `exact` trHeight would compress an ordinary grid
            // in Word, so they are planner input, not output.
            lowered.height_twips = None;
            lowered
        })
        .collect();
    vec![BlockElement::Table(Table {
        rows,
        width_pct: Some(5000),
        border_size: None,
        borders: grid.borders.clone(),
        cell_margins: None,
        width_twips: None,
        col_widths_twips: None,
        alignment: None,
        float: None,
    })]
}

fn lower_planned_table(
    planned: &PlannedTable,
    grid: &IrGrid,
    doc: &mut Document,
    ctx: &mut LowerCtx,
) -> Table {
    let rows = planned
        .rows
        .iter()
        .map(|row| {
            let mut cells = Vec::new();
            let mut lane = 0_usize;
            for cell in &row.cells {
                let span = usize::try_from(cell.col_span).unwrap_or(1);
                let width = planned
                    .col_widths_twips
                    .as_ref()
                    .and_then(|w| w.get(lane..lane + span).map(|slice| slice.iter().sum()));
                if cell.kind != PlannedCellKind::Filler {
                    cells.push(lower_planned_cell(cell, grid, width, doc, ctx));
                }
                lane += span;
            }
            TableRow {
                cells,
                height_twips: row.height_twips,
                is_header: row.is_header,
            }
        })
        .collect();
    Table {
        rows,
        width_pct: None,
        border_size: None,
        borders: Some(TableBorders::default()),
        cell_margins: Some(TableCellMargins {
            top: 0,
            left: 0,
            bottom: 0,
            right: 0,
        }),
        width_twips: planned.col_widths_twips.as_ref().map(|w| w.iter().sum()),
        alignment: None,
        col_widths_twips: planned.col_widths_twips.clone(),
        float: None,
    }
}

fn ooxml_cell_borders(borders: &IrCellBorders) -> CellBorders {
    let map = |b: &Option<crate::types::IrBorder>| {
        b.as_ref().map(|b| tydocx_ooxml::document::CellBorder {
            sz_eighths: b.sz_eighths,
            color_hex: b.color_hex.clone(),
            dashed: b.dashed,
        })
    };
    CellBorders {
        top: map(&borders.top),
        left: map(&borders.left),
        bottom: map(&borders.bottom),
        right: map(&borders.right),
    }
}

fn tiny_paragraph() -> Paragraph {
    let mut para = Paragraph::new();
    para.line_twips = Some(20);
    para.mark_size_half_pt = Some(2);
    para.spacing_before = Some(0);
    para.spacing_after = Some(0);
    para
}

fn lower_planned_cell(
    cell: &PlannedCell,
    grid: &IrGrid,
    width_twips: Option<u32>,
    doc: &mut Document,
    ctx: &mut LowerCtx,
) -> TableCell {
    let borders = Some(ooxml_cell_borders(&cell.borders));
    match cell.kind {
        PlannedCellKind::Real { row, index } => {
            let src = &grid.rows[row].cells[index];
            let mut out = lower_cell(src, None, doc, ctx);
            out.colspan = cell.col_span;
            out.vmerge = if cell.row_span > 1 {
                VMerge::Restart
            } else {
                VMerge::None
            };
            out.borders = borders;
            out.width_twips = width_twips;
            out
        }
        PlannedCellKind::Empty | PlannedCellKind::Filler => TableCell {
            paragraphs: vec![tiny_paragraph()],
            content: Vec::new(),
            colspan: 1,
            vmerge: VMerge::None,
            width_pct: None,
            vertical_align: None,
            shading: cell.fill_hex.clone(),
            width_twips,
            borders,
            zero_margins: true,
        },
        PlannedCellKind::VContinue => TableCell {
            paragraphs: vec![tiny_paragraph()],
            content: Vec::new(),
            colspan: cell.col_span,
            vmerge: VMerge::Continue,
            width_pct: None,
            vertical_align: None,
            shading: cell.fill_hex.clone(),
            width_twips,
            borders,
            zero_margins: true,
        },
    }
}

fn lower_row(
    row: &IrTableRow,
    column_widths_pct: Option<&Vec<u32>>,
    doc: &mut Document,
    ctx: &mut LowerCtx,
) -> TableRow {
    let mut cells = Vec::new();
    let mut col = 0usize;
    for cell in &row.cells {
        let width_pct = match column_widths_pct {
            Some(widths) => {
                let span = usize::try_from(cell.colspan).unwrap_or(1).max(1);
                let pct: u32 = (col..col + span)
                    .filter_map(|i| widths.get(i).copied())
                    .sum();
                if pct == 0 { None } else { Some(pct) }
            }
            None => None,
        };
        col += usize::try_from(cell.colspan).unwrap_or(1).max(1);
        cells.push(lower_cell(cell, width_pct, doc, ctx));
    }
    TableRow {
        cells,
        height_twips: row.height_twips,
        is_header: row.is_header,
    }
}

fn lower_cell(
    cell: &IrTableCell,
    width_pct: Option<u32>,
    doc: &mut Document,
    ctx: &mut LowerCtx,
) -> TableCell {
    let mut content: Vec<CellContent> = Vec::new();
    let mut paragraphs: Vec<Paragraph> = Vec::new();
    for block in &cell.body {
        for element in lower_block(block, doc, ctx) {
            match element {
                BlockElement::Paragraph(para) => {
                    paragraphs.push(para.clone());
                    content.push(CellContent::Paragraph(para));
                }
                BlockElement::Table(table) => content.push(CellContent::Table(table)),
                BlockElement::BibliographyBlock { paragraphs: bib }
                | BlockElement::TocBlock {
                    paragraphs: bib, ..
                } => {
                    for para in bib {
                        paragraphs.push(para.clone());
                        content.push(CellContent::Paragraph(para));
                    }
                }
            }
        }
    }
    if paragraphs.is_empty() && content.is_empty() {
        let para = Paragraph::new();
        paragraphs.push(para.clone());
        content.push(CellContent::Paragraph(para));
    }
    TableCell {
        paragraphs,
        content,
        colspan: cell.colspan,
        vmerge: match cell.vmerge {
            IrVMerge::None => VMerge::None,
            IrVMerge::Restart => VMerge::Restart,
            IrVMerge::Continue => VMerge::Continue,
        },
        width_pct,
        vertical_align: cell.vertical_align,
        shading: if ctx.compat.is_c1() {
            None
        } else {
            cell.fill_hex.clone()
        },
        width_twips: None,
        // Per-cell `w:tcBorders` only where an edge was declared on the cell
        // (a `table.cell(stroke:)` or a partial hline/vline folded in at
        // collection). Cells that merely inherit the table's uniform stroke
        // keep `None` and inherit `tblBorders` — the same output as before.
        borders: cell_borders_emitted(&cell.borders, cell.borders_declared),
        zero_margins: false,
    }
}

/// Emit `w:tcBorders` for a cell only when one of its edges was declared
/// (cell stroke, or a folded partial line). Inheriting cells return `None`
/// so the table-level `tblBorders` keep painting their edges.
fn cell_borders_emitted(
    borders: &IrCellBorders,
    declared: IrCellBorderFlags,
) -> Option<CellBorders> {
    if declared.top || declared.right || declared.bottom || declared.left {
        Some(ooxml_cell_borders(borders))
    } else {
        None
    }
}

fn lower_list(
    list: &IrList,
    doc: &mut Document,
    ctx: &mut LowerCtx,
    inherited: Option<ListAlloc>,
) -> Vec<BlockElement> {
    if ctx.compat.is_c1() && list.custom_marker && list.marker_texts.is_none() {
        ctx.degrade(
            "list-marker",
            DegradationAction::Literalized,
            "custom list marker is not representable; the default bullet is used".to_string(),
        );
    }
    let (num_id, depth) = if let Some(alloc) = inherited {
        (alloc.num_id, alloc.depth + 1)
    } else {
        let mut defs: Vec<Option<IrLevelDef>> = Vec::new();
        collect_levels_from_list(list, 0, None, &mut defs);
        let overrides = c1_numbering_overrides(level_overrides(defs), ctx, "list-marker");
        let id = doc.allocate_list_id_with_overrides(false, 1, overrides);
        (id, 0)
    };
    let layout = list_layout(list.indent_twips, list.body_indent_twips);
    let mut out = Vec::new();
    for item in &list.items {
        let segment_start = out.len();
        lower_list_item(&item.body, num_id, depth, layout, doc, ctx, &mut out);
        apply_item_spacing(&mut out[segment_start..], list.spacing_twips);
    }
    out
}

fn lower_enum(
    enumer: &IrEnum,
    doc: &mut Document,
    ctx: &mut LowerCtx,
    inherited: Option<ListAlloc>,
) -> Vec<BlockElement> {
    // The literal route also covers a reversed enum the collector rendered:
    // `w:numPr` can only count forward, so the reversed `number_text` is the
    // faithful output. Nested reversed enums keep the `w:numPr` path.
    if (enumer.numbering.pieces.is_empty() || enumer.reversed)
        && inherited.is_none()
        && enumer.items.iter().all(|item| item.number_text.is_some())
    {
        return lower_enum_literal(enumer, doc, ctx);
    }
    if enumer.reversed {
        ctx.degrade(
            "enum-reversed",
            DegradationAction::Literalized,
            "reversed enum is not representable; items are numbered in order".to_string(),
        );
    }
    let (num_id, depth) = if let Some(alloc) = inherited {
        (alloc.num_id, alloc.depth + 1)
    } else {
        let mut defs: Vec<Option<IrLevelDef>> = Vec::new();
        collect_levels_from_enum(enumer, 0, None, &mut defs);
        let overrides = c1_numbering_overrides(level_overrides(defs), ctx, "enum-numbering");
        let id = doc.allocate_list_id_with_overrides(true, enumer.start, overrides);
        (id, 0)
    };
    let layout = list_layout(enumer.indent_twips, enumer.body_indent_twips);
    let mut out = Vec::new();
    for item in &enumer.items {
        let segment_start = out.len();
        lower_list_item(&item.body, num_id, depth, layout, doc, ctx, &mut out);
        apply_item_spacing(&mut out[segment_start..], enumer.spacing_twips);
    }
    out
}

/// A literal-numbered enum (function numbering, unmappable pattern): the
/// numeral is literal text. At C3 a *flat* enum's numeral is spliced into a
/// `SEQ enum-N` field — prefix/suffix stay literal, the numeral is the
/// field's cached result. Items that can't carry a SEQ (a `number:`
/// override, a numeral with no arabic digit run, a nested enum) stay
/// literal and report.
fn lower_enum_literal(
    enumer: &IrEnum,
    doc: &mut Document,
    ctx: &mut LowerCtx,
) -> Vec<BlockElement> {
    if enumer.reversed {
        ctx.degrade(
            "enum-reversed",
            DegradationAction::Literalized,
            "reversed enum is not representable; items are numbered in order".to_string(),
        );
    }
    let nested = enumer.depth > 0;
    let seq = ctx.compat.is_c3() && !enumer.reversed && !nested;
    let seq_id = seq.then(|| {
        ctx.next_enum_seq += 1;
        format!("enum-{}", ctx.next_enum_seq)
    });
    if nested && ctx.compat.is_c3() {
        ctx.degrade(
            "enum-seq-nested",
            DegradationAction::Approximated,
            "a nested literal-numbered enum cannot be expressed by a SEQ field; \
             the numerals stay literal"
                .to_string(),
        );
    }
    // `expected` is the numeral the next SEQ should display: a plain
    // `{ SEQ id }` shows the previous SEQ + 1. `None` means the sequence is
    // broken (a literal item) and the next SEQ must reset with `\r` to its
    // own numeral. `emitted` tracks the first SEQ — a `start` other than 1
    // needs `\r` there too.
    let mut expected = Some(u64::from(enumer.start));
    let mut emitted = false;
    let mut out = Vec::new();
    for item in &enumer.items {
        let mut item_blocks = Vec::new();
        for block in &item.body {
            item_blocks.extend(lower_block(block, doc, ctx));
        }
        if let Some(number) = &item.number_text {
            let mut inlines = enum_number_inlines(number);
            let mut reason = None;
            let mut fielded = false;
            if let (Some(id), Some((prefix, numeral, suffix))) =
                (seq_id.as_ref(), enum_seq_split(number))
            {
                if item.number_override {
                    reason = Some("enum-seq-override");
                } else if expected.is_some_and(|e| e != numeral) {
                    reason = Some("enum-seq-numeral");
                } else {
                    let reset = if expected.is_none() || (!emitted && enumer.start != 1) {
                        format!("\\r {numeral} ")
                    } else {
                        String::new()
                    };
                    inlines = Vec::new();
                    if !prefix.is_empty() {
                        inlines.push(InlineElement::Text(Run::new(prefix)));
                    }
                    inlines.push(InlineElement::Field {
                        instruction: format!(" SEQ {id} {reset}"),
                        result: vec![Run::new(numeral.to_string())],
                    });
                    if !suffix.is_empty() {
                        inlines.push(InlineElement::Text(Run::new(format!("{suffix} "))));
                    }
                    expected = Some(numeral + 1);
                    emitted = true;
                    fielded = true;
                }
            } else if seq_id.is_some() {
                reason = Some("enum-seq-numeral");
            }
            if let Some(reason) = reason {
                ctx.degrade(
                    reason,
                    DegradationAction::Approximated,
                    format!("enum numeral {number:?} stays literal"),
                );
            }
            if !fielded {
                // A literal item breaks the SEQ sequence.
                expected = None;
            }
            for element in &mut item_blocks {
                if let BlockElement::Paragraph(para) = element {
                    para.inlines.splice(0..0, inlines);
                    break;
                }
            }
        }
        out.extend(item_blocks);
    }
    out
}

/// `number_text + " "` as a literal run — the C2 form of a literal numeral.
fn enum_number_inlines(number: &str) -> Vec<InlineElement> {
    vec![InlineElement::Text(Run::new(format!("{number} ")))]
}

/// Split a rendered literal numeral (`§1.`) at its arabic digit run —
/// `Some((prefix, numeral, suffix))` when exactly one contiguous ASCII-digit
/// run exists and it re-renders a `u64` identically (no leading zeros, which
/// a SEQ field could not reproduce). Anything else can't be a SEQ numeral.
fn enum_seq_split(number: &str) -> Option<(String, u64, String)> {
    let start = number.find(|c: char| c.is_ascii_digit())?;
    let rest = &number[start..];
    let len = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    let numeral: u64 = rest[..len].parse().ok()?;
    if numeral.to_string() != rest[..len]
        || number[start + len..].contains(|c: char| c.is_ascii_digit())
    {
        return None;
    }
    Some((
        number[..start].to_string(),
        numeral,
        number[start + len..].to_string(),
    ))
}

/// Typst `list(indent:, body-indent:)` → `(w:left, w:hanging)`: the body text
/// sits at `indent + body_indent`; the marker hangs `body_indent` to its left.
fn list_layout(indent_twips: Option<u32>, body_indent_twips: Option<u32>) -> Option<(u32, u32)> {
    match (indent_twips, body_indent_twips) {
        (Some(indent), Some(body)) => Some((indent.saturating_add(body), body)),
        _ => None,
    }
}

/// `list(spacing:)`/`enum(spacing:)` → `w:spacing w:after` on the item's last
/// paragraph.
fn apply_item_spacing(item_blocks: &mut [BlockElement], spacing_twips: Option<u32>) {
    let Some(spacing) = spacing_twips else { return };
    for element in item_blocks.iter_mut().rev() {
        if let BlockElement::Paragraph(para) = element {
            para.spacing_after = Some(spacing);
            return;
        }
    }
}

fn lower_list_item(
    body: &[IrBlock],
    num_id: u32,
    depth: u32,
    layout: Option<(u32, u32)>,
    doc: &mut Document,
    ctx: &mut LowerCtx,
    out: &mut Vec<BlockElement>,
) {
    let inherited = Some(ListAlloc { num_id, depth });
    let mut first_para = true;
    for block in body {
        let is_list = matches!(block, IrBlock::List(_) | IrBlock::Enum(_));
        for mut element in lower_block_in(block, doc, ctx, if is_list { inherited } else { None }) {
            if first_para && let BlockElement::Paragraph(para) = &mut element {
                para.list_info = Some(ListInfo {
                    level: depth,
                    id: num_id,
                });
                if let Some((left, hanging)) = layout {
                    para.list_left_twips = Some(left);
                    para.list_hanging_twips = Some(hanging);
                }
                first_para = false;
            }
            out.push(element);
        }
    }
}

fn level_overrides(defs: Vec<Option<IrLevelDef>>) -> Vec<LevelOverride> {
    defs.into_iter()
        .enumerate()
        .filter_map(|(level, def)| {
            let level = u32::try_from(level).ok()?;
            let def = def?;
            if is_default_level_def(&def, level) {
                return None;
            }
            Some(LevelOverride {
                ilvl: level,
                num_fmt: num_fmt_static(&def.num_fmt),
                lvl_text: def.lvl_text,
            })
        })
        .collect()
}

fn c1_numbering_overrides(
    overrides: Vec<LevelOverride>,
    ctx: &mut LowerCtx,
    construct: &'static str,
) -> Vec<LevelOverride> {
    if ctx.compat.is_c1() && !overrides.is_empty() {
        let first = overrides.first().map_or("?", |o| o.lvl_text.as_str());
        ctx.degrade(
            construct,
            DegradationAction::Literalized,
            format!("numbering pattern ({first}) fell back to the default"),
        );
        return Vec::new();
    }
    overrides
}

#[must_use]
pub fn footnote_mark_plan(rendered: &[&str], restart_each_page: bool) -> FootnoteFormat {
    if restart_each_page {
        if rendered.iter().all(|s| s.parse::<u32>().is_ok()) {
            return FootnoteFormat::Decimal;
        }
        if rendered.iter().all(|s| parse_circled_number(s).is_some()) {
            return FootnoteFormat::CircledNumber;
        }
    } else {
        if rendered.iter().enumerate().all(|(i, s)| {
            u32::try_from(i + 1).is_ok_and(|n| s.parse::<u32>().is_ok_and(|m| m == n))
        }) {
            return FootnoteFormat::Decimal;
        }
        if rendered
            .iter()
            .enumerate()
            .all(|(i, s)| u32::try_from(i + 1).is_ok_and(|n| parse_circled_number(s) == Some(n)))
        {
            return FootnoteFormat::CircledNumber;
        }
    }
    FootnoteFormat::CustomMarks
}

fn parse_circled_number(s: &str) -> Option<u32> {
    let mut chars = s.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    let value = match ch {
        '①'..='⑳' => u32::from(ch) - u32::from('①') + 1,
        '㉑'..='㉟' => u32::from(ch) - u32::from('㉑') + 21,
        '㊱'..='㊿' => u32::from(ch) - u32::from('㊱') + 36,
        _ => return None,
    };
    Some(value)
}

struct PatternState<'a> {
    pattern: &'a IrNumberingPattern,
    origin: u32,
}

fn collect_levels_from_list(
    list: &IrList,
    level: u32,
    state: Option<&PatternState<'_>>,
    defs: &mut Vec<Option<IrLevelDef>>,
) {
    if let Some(markers) = list.marker_texts.as_ref().filter(|m| !m.is_empty())
        && let Some(marker) = markers.get(usize::try_from(level).unwrap_or(0) % markers.len())
    {
        record_marker_def(
            level,
            IrLevelDef {
                num_fmt: "bullet".to_owned(),
                lvl_text: marker.clone(),
            },
            defs,
        );
    }
    record_level_def(level, state, defs);
    for item in &list.items {
        collect_levels_in_blocks(&item.body, level + 1, state, defs);
    }
}

fn collect_levels_from_enum(
    enumer: &IrEnum,
    level: u32,
    inherited: Option<&PatternState<'_>>,
    defs: &mut Vec<Option<IrLevelDef>>,
) {
    let own = PatternState {
        pattern: &enumer.numbering,
        origin: level,
    };
    let state = match inherited {
        Some(parent) if parent.pattern == &enumer.numbering => parent,
        _ => &own,
    };
    record_level_def(level, Some(state), defs);
    for item in &enumer.items {
        collect_levels_in_blocks(&item.body, level + 1, Some(state), defs);
    }
}

fn collect_levels_in_blocks(
    blocks: &[IrBlock],
    level: u32,
    state: Option<&PatternState<'_>>,
    defs: &mut Vec<Option<IrLevelDef>>,
) {
    for block in blocks {
        match block {
            IrBlock::Enum(nested) => collect_levels_from_enum(nested, level, state, defs),
            IrBlock::List(nested) => collect_levels_from_list(nested, level, state, defs),
            _ => {}
        }
    }
}

/// A literal marker (`list(marker: …)`) at this level overrides any def an
/// outer list already recorded — the nested declaration wins.
fn record_marker_def(level: u32, def: IrLevelDef, defs: &mut Vec<Option<IrLevelDef>>) {
    let idx = usize::try_from(level).unwrap_or(0);
    if idx >= defs.len() {
        defs.resize(idx + 1, None);
    }
    defs[idx] = Some(def);
}

fn record_level_def(
    level: u32,
    state: Option<&PatternState<'_>>,
    defs: &mut Vec<Option<IrLevelDef>>,
) {
    let idx = usize::try_from(level).unwrap_or(0);
    if idx >= defs.len() {
        defs.resize(idx + 1, None);
    }
    if defs[idx].is_none()
        && let Some(state) = state
        && let Some(def) = numbering_level_def(state.pattern, level.saturating_sub(state.origin))
    {
        defs[idx] = Some(def);
    }
}

#[must_use]
pub fn numbering_level_def(pattern: &IrNumberingPattern, level: u32) -> Option<IrLevelDef> {
    if pattern.pieces.is_empty() {
        return None;
    }
    let idx = (usize::try_from(level).unwrap_or(0)).min(pattern.pieces.len() - 1);
    let system = pattern.pieces[idx].system;
    let num_fmt = system.num_fmt().to_owned();
    let mut lvl_text = String::new();
    if pattern.full {
        for j in 0..=usize::try_from(level).unwrap_or(0) {
            let piece = &pattern.pieces[j.min(pattern.pieces.len() - 1)];
            // `NumberingPattern::apply` cycles the last piece for deeper
            // counters, substituting the suffix when that piece's prefix is
            // empty ("1." renders deeper levels as "1.1.", not "11.").
            if j < pattern.pieces.len() {
                lvl_text.push_str(&piece.prefix);
            } else {
                lvl_text.push_str(if piece.prefix.is_empty() {
                    &pattern.suffix
                } else {
                    &piece.prefix
                });
            }
            let _ = write!(lvl_text, "%{}", j + 1);
        }
        lvl_text.push_str(&pattern.suffix);
    } else {
        lvl_text.push_str(&pattern.pieces[0].prefix);
        let _ = write!(lvl_text, "%{}", level + 1);
        lvl_text.push_str(&pattern.suffix);
    }
    Some(IrLevelDef { num_fmt, lvl_text })
}

fn num_fmt_static(num_fmt: &str) -> &'static str {
    match num_fmt {
        "lowerLetter" => "lowerLetter",
        "upperLetter" => "upperLetter",
        "lowerRoman" => "lowerRoman",
        "upperRoman" => "upperRoman",
        "bullet" => "bullet",
        _ => "decimal",
    }
}

fn is_default_level_def(def: &IrLevelDef, level: u32) -> bool {
    def.num_fmt == "decimal" && def.lvl_text == format!("%{}.", level + 1)
}

fn same_format(a: &Run, b: &Run) -> bool {
    if a.line_break || b.line_break {
        return false;
    }
    a.format_key() == b.format_key()
}

fn merge_runs(inlines: Vec<InlineElement>) -> Vec<InlineElement> {
    let mut out: Vec<InlineElement> = Vec::new();
    for inline in inlines {
        if let (Some(InlineElement::Text(last)), InlineElement::Text(run)) =
            (out.last_mut(), &inline)
            && same_format(last, run)
        {
            last.text.push_str(&run.text);
            continue;
        }
        out.push(inline);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        DocumentStyle, IrCellBorderFlags, IrDocumentInfo, IrEnumItem, IrListItem, IrNumberingPiece,
        IrNumeralSystem, IrShapeKind, PageSettings, VerticalAlign,
    };
    use tydocx_ooxml::document::{AnchoredLine, BlockElement, InlineElement, ParagraphStyle};

    fn empty_doc() -> IrDocument {
        IrDocument {
            info: IrDocumentInfo::default(),
            page: PageSettings::default(),
            style: DocumentStyle::default(),
            body: Vec::new(),
            sections: Vec::new(),
            first_numbering: None,
            footnotes: Vec::new(),
            citation_sources: Vec::new(),
            page_furniture: Vec::new(),
        }
    }

    fn text_para(text: &str) -> IrParagraph {
        let mut para = IrParagraph::default();
        para.inlines.push(IrInline::Text(Run::new(text)));
        para
    }

    fn pattern(pieces: &[(&str, IrNumeralSystem)], suffix: &str, full: bool) -> IrNumberingPattern {
        IrNumberingPattern {
            pieces: pieces
                .iter()
                .map(|(prefix, system)| IrNumberingPiece {
                    prefix: (*prefix).to_owned(),
                    system: *system,
                })
                .collect(),
            suffix: suffix.to_owned(),
            full,
        }
    }

    #[test]
    fn enum_seq_split_accepts_only_a_single_arabic_numeral() {
        assert_eq!(
            enum_seq_split("§1."),
            Some(("§".to_owned(), 1, ".".to_owned()))
        );
        assert_eq!(
            enum_seq_split("(12)"),
            Some(("(".to_owned(), 12, ")".to_owned()))
        );
        assert_eq!(enum_seq_split("3"), Some((String::new(), 3, String::new())));
        for unexpressible in ["§①", "§I.", "1.2", "§06", "no digits"] {
            assert_eq!(
                enum_seq_split(unexpressible),
                None,
                "{unexpressible} can't be a SEQ numeral"
            );
        }
    }

    #[test]
    fn heading_number_and_bookmark() {
        let mut ir = empty_doc();
        ir.body.push(IrBlock::Heading {
            level: 1,
            numbering_text: Some("1.1".to_owned()),
            numbering: None,
            body: vec![IrInline::Text(Run::new("Lists"))],
            labels: vec!["sec-lists".to_owned()],
            par: IrParStyle::default(),
            alignment: None,
            span: None,
            toc_exclude: false,
        });
        let (doc, degradations) = lower(&ir, &GeometryInput::default(), CompatMode::C2);
        assert!(degradations.is_empty());
        assert_eq!(doc.body.elements.len(), 1);
        let BlockElement::Paragraph(para) = &doc.body.elements[0] else {
            panic!("expected a paragraph");
        };
        assert_eq!(para.style, Some(ParagraphStyle::Heading(1)));
        assert!(
            matches!(&para.inlines[0], InlineElement::Bookmark { name, .. } if name == "sec-lists")
        );
        assert!(matches!(&para.inlines[1], InlineElement::Text(run) if run.text == "1.1 "));
        assert!(matches!(&para.inlines[2], InlineElement::Text(run) if run.text == "Lists"));
        assert!(matches!(
            &para.inlines[3],
            InlineElement::BookmarkEnd { .. }
        ));
    }

    #[test]
    fn table_cell_fill_and_spans() {
        let mut ir = empty_doc();
        ir.body.push(IrBlock::Table(IrTable {
            rows: vec![IrTableRow {
                cells: vec![IrTableCell {
                    body: vec![IrBlock::Paragraph(text_para("a"))],
                    x: 0,
                    colspan: 2,
                    rowspan: 1,
                    vmerge: IrVMerge::Restart,
                    fill_hex: Some("DDEEFF".to_owned()),
                    vertical_align: Some(VerticalAlign::Center),
                    inset_twips: None,
                    borders: IrCellBorders::default(),
                    borders_declared: IrCellBorderFlags::default(),
                }],
                height_twips: None,
                is_header: false,
            }],
            column_widths_pct: Some(vec![2500, 2500]),
            borders: None,
            gutter: false,
            cell_margins: None,
            alignment: None,
        }));
        let (doc, _) = lower(&ir, &GeometryInput::default(), CompatMode::C2);
        let BlockElement::Table(table) = &doc.body.elements[0] else {
            panic!("expected a table");
        };
        let cell = &table.rows[0].cells[0];
        assert_eq!(cell.shading.as_deref(), Some("DDEEFF"));
        assert_eq!(cell.colspan, 2);
        assert_eq!(cell.vmerge, VMerge::Restart);
        assert_eq!(cell.vertical_align, Some(VerticalAlign::Center));
        assert_eq!(cell.width_pct, Some(5000));
    }

    #[test]
    fn nested_list_shares_one_numbering_instance() {
        let mut ir = empty_doc();
        ir.body.push(IrBlock::List(IrList {
            depth: 0,
            items: vec![IrListItem {
                marker: Some("•".to_owned()),
                body: vec![
                    IrBlock::Paragraph(text_para("outer")),
                    IrBlock::List(IrList {
                        depth: 1,
                        items: vec![IrListItem {
                            marker: Some("–".to_owned()),
                            body: vec![IrBlock::Paragraph(text_para("inner"))],
                        }],
                        custom_marker: false,
                        indent_twips: None,
                        body_indent_twips: None,
                        spacing_twips: None,
                        marker_texts: None,
                    }),
                ],
            }],
            custom_marker: false,
            indent_twips: None,
            body_indent_twips: None,
            spacing_twips: None,
            marker_texts: None,
        }));
        let (doc, _) = lower(&ir, &GeometryInput::default(), CompatMode::C2);
        assert_eq!(doc.list_num_instances.len(), 1);
        assert_eq!(doc.list_num_instances[0].abstract_num_id, 2);
        let paras: Vec<&Paragraph> = doc
            .body
            .elements
            .iter()
            .filter_map(|e| match e {
                BlockElement::Paragraph(p) => Some(p),
                _ => None,
            })
            .collect();
        assert_eq!(paras.len(), 2);
        assert_eq!(
            paras[0].list_info.as_ref().map(|i| (i.level, i.id)),
            Some((0, 4))
        );
        assert_eq!(
            paras[1].list_info.as_ref().map(|i| (i.level, i.id)),
            Some((1, 4))
        );
    }

    #[test]
    fn default_enum_pattern_needs_no_override() {
        let mut ir = empty_doc();
        ir.body.push(IrBlock::Enum(IrEnum {
            items: vec![IrEnumItem {
                number_text: Some("1.".to_owned()),
                number_override: false,
                body: vec![IrBlock::Paragraph(text_para("one"))],
            }],
            numbering: pattern(&[("", IrNumeralSystem::Arabic)], ".", false),
            start: 1,
            depth: 0,
            reversed: false,
            indent_twips: None,
            body_indent_twips: None,
            spacing_twips: None,
        }));
        let (doc, _) = lower(&ir, &GeometryInput::default(), CompatMode::C2);
        assert_eq!(doc.list_num_instances.len(), 1);
        assert!(doc.list_num_instances[0].level_overrides.is_empty());
    }

    #[test]
    fn multi_piece_enum_pattern_overrides_both_levels() {
        let nested = IrBlock::Enum(IrEnum {
            items: vec![IrEnumItem {
                number_text: Some("a)".to_owned()),
                number_override: false,
                body: vec![IrBlock::Paragraph(text_para("nested"))],
            }],
            numbering: pattern(
                &[
                    ("", IrNumeralSystem::Arabic),
                    (".", IrNumeralSystem::LowerLatin),
                ],
                ")",
                false,
            ),
            start: 1,
            depth: 1,
            reversed: false,
            indent_twips: None,
            body_indent_twips: None,
            spacing_twips: None,
        });
        let mut ir = empty_doc();
        ir.body.push(IrBlock::Enum(IrEnum {
            items: vec![
                IrEnumItem {
                    number_text: Some("1)".to_owned()),
                    number_override: false,
                    body: vec![IrBlock::Paragraph(text_para("one")), nested],
                },
                IrEnumItem {
                    number_text: Some("2)".to_owned()),
                    number_override: false,
                    body: vec![IrBlock::Paragraph(text_para("two"))],
                },
            ],
            numbering: pattern(
                &[
                    ("", IrNumeralSystem::Arabic),
                    (".", IrNumeralSystem::LowerLatin),
                ],
                ")",
                false,
            ),
            start: 1,
            depth: 0,
            reversed: false,
            indent_twips: None,
            body_indent_twips: None,
            spacing_twips: None,
        }));
        let (doc, _) = lower(&ir, &GeometryInput::default(), CompatMode::C2);
        let overrides = &doc.list_num_instances[0].level_overrides;
        assert_eq!(overrides.len(), 2);
        assert_eq!(overrides[0].ilvl, 0);
        assert_eq!(overrides[0].num_fmt, "decimal");
        assert_eq!(overrides[0].lvl_text, "%1)");
        assert_eq!(overrides[1].ilvl, 1);
        assert_eq!(overrides[1].num_fmt, "lowerLetter");
        assert_eq!(overrides[1].lvl_text, "%2)");
    }

    #[test]
    fn empty_pattern_without_numbers_falls_back_to_word_decimal() {
        let mut ir = empty_doc();
        ir.body.push(IrBlock::Enum(IrEnum {
            items: vec![IrEnumItem {
                number_text: None,
                number_override: false,
                body: vec![IrBlock::Paragraph(text_para("one"))],
            }],
            numbering: IrNumberingPattern {
                pieces: Vec::new(),
                suffix: String::new(),
                full: false,
            },
            start: 1,
            depth: 0,
            reversed: false,
            indent_twips: None,
            body_indent_twips: None,
            spacing_twips: None,
        }));
        let (doc, _) = lower(&ir, &GeometryInput::default(), CompatMode::C2);
        assert_eq!(doc.list_num_instances.len(), 1);
        assert!(doc.list_num_instances[0].level_overrides.is_empty());
        let BlockElement::Paragraph(para) = &doc.body.elements[0] else {
            panic!("expected a paragraph");
        };
        assert!(para.list_info.is_some());
    }

    #[test]
    fn full_numbering_includes_parent_numbers() {
        let def = numbering_level_def(
            &pattern(
                &[
                    ("", IrNumeralSystem::Arabic),
                    (".", IrNumeralSystem::LowerLatin),
                ],
                ")",
                true,
            ),
            1,
        )
        .expect("a level definition");
        assert_eq!(def.num_fmt, "lowerLetter");
        assert_eq!(def.lvl_text, "%1.%2)");
    }

    #[test]
    fn empty_pattern_has_no_level_definition() {
        let empty = IrNumberingPattern {
            pieces: Vec::new(),
            suffix: String::new(),
            full: false,
        };
        assert_eq!(numbering_level_def(&empty, 0), None);
    }

    #[test]
    fn footnote_marker_resolves_by_ordinal() {
        let mut ir = empty_doc();
        ir.footnotes.push(IrFootnote {
            ordinal: 1,
            rendered: "1".to_owned(),
            label: None,
            body: vec![text_para("note")],
        });
        let mut para = IrParagraph::default();
        para.inlines.push(IrInline::FootnoteMarker {
            ordinal: 1,
            rendered: "1".to_owned(),
        });
        ir.body.push(IrBlock::Paragraph(para));
        let (doc, _) = lower(&ir, &GeometryInput::default(), CompatMode::C2);
        assert_eq!(doc.footnotes.len(), 1);
        assert_eq!(doc.footnotes[0].id, 2);
        let BlockElement::Paragraph(para) = &doc.body.elements[0] else {
            panic!("expected a paragraph");
        };
        assert_eq!(
            para.inlines,
            vec![InlineElement::FootnoteRef {
                id: 2,
                custom_mark: None,
            }]
        );
    }

    #[test]
    fn unmatched_footnote_marker_keeps_its_text() {
        let mut ir = empty_doc();
        let mut para = IrParagraph::default();
        para.inlines.push(IrInline::FootnoteMarker {
            ordinal: 7,
            rendered: "7".to_owned(),
        });
        ir.body.push(IrBlock::Paragraph(para));
        let (doc, _) = lower(&ir, &GeometryInput::default(), CompatMode::C2);
        assert!(doc.footnotes.is_empty());
        let BlockElement::Paragraph(para) = &doc.body.elements[0] else {
            panic!("expected a paragraph");
        };
        assert!(matches!(&para.inlines[0], InlineElement::Text(run) if run.text == "7"));
    }

    #[test]
    fn placed_content_is_kept_with_a_degradation() {
        let mut ir = empty_doc();
        ir.body.push(IrBlock::Place {
            float: true,
            wrap: IrPlaceWrap::TopAndBottom,
            clearance_emu: 0,
            location_hash: None,
            anchor: None,
            body: vec![IrBlock::Paragraph(text_para("floating"))],
        });
        let (doc, degradations) = lower(&ir, &GeometryInput::default(), CompatMode::C1);
        assert_eq!(doc.body.elements.len(), 1);
        assert_eq!(degradations.len(), 1);
        assert_eq!(degradations[0].construct, "placed-content");
        assert_eq!(degradations[0].action, DegradationAction::Approximated);
    }

    #[test]
    fn weak_page_break_emits_nothing() {
        let mut ir = empty_doc();
        ir.body.push(IrBlock::PageBreak {
            weak: true,
            to: None,
        });
        let (doc, _) = lower(&ir, &GeometryInput::default(), CompatMode::C2);
        assert!(doc.body.elements.is_empty());
    }

    #[test]
    fn odd_page_break_becomes_a_section_break() {
        let mut ir = empty_doc();
        ir.body.push(IrBlock::PageBreak {
            weak: false,
            to: Some(IrSectionBreakType::Odd),
        });
        let (doc, _) = lower(&ir, &GeometryInput::default(), CompatMode::C2);
        let BlockElement::Paragraph(para) = &doc.body.elements[0] else {
            panic!("expected a paragraph");
        };
        assert_eq!(
            para.section_break.as_ref().map(|s| s.break_type),
            Some(SectionBreakType::OddPage)
        );
    }

    #[test]
    fn page_ref_and_ref_lower_to_different_fields() {
        let mut ir = empty_doc();
        let mut para = IrParagraph::default();
        para.inlines.push(IrInline::Ref {
            target_label: "tab".to_owned(),
            display_text: "Table 1".to_owned(),
            page_ref: false,
            page_target: None,
        });
        para.inlines.push(IrInline::Ref {
            target_label: "tab".to_owned(),
            display_text: "1".to_owned(),
            page_ref: true,
            page_target: None,
        });
        ir.body.push(IrBlock::Paragraph(para));
        let (doc, _) = lower(&ir, &GeometryInput::default(), CompatMode::C2);
        let BlockElement::Paragraph(para) = &doc.body.elements[0] else {
            panic!("expected a paragraph");
        };
        assert!(matches!(&para.inlines[0], InlineElement::FieldRef { .. }));
        assert!(matches!(
            &para.inlines[1],
            InlineElement::FieldPageRef { .. }
        ));
    }

    #[test]
    fn citation_becomes_an_internal_link() {
        let mut ir = empty_doc();
        let mut para = IrParagraph::default();
        para.inlines.push(IrInline::Cite {
            keys: vec!["knuth84".to_owned()],
            rendered: "[1]".to_owned(),
            target: Some("knuth84".to_owned()),
        });
        ir.body.push(IrBlock::Paragraph(para));
        let (doc, _) = lower(&ir, &GeometryInput::default(), CompatMode::C2);
        let BlockElement::Paragraph(para) = &doc.body.elements[0] else {
            panic!("expected a paragraph");
        };
        assert!(matches!(
            &para.inlines[0],
            InlineElement::InternalLink { anchor, .. } if anchor == "knuth84"
        ));
    }

    #[test]
    fn adjacent_same_format_runs_merge() {
        let mut ir = empty_doc();
        let mut para = IrParagraph::default();
        para.inlines.push(IrInline::Text(Run::new("hello ")));
        para.inlines.push(IrInline::Text(Run::new("world")));
        let mut bold = Run::new("!");
        bold.bold = true;
        para.inlines.push(IrInline::Text(bold));
        ir.body.push(IrBlock::Paragraph(para));
        let (doc, _) = lower(&ir, &GeometryInput::default(), CompatMode::C2);
        let BlockElement::Paragraph(para) = &doc.body.elements[0] else {
            panic!("expected a paragraph");
        };
        assert_eq!(para.inlines.len(), 2);
        assert!(matches!(&para.inlines[0], InlineElement::Text(run) if run.text == "hello world"));
        assert!(matches!(&para.inlines[1], InlineElement::Text(run) if run.bold));
    }

    #[test]
    fn quote_paragraphs_get_the_body_indent() {
        let mut ir = empty_doc();
        ir.style.body_size_half_pt = 22;
        ir.body.push(IrBlock::Quote {
            body: vec![IrBlock::Paragraph(text_para("quoted"))],
        });
        let (doc, _) = lower(&ir, &GeometryInput::default(), CompatMode::C2);
        let BlockElement::Paragraph(para) = &doc.body.elements[0] else {
            panic!("expected a paragraph");
        };
        assert_eq!(para.left_indent, Some(220));
        assert!(para.suppress_indent);
    }

    #[test]
    fn adjacent_line_break_runs_do_not_merge() {
        let mut ir = empty_doc();
        let mut para = IrParagraph::default();
        para.inlines.push(IrInline::LineBreak { justify: false });
        para.inlines.push(IrInline::LineBreak { justify: false });
        ir.body.push(IrBlock::Paragraph(para));
        let (doc, _) = lower(&ir, &GeometryInput::default(), CompatMode::C2);
        let BlockElement::Paragraph(para) = &doc.body.elements[0] else {
            panic!("expected a paragraph");
        };
        assert_eq!(para.inlines.len(), 2);
    }

    #[test]
    fn c1_shape_keeps_body_text_and_reports() {
        let mut ir = empty_doc();
        ir.body.push(IrBlock::Shape {
            kind: IrShapeKind::Rectangle,
            span: None,
            body: vec![IrBlock::Paragraph(text_para("inside"))],
            fill_hex: None,
            geometry: None,
            loss: None,
            alignment: None,
        });
        let (doc, degradations) = lower(&ir, &GeometryInput::default(), CompatMode::C1);
        assert!(
            degradations
                .iter()
                .any(|d| d.construct == "shape" && d.message == "rectangle dropped")
        );
        assert!(matches!(
            &doc.body.elements[0],
            BlockElement::Paragraph(para)
                if matches!(&para.inlines[0], InlineElement::Text(run) if run.text == "inside")
        ));
    }

    #[test]
    fn c1_curve_shape_reports_curve() {
        let mut ir = empty_doc();
        ir.body.push(IrBlock::Shape {
            kind: IrShapeKind::Curve,
            span: None,
            body: vec![IrBlock::Paragraph(text_para("inside"))],
            fill_hex: None,
            geometry: None,
            loss: None,
            alignment: None,
        });
        let (_, degradations) = lower(&ir, &GeometryInput::default(), CompatMode::C1);
        assert!(
            degradations
                .iter()
                .any(|d| d.construct == "shape" && d.message == "curve dropped")
        );
    }

    #[test]
    fn c2_shape_flattens_silently() {
        let mut ir = empty_doc();
        ir.body.push(IrBlock::Shape {
            kind: IrShapeKind::Rectangle,
            span: None,
            body: vec![IrBlock::Paragraph(text_para("inside"))],
            fill_hex: None,
            geometry: None,
            loss: None,
            alignment: None,
        });
        let (doc, degradations) = lower(&ir, &GeometryInput::default(), CompatMode::C2);
        assert!(degradations.is_empty());
        assert!(matches!(
            &doc.body.elements[0],
            BlockElement::Paragraph(para)
                if matches!(&para.inlines[0], InlineElement::Text(run) if run.text == "inside")
        ));
    }

    #[test]
    fn c1_overline_reports_and_drops() {
        let mut ir = empty_doc();
        let mut run = Run::new("over");
        run.overline = true;
        let mut para = IrParagraph::default();
        para.inlines.push(IrInline::Text(run));
        ir.body.push(IrBlock::Paragraph(para));
        let (doc, degradations) = lower(&ir, &GeometryInput::default(), CompatMode::C1);
        assert!(degradations.iter().any(|d| d.construct == "overline"));
        assert!(matches!(
            &doc.body.elements[0],
            BlockElement::Paragraph(para)
                if matches!(&para.inlines[0], InlineElement::Text(run) if !run.underline)
        ));
    }

    #[test]
    fn c2_unmatched_overline_reports_and_drops() {
        let mut ir = empty_doc();
        let mut run = Run::new("over");
        run.overline = true;
        let mut para = IrParagraph::default();
        para.inlines.push(IrInline::Text(run));
        ir.body.push(IrBlock::Paragraph(para));
        let (doc, degradations) = lower(&ir, &GeometryInput::default(), CompatMode::C2);
        assert!(degradations.iter().any(|d| d.construct == "overline"));
        assert!(matches!(
            &doc.body.elements[0],
            BlockElement::Paragraph(para)
                if matches!(&para.inlines[0], InlineElement::Text(run) if !run.underline)
        ));
    }

    #[test]
    fn c2_measured_overline_becomes_anchored_line() {
        let mut ir = empty_doc();
        let mut para = IrParagraph::default();
        para.inlines.push(IrInline::Overline(AnchoredLine {
            x_emu: 914_400,
            y_emu: 18_999,
            length_emu: 457_200,
            thickness_emu: 6_350,
            color: None,
        }));
        para.inlines.push(IrInline::Text(Run::new("over")));
        ir.body.push(IrBlock::Paragraph(para));
        let (doc, degradations) = lower(&ir, &GeometryInput::default(), CompatMode::C2);
        assert!(degradations.is_empty());
        assert!(matches!(
            &doc.body.elements[0],
            BlockElement::Paragraph(para)
                if matches!(&para.inlines[0], InlineElement::AnchoredLine(line) if line.y_emu == 18_999)
        ));
    }

    #[test]
    fn c1_reversed_enum_reports() {
        let mut ir = empty_doc();
        ir.body.push(IrBlock::Enum(IrEnum {
            items: vec![IrEnumItem {
                number_text: Some("1.".to_owned()),
                number_override: false,
                body: vec![IrBlock::Paragraph(text_para("item"))],
            }],
            numbering: pattern(&[("", IrNumeralSystem::Arabic)], ".", false),
            start: 1,
            depth: 0,
            reversed: true,
            indent_twips: None,
            body_indent_twips: None,
            spacing_twips: None,
        }));
        let (_, degradations) = lower(&ir, &GeometryInput::default(), CompatMode::C1);
        assert!(degradations.iter().any(|d| d.construct == "enum-reversed"));
    }

    #[test]
    fn footnote_mark_plan_decides() {
        assert_eq!(parse_circled_number("①"), Some(1));
        assert_eq!(parse_circled_number("㊿"), Some(50));
        assert_eq!(parse_circled_number("①②"), None);
        let decimal: Vec<&str> = vec!["1", "2", "3"];
        assert_eq!(footnote_mark_plan(&decimal, false), FootnoteFormat::Decimal);
        let circled: Vec<&str> = vec!["①", "②", "③"];
        assert_eq!(
            footnote_mark_plan(&circled, false),
            FootnoteFormat::CircledNumber
        );
        let shifted: Vec<&str> = vec!["①", "②", "⑦"];
        assert_eq!(
            footnote_mark_plan(&shifted, false),
            FootnoteFormat::CustomMarks
        );
        let parens: Vec<&str> = vec!["(1)"];
        assert_eq!(
            footnote_mark_plan(&parens, false),
            FootnoteFormat::CustomMarks
        );
        let restarted: Vec<&str> = vec!["1", "1", "2"];
        assert_eq!(
            footnote_mark_plan(&restarted, true),
            FootnoteFormat::Decimal
        );
        assert_eq!(
            footnote_mark_plan(&restarted, false),
            FootnoteFormat::CustomMarks
        );
        let circled_restart: Vec<&str> = vec!["①", "①", "②"];
        assert_eq!(
            footnote_mark_plan(&circled_restart, true),
            FootnoteFormat::CircledNumber
        );
        let empty: Vec<&str> = vec![];
        assert_eq!(footnote_mark_plan(&empty, false), FootnoteFormat::Decimal);
    }

    fn heading(level: u8, number: &str, numbering: Option<IrHeadingNumbering>) -> IrBlock {
        IrBlock::Heading {
            level,
            numbering_text: Some(number.to_owned()),
            numbering,
            body: vec![IrInline::Text(Run::new("Title"))],
            labels: Vec::new(),
            par: IrParStyle::default(),
            alignment: None,
            span: None,
            toc_exclude: false,
        }
    }

    fn heading_para(doc: &Document, index: usize) -> &Paragraph {
        let BlockElement::Paragraph(para) = &doc.body.elements[index] else {
            panic!("expected a paragraph");
        };
        para
    }

    #[test]
    fn mappable_heading_uses_word_numbering() {
        let mut ir = empty_doc();
        let numbering = Some(IrHeadingNumbering::Mappable(pattern(
            &[
                ("", IrNumeralSystem::Arabic),
                (".", IrNumeralSystem::Arabic),
            ],
            "",
            true,
        )));
        ir.body.push(heading(1, "1", numbering.clone()));
        ir.body.push(heading(2, "1.1", numbering));
        let (doc, degradations) = lower(&ir, &GeometryInput::default(), CompatMode::C2);
        assert!(degradations.is_empty());
        assert_eq!(doc.heading_num_instances.len(), 1);
        let num_id = doc.heading_num_instances[0].num_id;
        assert_eq!(doc.heading_num_instances[0].levels.len(), 9);
        let h1 = heading_para(&doc, 0);
        assert_eq!(
            h1.num_pr.as_ref().map(|info| (info.id, info.level)),
            Some((num_id, 0))
        );
        let h2 = heading_para(&doc, 1);
        assert_eq!(
            h2.num_pr.as_ref().map(|info| (info.id, info.level)),
            Some((num_id, 1))
        );
        assert!(
            !h2.inlines.iter().any(
                |inline| matches!(inline, InlineElement::Text(run) if run.text.contains("1.1"))
            )
        );
    }

    #[test]
    fn c1_mappable_heading_keeps_literal_number() {
        let mut ir = empty_doc();
        ir.body.push(heading(
            1,
            "1.",
            Some(IrHeadingNumbering::Mappable(pattern(
                &[("", IrNumeralSystem::Arabic)],
                ".",
                true,
            ))),
        ));
        let (doc, degradations) = lower(&ir, &GeometryInput::default(), CompatMode::C1);
        assert!(degradations.is_empty());
        assert!(doc.heading_num_instances.is_empty());
        let para = heading_para(&doc, 0);
        assert!(para.num_pr.is_none());
        assert!(matches!(
            &para.inlines[0],
            InlineElement::Text(run) if run.text == "1. "
        ));
    }

    #[test]
    fn unmappable_heading_numbering_literalizes_once() {
        let mut ir = empty_doc();
        ir.body
            .push(heading(1, "一、", Some(IrHeadingNumbering::Unmappable)));
        ir.body
            .push(heading(1, "二、", Some(IrHeadingNumbering::Unmappable)));
        let (doc, degradations) = lower(&ir, &GeometryInput::default(), CompatMode::C2);
        assert_eq!(
            degradations
                .iter()
                .filter(|d| d.construct == "heading-numbering")
                .count(),
            1
        );
        let para = heading_para(&doc, 1);
        assert!(para.num_pr.is_none());
        assert!(matches!(
            &para.inlines[0],
            InlineElement::Text(run) if run.text == "二、 "
        ));
    }

    #[test]
    fn shifted_heading_counter_reports_divergence_once() {
        let numbering = || {
            Some(IrHeadingNumbering::Mappable(pattern(
                &[("", IrNumeralSystem::Arabic)],
                ".",
                true,
            )))
        };
        let mut ir = empty_doc();
        ir.body.push(heading(1, "1.", numbering()));
        ir.body.push(heading(1, "3.", numbering()));
        ir.body.push(heading(1, "5.", numbering()));
        let (_, degradations) = lower(&ir, &GeometryInput::default(), CompatMode::C2);
        let reports: Vec<_> = degradations
            .iter()
            .filter(|d| d.construct == "heading-numbering")
            .collect();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].action, DegradationAction::Approximated);
    }

    #[test]
    fn level_def_cycles_last_piece_with_suffix_separator() {
        let pat = pattern(&[("", IrNumeralSystem::Arabic)], ".", true);
        assert_eq!(
            numbering_level_def(&pat, 1).map(|def| def.lvl_text),
            Some("%1.%2.".to_owned())
        );
        let two_piece = pattern(
            &[
                ("", IrNumeralSystem::Arabic),
                (".", IrNumeralSystem::Arabic),
            ],
            "",
            true,
        );
        assert_eq!(
            numbering_level_def(&two_piece, 2).map(|def| def.lvl_text),
            Some("%1.%2.%3".to_owned())
        );
    }

    #[test]
    fn expected_number_matches_rendered_pattern() {
        let pat = pattern(&[("", IrNumeralSystem::Arabic)], ".", true);
        assert_eq!(
            expected_heading_number(&pat, &[1, 1, 0, 0, 0, 0, 0, 0, 0], 1),
            "1.1."
        );
        let roman = pattern(
            &[
                ("Ch.", IrNumeralSystem::UpperRoman),
                (".", IrNumeralSystem::Arabic),
            ],
            ":",
            true,
        );
        assert_eq!(
            expected_heading_number(&roman, &[2, 3, 0, 0, 0, 0, 0, 0, 0], 1),
            "Ch.II.3:"
        );
    }
}
