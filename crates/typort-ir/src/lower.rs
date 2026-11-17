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
    IrGrid, IrHeadingNumbering, IrImage, IrInline, IrLevelDef, IrList,
    IrNumberingPattern, IrNumeralSystem, IrParagraph, IrRaw, IrSectionBreakType, IrTable,
    IrTableCell, IrTableRow, IrVMerge, ParagraphStyle, Run,
};

#[derive(Debug, Default)]
pub struct LowerCtx {
    pub compat: CompatMode,
    pub body_size_half_pt: u32,
    pub heading_sizes: [u32; 5],
    pub footnote_ids: HashMap<u32, u32>,
    pub next_bookmark_id: u32,
    pub degradations: Vec<Degradation>,
    /// Simulated heading-numbering counters per num instance (`numId`), used
    /// to compare Word's expected number against the number Typst rendered —
    /// a source-side `#counter(heading).update` shifts the two apart.
    pub heading_counters: HashMap<u32, HeadingNumState>,
    /// Whether an unrepresentable heading numbering was already reported.
    pub unmappable_heading_reported: bool,
}

#[derive(Debug, Default)]
pub struct HeadingNumState {
    /// Counter values for heading levels 0..=8 (index = level - 1).
    pub counters: [u32; 9],
    /// Whether a counter divergence was already reported for this instance.
    pub reported: bool,
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
    doc.footer.clone_from(&geometry.footer);
    doc.page_numbering = geometry.page_numbering;
    doc.page_number_footer = geometry.page_number_footer;

    let mut ctx = LowerCtx {
        compat,
        body_size_half_pt: ir.style.body_size_half_pt,
        heading_sizes: ir.style.heading_sizes,
        footnote_ids: HashMap::new(),
        next_bookmark_id: doc.bookmark_counter,
        degradations: Vec::new(),
        heading_counters: HashMap::new(),
        unmappable_heading_reported: false,
    };

    for footnote in &ir.footnotes {
        if compat.is_c1() && !footnote_marker_representable(&footnote.rendered) {
            ctx.degrade(
                "footnote-numbering",
                DegradationAction::Literalized,
                format!(
                    "footnote numbering {:?} is not representable; references using it are dropped",
                    footnote.rendered
                ),
            );
        }
        let content = lower_footnote_body(footnote, &mut ctx);
        let id = doc.add_footnote(content);
        ctx.footnote_ids.insert(footnote.ordinal, id);
    }

    for block in &ir.body {
        let lowered = lower_block(block, &mut doc, &mut ctx);
        doc.body.elements.extend(lowered);
    }

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

fn lower_footnote_body(footnote: &IrFootnote, ctx: &mut LowerCtx) -> Vec<InlineElement> {
    let mut content = Vec::new();
    for para in &footnote.body {
        content.extend(lower_inlines(&para.inlines, ctx));
    }
    content
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
                vec![BlockElement::Table(lower_grid(grid, doc, ctx))]
            }
        }
        IrBlock::Figure {
            body,
            caption,
            caption_alignment,
            kind: _,
            supplement: _,
            label,
            caption_first,
            custom_placement,
            gap_twips,
        } => {
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
                    label: label.as_ref(),
                    caption_first: *caption_first,
                    gap_twips: *gap_twips,
                },
                doc,
                ctx,
            )
        }
        IrBlock::List(list) => lower_list(list, doc, ctx, inherited),
        IrBlock::Enum(enumer) => lower_enum(enumer, doc, ctx, inherited),
        IrBlock::Equation(eq) => lower_equation(eq, ctx),
        IrBlock::Quote { body } => lower_quote(body, doc, ctx),
        IrBlock::Raw(raw) => lower_raw(raw),
        IrBlock::Outline {
            title,
            entries: _,
            depth,
            custom_target,
            custom_indent,
        } => lower_outline(title, *depth, *custom_target, *custom_indent, ctx),
        IrBlock::Bibliography { entries } => lower_bibliography(entries, ctx),
        IrBlock::Place { float, body } => lower_place(*float, body, doc, ctx),
        IrBlock::PageBreak { weak, to } => lower_pagebreak(*weak, *to),
        IrBlock::Shape { kind, span, body } => {
            if ctx.compat.is_c1() {
                ctx.degrade_at(
                    *span,
                    "shape",
                    DegradationAction::Skipped,
                    format!("{kind} dropped"),
                );
            }
            let mut out = Vec::new();
            for inner in body {
                out.extend(lower_block(inner, doc, ctx));
            }
            out
        }
    }
}

fn lower_outline(
    title: &[IrInline],
    depth: u8,
    custom_target: bool,
    custom_indent: bool,
    ctx: &mut LowerCtx,
) -> Vec<BlockElement> {
    if ctx.compat.is_c1() {
        if custom_target {
            ctx.degrade(
                "outline-target",
                DegradationAction::Skipped,
                "outline target is not headings; entries are dropped and only the title is kept"
                    .to_string(),
            );
        }
        if custom_indent {
            ctx.degrade(
                "outline-indent",
                DegradationAction::Skipped,
                "outline indent is custom; the table of contents uses the default indent"
                    .to_string(),
            );
        }
    }
    // A heading TOC field would list headings the source did not ask for;
    // keep only the outline's own title.
    if custom_target {
        let mut para = Paragraph::new();
        para.inlines = merge_runs(lower_inlines(title, ctx));
        return vec![BlockElement::Paragraph(para)];
    }
    let mut out = Vec::new();
    if !title.is_empty() {
        let mut title_para = Paragraph::new();
        title_para.inlines = lower_inlines(title, ctx);
        out.push(BlockElement::Paragraph(title_para));
    }
    let mut para = Paragraph::new();
    para.add_toc(depth);
    out.push(BlockElement::Paragraph(para));
    out
}

#[allow(
    clippy::too_many_arguments,
    reason = "the heading IR fields are passed through; bundling them into a struct only wraps the enum variant"
)]
fn lower_heading(
    level: u8,
    numbering_text: Option<&String>,
    numbering: Option<&IrHeadingNumbering>,
    body: &[IrInline],
    label: Option<&String>,
    alignment: Option<Alignment>,
    span: Option<typst_syntax::Span>,
    doc: &mut Document,
    ctx: &mut LowerCtx,
) -> Vec<BlockElement> {
    let mut para = Paragraph::new();
    para.style = Some(ParagraphStyle::Heading(level));
    para.alignment = alignment;
    let num_id = match numbering {
        Some(IrHeadingNumbering::Mappable(pattern))
            if !ctx.compat.is_c1() && numbering_text.is_some() =>
        {
            let levels = heading_level_overrides(pattern);
            let (num_id, _) = doc.find_or_allocate_heading_numbering(&levels);
            if let Some(rendered) = numbering_text {
                check_heading_number(ctx, num_id, pattern, level, rendered, span);
            }
            Some(num_id)
        }
        Some(IrHeadingNumbering::Unmappable) => {
            if !ctx.unmappable_heading_reported {
                ctx.unmappable_heading_reported = true;
                ctx.degrade_at(
                    span,
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
            level: u32::from(level).saturating_sub(1),
            id: num_id,
        });
    } else if let Some(number) = numbering_text {
        para.push_run(Run::new(format!("{number} ")));
    }
    para.inlines.extend(lower_inlines(body, ctx));
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
/// Word will render against the one Typst already rendered. A divergence —
/// a source `#counter(heading).update`, a numbering switched mid-document —
/// is reported once per numbering instance so the user knows the emitted
/// `w:numPr` does not match the source.
fn check_heading_number(
    ctx: &mut LowerCtx,
    num_id: u32,
    pattern: &IrNumberingPattern,
    level: u8,
    rendered: &str,
    span: Option<typst_syntax::Span>,
) {
    let idx = usize::from(level).saturating_sub(1).min(8);
    let mut diverged = false;
    let state = ctx.heading_counters.entry(num_id).or_default();
    state.counters[idx] = state.counters[idx].saturating_add(1);
    for counter in state.counters.iter_mut().skip(idx + 1) {
        *counter = 0;
    }
    if expected_heading_number(pattern, &state.counters, idx) != rendered && !state.reported {
        state.reported = true;
        diverged = true;
    }
    if diverged {
        ctx.degrade_at(
            span,
            "heading-numbering",
            DegradationAction::Approximated,
            "heading counter was shifted in the source; Word numbering restarts at 1".to_string(),
        );
    }
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

fn lower_table_block(table: &IrTable, doc: &mut Document, ctx: &mut LowerCtx) -> Vec<BlockElement> {
    vec![BlockElement::Table(lower_table(table, doc, ctx))]
}

struct FigureSpec<'a> {
    body: &'a [IrBlock],
    caption: &'a [IrInline],
    caption_alignment: Option<Alignment>,
    label: Option<&'a String>,
    caption_first: bool,
    gap_twips: Option<u32>,
}

fn lower_heading_block(block: &IrBlock, doc: &mut Document, ctx: &mut LowerCtx) -> Vec<BlockElement> {
    let IrBlock::Heading {
        level,
        numbering_text,
        numbering,
        body,
        label,
        alignment,
        span,
    } = block
    else {
        return Vec::new();
    };
    lower_heading(
        *level,
        numbering_text.as_ref(),
        numbering.as_ref(),
        body,
        label.as_ref(),
        *alignment,
        *span,
        doc,
        ctx,
    )
}

fn lower_figure(fig: &FigureSpec<'_>, doc: &mut Document, ctx: &mut LowerCtx) -> Vec<BlockElement> {
    let mut caption_para: Option<Paragraph> = None;
    if !fig.caption.is_empty() || fig.label.is_some() {
        let mut para = Paragraph::new();
        para.alignment = fig.caption_alignment;
        if let Some(gap) = fig.gap_twips {
            if fig.caption_first {
                para.spacing_after = Some(gap);
            } else {
                para.spacing_before = Some(gap);
            }
        }
        para.inlines.extend(lower_inlines(fig.caption, ctx));
        if let Some(name) = fig.label {
            let id = ctx.next_bookmark();
            para.add_bookmark_at_start(id, name.clone());
        }
        caption_para = Some(para);
    }
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
        } else if let Some(name) = fig.label {
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

fn lower_paragraph(para: &IrParagraph, ctx: &mut LowerCtx) -> Paragraph {
    let mut out = Paragraph::new();
    out.style.clone_from(&para.style);
    out.alignment = para.alignment;
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
    if let Some(name) = &para.bookmark {
        let id = ctx.next_bookmark();
        out.add_bookmark_at_start(id, name.clone());
    }
    out
}

fn lower_inlines(inlines: &[IrInline], ctx: &mut LowerCtx) -> Vec<InlineElement> {
    let mut out = Vec::new();
    for inline in inlines {
        match inline {
            IrInline::Text(run) => out.push(InlineElement::Text(lower_run(run, ctx))),
            IrInline::Link { url, anchor, body } => {
                let runs = inline_to_runs(body, ctx);
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
                    out.push(InlineElement::Text(Run::new(rendered)));
                }
            }
            IrInline::LineBreak => {
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
            IrInline::PageField { display_text } => {
                out.push(InlineElement::FieldPage {
                    display_text: display_text.clone(),
                });
            }
            IrInline::Shape { kind, span, body } => {
                if ctx.compat.is_c1() {
                    ctx.degrade_at(
                        *span,
                        "shape",
                        DegradationAction::Skipped,
                        format!("{kind} dropped"),
                    );
                }
                out.extend(lower_inlines(body, ctx));
            }
            IrInline::Overline(line) => out.push(InlineElement::AnchoredLine(line.clone())),
        }
    }
    out
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
        width_emu: image.width_emu,
        height_emu: image.height_emu,
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
            | IrInline::PageField { display_text } => {
                runs.push(Run::new(display_text));
            }
            IrInline::FootnoteMarker { rendered, .. } => runs.push(Run::new(rendered)),
            IrInline::LineBreak => {
                let mut run = Run::new(String::new());
                run.line_break = true;
                runs.push(run);
            }
            IrInline::Space => runs.push(Run::new(" ")),
            IrInline::Link { body, .. } | IrInline::Shape { body, .. } => {
                runs.extend(inline_to_runs(body, ctx));
            }
            IrInline::Image(_)
            | IrInline::Math { .. }
            | IrInline::PageBreak
            | IrInline::Overline(_)
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
        cell_margins: None,
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
    if enumer.numbering.pieces.is_empty()
        && inherited.is_none()
        && enumer.items.iter().all(|item| item.number_text.is_some())
    {
        return lower_enum_literal(enumer, doc, ctx);
    }
    if ctx.compat.is_c1() && enumer.reversed {
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

fn footnote_marker_representable(marker: &str) -> bool {
    marker.parse::<u32>().is_ok() || parse_circled_number(marker).is_some()
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
        && a.tracking_twips == b.tracking_twips
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
        DocumentStyle, IrDocumentInfo, IrEnumItem, IrListItem, IrNumberingPiece, IrShapeKind,
        PageSettings, VerticalAlign,
    };
    use typort_ooxml::document::{AnchoredLine, BlockElement, InlineElement, ParagraphStyle};

    fn empty_doc() -> IrDocument {
        IrDocument {
            info: IrDocumentInfo::default(),
            page: PageSettings::default(),
            style: DocumentStyle::default(),
            body: Vec::new(),
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
    fn heading_number_and_bookmark() {
        let mut ir = empty_doc();
        ir.body.push(IrBlock::Heading {
            level: 1,
            numbering_text: Some("1.1".to_owned()),
            numbering: None,
            body: vec![IrInline::Text(Run::new("Lists"))],
            label: Some("sec-lists".to_owned()),
            alignment: None,
            span: None,
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
            gutter: false,
            cell_margins: None,
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

    #[test]
    fn adjacent_line_break_runs_do_not_merge() {
        let mut ir = empty_doc();
        let mut para = IrParagraph::default();
        para.inlines.push(IrInline::LineBreak);
        para.inlines.push(IrInline::LineBreak);
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
            y_emu: -12_700,
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
                if matches!(&para.inlines[0], InlineElement::AnchoredLine(line) if line.y_emu < 0)
        ));
    }

    #[test]
    fn c1_reversed_enum_reports() {
        let mut ir = empty_doc();
        ir.body.push(IrBlock::Enum(IrEnum {
            items: vec![IrEnumItem {
                number_text: Some("1.".to_owned()),
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
    fn footnote_markers() {
        assert!(footnote_marker_representable("1"));
        assert!(footnote_marker_representable("42"));
        assert!(footnote_marker_representable("①"));
        assert!(footnote_marker_representable("⑳"));
        assert!(!footnote_marker_representable("a)"));
        assert!(!footnote_marker_representable("1)"));
        assert!(!footnote_marker_representable(""));
        assert_eq!(parse_circled_number("①"), Some(1));
        assert_eq!(parse_circled_number("㊿"), Some(50));
        assert_eq!(parse_circled_number("①②"), None);
    }
}
