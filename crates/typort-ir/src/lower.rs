//! Pure lowering: `IrDocument` -> `typort_ooxml::Document`.
//!
//! The lowering is a pure function of its inputs (the IR, the geometry
//! boundary, and the compat mode). It never fails: constructs without a
//! faithful OOXML encoding become `Degradation`s, never errors.
//!
//! Parity notes (against the legacy HTML-walk path, which this replaces):
//! - Lists share one numbering instance per top-level list; nested lists
//!   reuse the parent's instance at a deeper level, exactly as the legacy
//!   `ListWalker` does (first writer wins per level; levels equal to the
//!   shared abstract definition need no override).
//! - In-text citations lower to literal markers + internal hyperlinks
//!   (the L6 verdict), never to CITATION fields.
//! - Raw blocks keep their syntax colours (the legacy path dropped them);
//!   this is a deliberate, documented divergence (see the equivalence
//!   report in `docs/fallback-registry.md`).
//! - An enum whose numbering is a closure (no static pattern) lowers its
//!   items to literal number text instead of the legacy path's shared
//!   decimal fallback.

use std::collections::HashMap;
use std::fmt::Write;

use typort_ooxml::document::{
    BlockElement, CellContent, Document, ImageData, ImageFormat, InlineElement, LevelOverride,
    ListInfo, Paragraph, SectionBreak, SectionBreakType, Table, TableCell, TableRow, VMerge,
};

use crate::degradation::{Degradation, DegradationAction};
use crate::geometry::GeometryInput;
use crate::mode::CompatMode;
use crate::types::{
    Alignment, HangingIndent, IrBibEntry, IrBlock, IrDocument, IrEnum, IrEquation, IrFootnote,
    IrGrid, IrInline, IrLevelDef, IrList, IrNumberingPattern, IrNumeralSystem, IrParagraph, IrRaw,
    IrSectionBreakType, IrTable, IrTableCell, IrTableRow, IrVMerge, ParagraphStyle, Run,
};

/// Shared lowering state: footnote id resolution, bookmark ids, the
/// list instance in force, and degradations. (Bundled in a struct rather
/// than threaded as positional parameters, per the repo's rule.)
#[derive(Debug, Default)]
pub struct LowerCtx {
    /// Compat mode of this conversion.
    pub compat: CompatMode,
    /// Body size in half-points (quote indentation is `body_size * 10` twips).
    pub body_size_half_pt: u32,
    /// Per-level heading sizes in half-points (the `Heading{n}` paragraph
    /// styles' sizes, used to suppress redundant per-run heading sizes).
    pub heading_sizes: [u32; 5],
    /// Footnote ordinal -> OOXML footnote id.
    pub footnote_ids: HashMap<u32, u32>,
    /// Next bookmark id to allocate.
    pub next_bookmark_id: u32,
    /// Degradations collected during lowering.
    pub degradations: Vec<Degradation>,
}

impl LowerCtx {
    fn next_bookmark(&mut self) -> u32 {
        let id = self.next_bookmark_id;
        self.next_bookmark_id = self.next_bookmark_id.saturating_add(1);
        id
    }

    fn degrade(&mut self, construct: &'static str, action: DegradationAction, message: String) {
        self.degradations.push(Degradation {
            span: None,
            construct,
            action,
            message,
        });
    }
}

/// A numbering instance in force for nested lists: nested lists reuse
/// their parent's `num_id` at a deeper level. `depth` is the owning
/// list's level within the instance.
#[derive(Debug, Clone, Copy)]
struct ListAlloc {
    num_id: u32,
    depth: u32,
}

/// Lower an IR document to an OOXML document.
///
/// Returns the document and the degradations recorded during lowering.
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
    doc.citation_sources.clone_from(&ir.citation_sources);
    doc.header.clone_from(&geometry.header);
    doc.footer.clone_from(&geometry.footer);
    doc.page_numbering = geometry.page_numbering;

    let mut ctx = LowerCtx {
        compat,
        body_size_half_pt: ir.style.body_size_half_pt,
        heading_sizes: ir.style.heading_sizes,
        footnote_ids: HashMap::new(),
        next_bookmark_id: doc.bookmark_counter,
        degradations: Vec::new(),
    };

    // Footnotes first: body markers resolve to ids by ordinal.
    for footnote in &ir.footnotes {
        let content = lower_footnote_body(footnote, &mut ctx);
        let id = doc.add_footnote(content);
        ctx.footnote_ids.insert(footnote.ordinal, id);
    }

    for block in &ir.body {
        let lowered = lower_block(block, &mut doc, &mut ctx);
        doc.body.elements.extend(lowered);
    }

    doc.bookmark_counter = ctx.next_bookmark_id;
    let degradations = ctx.degradations;
    (doc, degradations)
}

/// Lower one footnote body: its paragraphs' inlines, concatenated into a
/// single inline list (the OOXML footnote model has no paragraphs).
fn lower_footnote_body(footnote: &IrFootnote, ctx: &mut LowerCtx) -> Vec<InlineElement> {
    let mut content = Vec::new();
    for para in &footnote.body {
        content.extend(lower_inlines(&para.inlines, ctx));
    }
    content
}

/// Lower one block, with no list instance in force.
fn lower_block(block: &IrBlock, doc: &mut Document, ctx: &mut LowerCtx) -> Vec<BlockElement> {
    lower_block_in(block, doc, ctx, None)
}

/// Lower one block, reusing a parent list's numbering instance for a
/// directly nested list.
fn lower_block_in(
    block: &IrBlock,
    doc: &mut Document,
    ctx: &mut LowerCtx,
    inherited: Option<ListAlloc>,
) -> Vec<BlockElement> {
    match block {
        IrBlock::Paragraph(para) => vec![BlockElement::Paragraph(lower_paragraph(para, ctx))],
        IrBlock::Heading {
            level,
            numbering_text,
            body,
            label,
            alignment,
        } => lower_heading(
            *level,
            numbering_text.as_ref(),
            body,
            label.as_ref(),
            *alignment,
            ctx,
        ),
        IrBlock::Table(table) => vec![BlockElement::Table(lower_table(table, doc, ctx))],
        IrBlock::Grid(grid) => vec![BlockElement::Table(lower_grid(grid, doc, ctx))],
        IrBlock::Figure {
            body,
            caption,
            caption_alignment,
            kind: _,
            supplement: _,
            label,
            caption_first,
        } => lower_figure(
            body,
            caption,
            *caption_alignment,
            label.as_ref(),
            *caption_first,
            doc,
            ctx,
        ),
        IrBlock::List(list) => lower_list(list, doc, ctx, inherited),
        IrBlock::Enum(enumer) => lower_enum(enumer, doc, ctx, inherited),
        IrBlock::Equation(eq) => lower_equation(eq, ctx),
        IrBlock::Quote { body } => lower_quote(body, doc, ctx),
        IrBlock::Raw(raw) => lower_raw(raw),
        IrBlock::Outline {
            title: _,
            entries: _,
            depth,
        } => {
            let mut para = Paragraph::new();
            para.add_toc(*depth);
            vec![BlockElement::Paragraph(para)]
        }
        IrBlock::Bibliography { entries } => lower_bibliography(entries, ctx),
        IrBlock::Place { float, body } => lower_place(*float, body, doc, ctx),
        IrBlock::PageBreak { weak, to } => lower_pagebreak(*weak, *to),
    }
}

fn lower_heading(
    level: u8,
    numbering_text: Option<&String>,
    body: &[IrInline],
    label: Option<&String>,
    alignment: Option<Alignment>,
    ctx: &mut LowerCtx,
) -> Vec<BlockElement> {
    let mut para = Paragraph::new();
    para.style = Some(ParagraphStyle::Heading(level));
    para.alignment = alignment;
    if let Some(number) = numbering_text {
        para.push_run(Run::new(format!("{number} ")));
    }
    para.inlines.extend(lower_inlines(body, ctx));
    // Drop per-run bold/size that merely restate the `Heading{n}`
    // paragraph style's own values (it always supplies `<w:b/>` and the
    // per-level size), so the runs stay implicit — mirroring the legacy
    // `suppress_redundant_heading_run_props`. Only values that EQUAL the
    // style are removed; a differing size, plus colour / italic / font /
    // script overrides, survive.
    let idx = usize::from(level).saturating_sub(1).min(4);
    let style_size = ctx.heading_sizes[idx];
    for inline in &mut para.inlines {
        if let InlineElement::Text(run) = inline {
            strip_redundant_heading_run(run, style_size);
        }
    }
    if let Some(name) = label {
        let id = ctx.next_bookmark();
        para.add_bookmark_at_start(id, name.clone());
    }
    vec![BlockElement::Paragraph(para)]
}

/// Strip a single heading run's overrides that merely duplicate the style.
///
/// `bold` is cleared (the `Heading{n}` style always supplies `<w:b/>`).
/// `size_half_pt` is cleared only when it equals `style_size`.
fn strip_redundant_heading_run(run: &mut Run, style_size: u32) {
    run.bold = false;
    if run.size_half_pt == Some(style_size) {
        run.size_half_pt = None;
    }
}

fn lower_equation(eq: &IrEquation, ctx: &mut LowerCtx) -> Vec<BlockElement> {
    let mut para = Paragraph::new();
    para.alignment = Some(Alignment::Center);
    para.inlines.push(InlineElement::Math {
        omml_xml: eq.omml.clone(),
        equation_number: eq.number.clone(),
    });
    if let Some(name) = &eq.bookmark {
        let id = ctx.next_bookmark();
        para.add_bookmark_at_start(id, name.clone());
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

fn lower_bibliography(entries: &[IrBibEntry], ctx: &mut LowerCtx) -> Vec<BlockElement> {
    let mut paragraphs = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let mut para = Paragraph::new();
        para.hanging_indent = Some(HangingIndent::Default);
        let marker = entry
            .marker
            .clone()
            .unwrap_or_else(|| format!("[{}]", index + 1));
        para.push_run(Run::new(format!("{marker} ")));
        para.inlines.extend(lower_inlines(&entry.body, ctx));
        let id = ctx.next_bookmark();
        para.add_bookmark_at_start(id, entry.bookmark.clone());
        paragraphs.push(para);
    }
    vec![BlockElement::BibliographyBlock { paragraphs }]
}

fn lower_place(
    float: bool,
    body: &[IrBlock],
    doc: &mut Document,
    ctx: &mut LowerCtx,
) -> Vec<BlockElement> {
    let mut out = Vec::new();
    for inner in body {
        out.extend(lower_block(inner, doc, ctx));
    }
    let position = if float { "floating" } else { "placed" };
    ctx.degrade(
        "placed-content",
        DegradationAction::Approximated,
        format!("{position} content is kept as normal paragraphs; its position is not preserved"),
    );
    out
}

fn lower_pagebreak(weak: bool, to: Option<IrSectionBreakType>) -> Vec<BlockElement> {
    if weak {
        // A weak break is a flow boundary, not a hard break: no output.
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
        });
        vec![BlockElement::Paragraph(para)]
    } else {
        let mut para = Paragraph::new();
        para.inlines.push(InlineElement::PageBreak);
        vec![BlockElement::Paragraph(para)]
    }
}

/// Lower a figure: body blocks plus a caption paragraph. The caption goes
/// first only when the author placed it on top.
fn lower_figure(
    body: &[IrBlock],
    caption: &[IrInline],
    caption_alignment: Option<Alignment>,
    label: Option<&String>,
    caption_first: bool,
    doc: &mut Document,
    ctx: &mut LowerCtx,
) -> Vec<BlockElement> {
    let mut caption_para: Option<Paragraph> = None;
    if !caption.is_empty() || label.is_some() {
        let mut para = Paragraph::new();
        para.alignment = caption_alignment;
        para.inlines.extend(lower_inlines(caption, ctx));
        if let Some(name) = label {
            let id = ctx.next_bookmark();
            para.add_bookmark_at_start(id, name.clone());
        }
        caption_para = Some(para);
    }
    let mut out = Vec::new();
    if caption_first && let Some(para) = caption_para.take() {
        out.push(BlockElement::Paragraph(para));
    }
    // A figure whose body carries no content in the IR (a drawn/vector
    // body: shapes and placed content have no IR form yet, so the body
    // lowers to nothing but empty paragraphs): say so instead of
    // emitting a bare empty paragraph. Rasterizing such bodies from the
    // companion Paged frame is a follow-up (see the fallback registry).
    if body.is_empty()
        || body
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
    for inner in body {
        out.extend(lower_block(inner, doc, ctx));
    }
    if !caption_first {
        if let Some(para) = caption_para.take() {
            out.push(BlockElement::Paragraph(para));
        } else if let Some(name) = label {
            // A labelled figure without a caption: anchor the bookmark on
            // its first paragraph, if it has one.
            let id = ctx.next_bookmark();
            for element in &mut out {
                if let BlockElement::Paragraph(para) = element {
                    para.add_bookmark_at_start(id, name.clone());
                    break;
                }
            }
        }
    }
    out
}

/// Lower one paragraph, merging adjacent same-format runs.
fn lower_paragraph(para: &IrParagraph, ctx: &mut LowerCtx) -> Paragraph {
    let mut out = Paragraph::new();
    out.style.clone_from(&para.style);
    out.alignment = para.alignment;
    out.suppress_indent = para.suppress_indent;
    out.hanging_indent = para.hanging_indent;
    out.left_indent = para.left_indent;
    out.code_block = para.code_block;
    out.horizontal_rule = para.horizontal_rule;
    out.tab_stops.clone_from(&para.tab_stops);
    out.spacing_before = para.spacing_before;
    out.inlines = merge_runs(lower_inlines(&para.inlines, ctx));
    if let Some(name) = &para.bookmark {
        let id = ctx.next_bookmark();
        out.add_bookmark_at_start(id, name.clone());
    }
    out
}

/// Lower inlines to OOXML inline elements.
fn lower_inlines(inlines: &[IrInline], ctx: &mut LowerCtx) -> Vec<InlineElement> {
    let mut out = Vec::new();
    for inline in inlines {
        match inline {
            IrInline::Text(run) => out.push(InlineElement::Text(run.clone())),
            IrInline::Link { url, anchor, body } => {
                let runs = inline_to_runs(body);
                if let Some(url) = url {
                    out.push(InlineElement::Hyperlink {
                        url: url.clone(),
                        runs,
                    });
                } else if let Some(anchor) = anchor {
                    out.push(InlineElement::InternalLink {
                        anchor: anchor.clone(),
                        runs,
                    });
                } else {
                    out.extend(runs.into_iter().map(InlineElement::Text));
                }
            }
            IrInline::Ref {
                target_label,
                display_text,
                page_ref,
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
                keys: _,
                rendered,
                target,
            } => {
                // L6 verdict: literal markers + internal hyperlinks, never
                // CITATION fields (C3 decides those later, D2 deferred).
                if let Some(anchor) = target {
                    out.push(InlineElement::InternalLink {
                        anchor: anchor.clone(),
                        runs: vec![Run::new(rendered)],
                    });
                } else {
                    out.push(InlineElement::Text(Run::new(rendered)));
                }
            }
            IrInline::FootnoteMarker { ordinal, rendered } => {
                if let Some(id) = ctx.footnote_ids.get(ordinal) {
                    out.push(InlineElement::FootnoteRef(*id));
                } else {
                    // A marker with no matching entry (should not happen):
                    // keep the rendered text rather than dropping it.
                    out.push(InlineElement::Text(Run::new(rendered)));
                }
            }
            IrInline::LineBreak => {
                let mut run = Run::new(String::new());
                run.line_break = true;
                out.push(InlineElement::Text(run));
            }
            IrInline::Space => out.push(InlineElement::Text(Run::new(" "))),
            IrInline::Image(image) => {
                let format = match image.mime.as_deref() {
                    Some("image/jpeg") => ImageFormat::Jpeg,
                    // The collector rasterizes SVGs to PNG before lowering,
                    // so anything else here is PNG bytes.
                    _ => ImageFormat::Png,
                };
                out.push(InlineElement::Image(ImageData {
                    bytes: image.bytes.clone(),
                    format,
                    width_emu: image.width_emu,
                    height_emu: image.height_emu,
                }));
            }
            IrInline::Math { omml } => out.push(InlineElement::Math {
                omml_xml: omml.clone(),
                equation_number: None,
            }),
            IrInline::PageBreak => out.push(InlineElement::PageBreak),
            IrInline::ColumnBreak => out.push(InlineElement::ColumnBreak),
            IrInline::Tab => out.push(InlineElement::Tab),
        }
    }
    out
}

/// Render inlines to plain runs (for hyperlink bodies): text runs pass
/// through, references and citations become their display text, and
/// constructs with no run form are skipped.
fn inline_to_runs(inlines: &[IrInline]) -> Vec<Run> {
    let mut runs = Vec::new();
    for inline in inlines {
        match inline {
            IrInline::Text(run) => runs.push(run.clone()),
            IrInline::Ref { display_text, .. }
            | IrInline::Cite {
                rendered: display_text,
                ..
            } => {
                runs.push(Run::new(display_text));
            }
            IrInline::FootnoteMarker { rendered, .. } => runs.push(Run::new(rendered)),
            IrInline::LineBreak => {
                let mut run = Run::new(String::new());
                run.line_break = true;
                runs.push(run);
            }
            IrInline::Space => runs.push(Run::new(" ")),
            IrInline::Link { body, .. } => runs.extend(inline_to_runs(body)),
            IrInline::Image(_)
            | IrInline::Math { .. }
            | IrInline::PageBreak
            | IrInline::ColumnBreak
            | IrInline::Tab => {}
        }
    }
    runs
}

/// Lower a table: rows, column widths, borders.
fn lower_table(table: &IrTable, doc: &mut Document, ctx: &mut LowerCtx) -> Table {
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
    }
}

/// Lower a grid: same shape as a table; a borderless grid passes
/// `Some(TableBorders::default())` so the writer emits no visible grid.
fn lower_grid(grid: &IrGrid, doc: &mut Document, ctx: &mut LowerCtx) -> Table {
    let rows = grid
        .rows
        .iter()
        .map(|row| lower_row(row, grid.column_widths_pct.as_ref(), doc, ctx))
        .collect();
    Table {
        rows,
        width_pct: Some(5000),
        border_size: None,
        borders: grid.borders.clone(),
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
    TableRow { cells }
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
                BlockElement::BibliographyBlock { paragraphs: bib } => {
                    for para in bib {
                        paragraphs.push(para.clone());
                        content.push(CellContent::Paragraph(para));
                    }
                }
            }
        }
    }
    // A cell with no content at all still needs one paragraph: Word
    // requires at least one `w:p` per cell, and the shared post-pass
    // suppresses the body first-line indent on cell paragraphs — so the
    // empty paragraph is a real one (not the writer's bare fallback),
    // exactly as the legacy path emits for merged-continuation cells.
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
        shading: cell.fill_hex.clone(),
    }
}

/// Lower an unordered list: one numbering instance for the whole tree,
/// nested lists reusing the parent's instance at a deeper level, exactly
/// as the legacy `ListWalker` does. Unordered lists contribute no level
/// definitions (the shared bullet abstract applies); enums nested under
/// them contribute theirs (collected at the top-level allocation).
fn lower_list(
    list: &IrList,
    doc: &mut Document,
    ctx: &mut LowerCtx,
    inherited: Option<ListAlloc>,
) -> Vec<BlockElement> {
    let (num_id, depth) = if let Some(alloc) = inherited {
        (alloc.num_id, alloc.depth + 1)
    } else {
        let mut defs: Vec<Option<IrLevelDef>> = Vec::new();
        collect_levels_from_list(list, 0, None, &mut defs);
        let overrides = level_overrides(defs);
        let id = doc.allocate_list_id_with_overrides(false, 1, overrides);
        (id, 0)
    };
    let mut out = Vec::new();
    for item in &list.items {
        lower_list_item(&item.body, num_id, depth, doc, ctx, &mut out);
    }
    out
}

/// Lower an ordered (enum) list, mirroring the legacy `ListWalker`:
/// one numbering instance per top-level list, per-level definitions
/// collected first-writer-wins over the whole subtree, levels equal to
/// the shared abstract definition needing no override.
fn lower_enum(
    enumer: &IrEnum,
    doc: &mut Document,
    ctx: &mut LowerCtx,
    inherited: Option<ListAlloc>,
) -> Vec<BlockElement> {
    // An enum with no static pattern and no parent instance: when every
    // item carries its rendered number (an unmappable-but-static pattern
    // such as Greek, computed by the collector via `pattern.apply`), the
    // items become literal number text. When some item has no rendered
    // number (a closure numbering the collector cannot evaluate), fall
    // through to the plain Word-numbered path — the legacy decimal
    // fallback. (Nested under a parent instance it inherits the instance
    // and renders per the pattern in force, as the legacy walk does.)
    if enumer.numbering.pieces.is_empty()
        && inherited.is_none()
        && enumer.items.iter().all(|item| item.number_text.is_some())
    {
        return lower_enum_literal(enumer, doc, ctx);
    }
    let (num_id, depth) = if let Some(alloc) = inherited {
        (alloc.num_id, alloc.depth + 1)
    } else {
        let mut defs: Vec<Option<IrLevelDef>> = Vec::new();
        collect_levels_from_enum(enumer, 0, None, &mut defs);
        let overrides = level_overrides(defs);
        let id = doc.allocate_list_id_with_overrides(true, enumer.start, overrides);
        (id, 0)
    };
    let mut out = Vec::new();
    for item in &enumer.items {
        lower_list_item(&item.body, num_id, depth, doc, ctx, &mut out);
    }
    out
}

/// Lower an enum whose items all carry rendered number text as literal
/// text: each item's number becomes a leading run; nested enums keep
/// their own numbering.
fn lower_enum_literal(
    enumer: &IrEnum,
    doc: &mut Document,
    ctx: &mut LowerCtx,
) -> Vec<BlockElement> {
    let mut out = Vec::new();
    for item in &enumer.items {
        let mut item_blocks = Vec::new();
        for block in &item.body {
            item_blocks.extend(lower_block(block, doc, ctx));
        }
        if let Some(number) = &item.number_text {
            for element in &mut item_blocks {
                if let BlockElement::Paragraph(para) = element {
                    para.inlines
                        .insert(0, InlineElement::Text(Run::new(format!("{number} "))));
                    break;
                }
            }
        }
        out.extend(item_blocks);
    }
    out
}

/// Lower one list/enum item's body: the first paragraph carries the
/// numbering info; nested lists reuse the parent's instance. An item
/// with no direct paragraph (only a nested list) produces no paragraph
/// of its own, matching the legacy walk.
fn lower_list_item(
    body: &[IrBlock],
    num_id: u32,
    depth: u32,
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
                first_para = false;
            }
            out.push(element);
        }
    }
}

/// Turn collected per-level definitions into `LevelOverride`s, dropping
/// levels equal to the shared abstract definition.
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

/// The numbering pattern in force at one nesting depth, with the depth at
/// which it was introduced: a nested enum with a different pattern resets
/// the piece indexing at its own depth, while an inherited pattern keeps
/// indexing from the outermost enum.
struct PatternState<'a> {
    pattern: &'a IrNumberingPattern,
    origin: u32,
}

/// Collect one level definition per nesting depth an unordered-list
/// subtree reaches, first writer wins per level. A list contributes no
/// definition itself; enums nested under it contribute theirs.
fn collect_levels_from_list(
    list: &IrList,
    level: u32,
    state: Option<&PatternState<'_>>,
    defs: &mut Vec<Option<IrLevelDef>>,
) {
    record_level_def(level, state, defs);
    for item in &list.items {
        collect_levels_in_blocks(&item.body, level + 1, state, defs);
    }
}

/// Collect one level definition per nesting depth an enum subtree
/// reaches, first writer wins per level.
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

/// Walk blocks looking for nested lists/enums, at one deeper level.
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

/// Record this level's definition from the pattern in force (first
/// writer wins: sibling sub-enums at one depth share a single OOXML
/// level).
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

/// Derive the OOXML level definition for piece `level` (0-based) of a
/// Typst numbering pattern, mirroring the legacy `numbering_level_def`:
/// deeper levels cycle the last piece; `full` selects Typst's `full`
/// rendering, where a nested label includes its parent numbers.
///
/// Returns `None` when the pattern has no pieces, or for numeral systems
/// with no OOXML equivalent (Greek, Hebrew, ...); those levels keep the
/// shared decimal definition.
#[must_use]
pub fn numbering_level_def(pattern: &IrNumberingPattern, level: u32) -> Option<IrLevelDef> {
    if pattern.pieces.is_empty() {
        return None;
    }
    let idx = (usize::try_from(level).unwrap_or(0)).min(pattern.pieces.len() - 1);
    let system = pattern.pieces[idx].system;
    let num_fmt = match system {
        IrNumeralSystem::Arabic => "decimal".to_owned(),
        IrNumeralSystem::LowerLatin => "lowerLetter".to_owned(),
        IrNumeralSystem::UpperLatin => "upperLetter".to_owned(),
        IrNumeralSystem::LowerRoman => "lowerRoman".to_owned(),
        IrNumeralSystem::UpperRoman => "upperRoman".to_owned(),
    };
    let mut lvl_text = String::new();
    if pattern.full {
        for j in 0..=usize::try_from(level).unwrap_or(0) {
            let piece = &pattern.pieces[j.min(pattern.pieces.len() - 1)];
            lvl_text.push_str(&piece.prefix);
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

/// Map a `num_fmt` string to the static string the writer's
/// `LevelOverride` requires; unknown values fall back to decimal.
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

/// Whether a level definition equals the shared abstract definition for
/// that level (decimal `"%{n}."`), i.e. no `w:lvlOverride` is needed.
fn is_default_level_def(def: &IrLevelDef, level: u32) -> bool {
    def.num_fmt == "decimal" && def.lvl_text == format!("%{}.", level + 1)
}

/// Whether two runs have identical formatting (all fields except text
/// and span), so adjacent runs may be merged.
///
/// A line-break run never merges with anything: the writer emits one
/// `<w:br/>` per such run, so merging two adjacent breaks would delete a
/// forced blank line the author wrote (`#linebreak()#linebreak()`).
/// Mirrors `convert::coalesce::same_run_formatting`.
fn same_format(a: &Run, b: &Run) -> bool {
    if a.line_break || b.line_break {
        return false;
    }
    a.bold == b.bold
        && a.italic == b.italic
        && a.superscript == b.superscript
        && a.subscript == b.subscript
        && a.monospace == b.monospace
        && a.underline == b.underline
        && a.strikethrough == b.strikethrough
        && a.highlight_color == b.highlight_color
        && a.shading == b.shading
        && a.smallcaps == b.smallcaps
        && a.color == b.color
        && a.font_ascii == b.font_ascii
        && a.font_east_asia == b.font_east_asia
        && a.size_half_pt == b.size_half_pt
}

/// Merge adjacent text runs with identical formatting.
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
        DocumentStyle, IrDocumentInfo, IrEnumItem, IrListItem, IrNumberingPiece, PageSettings,
        VerticalAlign,
    };
    use typort_ooxml::document::{BlockElement, InlineElement, ParagraphStyle};

    fn empty_doc() -> IrDocument {
        IrDocument {
            info: IrDocumentInfo::default(),
            page: PageSettings::default(),
            style: DocumentStyle::default(),
            body: Vec::new(),
            footnotes: Vec::new(),
            citation_sources: Vec::new(),
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
    fn heading_number_and_bookmark() {
        let mut ir = empty_doc();
        ir.body.push(IrBlock::Heading {
            level: 1,
            numbering_text: Some("1.1".to_owned()),
            body: vec![IrInline::Text(Run::new("Lists"))],
            label: Some("sec-lists".to_owned()),
            alignment: None,
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
        assert!(matches!(
            &para.inlines[1],
            InlineElement::BookmarkEnd { .. }
        ));
        assert!(matches!(&para.inlines[2], InlineElement::Text(run) if run.text == "1.1 "));
        assert!(matches!(&para.inlines[3], InlineElement::Text(run) if run.text == "Lists"));
    }

    #[test]
    fn table_cell_fill_and_spans() {
        let mut ir = empty_doc();
        ir.body.push(IrBlock::Table(IrTable {
            rows: vec![IrTableRow {
                cells: vec![IrTableCell {
                    body: vec![IrBlock::Paragraph(text_para("a"))],
                    colspan: 2,
                    rowspan: 1,
                    vmerge: IrVMerge::Restart,
                    fill_hex: Some("DDEEFF".to_owned()),
                    vertical_align: Some(VerticalAlign::Center),
                    inset_twips: None,
                }],
            }],
            column_widths_pct: Some(vec![2500, 2500]),
            borders: None,
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
                    }),
                ],
            }],
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
                body: vec![IrBlock::Paragraph(text_para("one"))],
            }],
            numbering: pattern(&[("", IrNumeralSystem::Arabic)], ".", false),
            start: 1,
            depth: 0,
        }));
        let (doc, _) = lower(&ir, &GeometryInput::default(), CompatMode::C2);
        assert_eq!(doc.list_num_instances.len(), 1);
        assert!(doc.list_num_instances[0].level_overrides.is_empty());
    }

    #[test]
    fn multi_piece_enum_pattern_overrides_both_levels() {
        // A level's definition is recorded only when the walk reaches that
        // level, so the second piece is exercised by a nested enum.
        let nested = IrBlock::Enum(IrEnum {
            items: vec![IrEnumItem {
                number_text: Some("a)".to_owned()),
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
        });
        let mut ir = empty_doc();
        ir.body.push(IrBlock::Enum(IrEnum {
            items: vec![
                IrEnumItem {
                    number_text: Some("1)".to_owned()),
                    body: vec![IrBlock::Paragraph(text_para("one")), nested],
                },
                IrEnumItem {
                    number_text: Some("2)".to_owned()),
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
                body: vec![IrBlock::Paragraph(text_para("one"))],
            }],
            numbering: IrNumberingPattern {
                pieces: Vec::new(),
                suffix: String::new(),
                full: false,
            },
            start: 1,
            depth: 0,
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
        assert_eq!(para.inlines, vec![InlineElement::FootnoteRef(2)]);
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
        });
        para.inlines.push(IrInline::Ref {
            target_label: "tab".to_owned(),
            display_text: "1".to_owned(),
            page_ref: true,
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
}
