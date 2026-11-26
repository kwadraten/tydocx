use typst_syntax::Span;

use crate::compat::{CompatMode, Degradation, DegradationAction};

use super::{
    CellContent, HtmlElement, HtmlNode, InlineFmt, InlineOptions, Location, Paragraph, StyleChain,
    Table, TableCell, TableElem, TableRow, Tag, VMerge, WalkCtx, collect_inlines,
    content_at_location, convert_html_list_to_paragraphs, element_at_location,
    enum_location_before, enum_numbering_at, find_first_element, find_tag_end, get_attr_value,
    image, is_inline_equation_at, subtree_has_element, table_align, table_width, tag_name,
    walk_tags,
};

/// Rowspan metadata for a single cell: `(html_cell_index, rowspan, colspan)`.
pub(super) type CellSpanInfo = (usize, u32, u32);

/// A parsed table row paired with its rowspan metadata.
pub(super) type RawTableRow = (TableRow, Vec<CellSpanInfo>);
pub(super) fn handle_table_tag(
    children: &[HtmlNode],
    i: usize,
    location: Location,
    ctx: &mut WalkCtx,
) -> usize {
    let end = find_tag_end(children, i, location);
    handle_table(&children[i..=end], Some(location), ctx);
    end
}

/// Handle a `table` Tag: find the HTML `<table>` element in the inner children and parse it.
pub(super) fn handle_table(slice: &[HtmlNode], table_loc: Option<Location>, ctx: &mut WalkCtx) {
    if let Some(table) = find_first_element(slice, &|element| tag_name(element) == "table") {
        convert_html_table(table, table_loc, ctx);
        return;
    }
    // Fallback: walk inner children normally
    let inner = &slice[1..slice.len().saturating_sub(1)];
    walk_tags(inner, ctx);
}

/// The `TableElem`'s source span for a table location, or `None` when the
/// location is unknown or the span is detached. Degradations never carry a
/// fabricated span.
fn table_span(ctx: &WalkCtx, loc: Option<Location>) -> Option<Span> {
    let content = content_at_location(ctx.html_doc, loc?)?;
    let span = content.span();
    (!span.is_detached()).then_some(span)
}

/// Convert an HTML `<table>` element into the document model.
pub(super) fn convert_html_table(
    elem: &HtmlElement,
    table_loc: Option<Location>,
    ctx: &mut WalkCtx,
) {
    let Some(mut table) = convert_html_table_to_model(elem, ctx) else {
        return;
    };

    // Semantic column widths: read the declared track sizes off the TableElem
    // and turn them into per-cell percentages. Degrades to equal distribution
    // (cells left at width_pct = None) when the spec is all-`Auto`/`columns: N`,
    // or when the element is not queryable (e.g. nested tables with no location).
    if let Some(loc) = table_loc
        && let Some(table_elem) = element_at_location::<TableElem>(ctx.html_doc, loc)
    {
        let tracks = table_elem.columns.get_ref(StyleChain::default());
        let content_pt = f64::from(
            ctx.doc
                .page_settings
                .width_twips
                .saturating_sub(ctx.doc.page_settings.margin_left)
                .saturating_sub(ctx.doc.page_settings.margin_right),
        ) / 20.0;
        let wctx = table_width::TableWidthCtx {
            content_pt,
            body_font_pt: f64::from(ctx.doc.style.body_size_half_pt) / 2.0,
        };
        if let Some(col_pct) = table_width::track_widths_pct(&tracks.0, wctx) {
            table_width::assign_cell_widths(&mut table, &col_pct);
        }
        // Semantic cell alignment (the HTML `<td>`s carry none): horizontal → cell
        // paragraph `w:jc`, vertical → `w:vAlign`, read from the same TableElem.
        table_align::apply_cell_alignment(&mut table, &table_elem, ctx.world, ctx.html_doc);
        if ctx.compat == CompatMode::C1 {
            let span = table_span(ctx, table_loc);
            // M2: cell fills are dropped. The fill count needs the same
            // resolution the writer path runs, so resolve on the real
            // table and then strip the shading — the output is identical
            // to never applying it.
            let filled =
                table_align::apply_cell_fill(&mut table, &table_elem, ctx.world, ctx.html_doc);
            if filled > 0 {
                for row in &mut table.rows {
                    for cell in &mut row.cells {
                        cell.shading = None;
                    }
                }
                ctx.degradations.push(Degradation::new(
                    span,
                    "table-cell-fill",
                    DegradationAction::Skipped,
                    format!("{filled} table cells had fills; cell fills are not emitted"),
                ));
            }
            // U4: a table gutter is dropped (no inter-cell gap is emitted).
            // `gutter` itself is a parse-time shorthand (`#[external]`) —
            // it is never stored; `column_gutter`/`row_gutter` already
            // default to it, so checking those two covers all three.
            let col_gutter = table_elem.column_gutter.get_ref(StyleChain::default());
            let row_gutter = table_elem.row_gutter.get_ref(StyleChain::default());
            if !col_gutter.0.is_empty() || !row_gutter.0.is_empty() {
                ctx.degradations.push(Degradation::new(
                    span,
                    "table-gutter",
                    DegradationAction::Skipped,
                    "table gutter is not representable; cells are placed adjacently",
                ));
            }
            // U22: no gate — `TableElem.summary` is `#[internal]` +
            // `#[parse(None)]`, so no user document can ever carry one;
            // the gate could never fire.
        } else {
            // Semantic cell fill (likewise absent from the HTML): solid colors →
            // `w:shd` cell shading.
            table_align::apply_cell_fill(&mut table, &table_elem, ctx.world, ctx.html_doc);
        }
    }

    ctx.doc.add_table(table);
}

/// Post-process table rows to insert `VMerge::Continue` cells for rowspans.
///
/// In HTML, when a cell has `rowspan=N`, the subsequent N-1 rows omit the cell at that
/// column position. In OOXML, every row must have the same number of logical columns,
/// and continuation cells must have `<w:vMerge/>` (no val = continue).
pub(super) fn postprocess_rowspans(raw_rows: Vec<RawTableRow>) -> Table {
    // Track active rowspans: (logical_col_index, rows_remaining, colspan)
    // `rows_remaining` counts how many MORE rows need a continuation cell.
    let mut active_spans: Vec<(usize, u32, u32)> = Vec::new();
    let mut final_rows = Vec::new();

    for (row, span_info) in raw_rows {
        // Sort active spans by column index
        active_spans.sort_by_key(|(col, _, _)| *col);

        // Build the new row by interleaving continuation cells with source cells
        let mut new_cells = Vec::new();
        let mut logical_col: usize = 0;
        let mut src_idx: usize = 0;
        let src_cells = row.cells;

        loop {
            // Check if this logical column needs a continuation cell
            if let Some(&(_, _, colspan)) = active_spans.iter().find(|(c, _, _)| *c == logical_col)
            {
                new_cells.push(TableCell {
                    paragraphs: vec![Paragraph::new()],
                    content: Vec::new(),
                    colspan,
                    vmerge: VMerge::Continue,
                    width_pct: None,
                    vertical_align: None,
                    shading: None,
                    width_twips: None,
                    borders: None,
                    margins: None,
                });
                logical_col += colspan as usize;
            } else if src_idx < src_cells.len() {
                logical_col += src_cells[src_idx].colspan as usize;
                new_cells.push(src_cells[src_idx].clone());
                src_idx += 1;
            } else {
                break;
            }
        }

        // Decrement active spans and remove expired ones (AFTER using them for this row)
        active_spans.retain_mut(|(_, remaining, _)| {
            *remaining -= 1;
            *remaining > 0
        });

        // Register new rowspans from this row's span_info.
        // span_info indices are relative to the HTML source cells; remap to logical positions.
        for (html_col_idx, rowspan, colspan) in &span_info {
            // Find the logical column for this HTML cell index by walking the final cells,
            // skipping continuation cells (which don't correspond to HTML source cells).
            let mut logical = 0_usize;
            let mut html_idx = 0_usize;
            for cell in &new_cells {
                if cell.vmerge == VMerge::Continue {
                    logical += cell.colspan as usize;
                    continue;
                }
                if html_idx == *html_col_idx {
                    break;
                }
                logical += cell.colspan as usize;
                html_idx += 1;
            }
            if *rowspan > 1 {
                active_spans.push((logical, *rowspan - 1, *colspan));
            }
        }

        final_rows.push(TableRow {
            cells: new_cells,
            height_twips: None,
            is_header: false,
        });
    }

    Table {
        rows: final_rows,
        width_pct: None,
        border_size: None,
        borders: None,
        cell_margins: None,
        width_twips: None,
        col_widths_twips: None,
        alignment: None,
        float: None,
    }
}

/// Convert a `<tr>` element into a `TableRow` plus rowspan metadata.
///
/// Returns `(TableRow, Vec<(cell_index, rowspan, colspan)>)` where the second
/// element records which cells have `rowspan > 1` so the caller can insert
/// `VMerge::Continue` cells in subsequent rows.
pub(super) fn convert_table_row(tr: &HtmlElement, ctx: &mut WalkCtx) -> Option<RawTableRow> {
    let mut cells = Vec::new();
    let mut span_info = Vec::new();
    let mut cell_idx: usize = 0;

    for cell in &tr.children {
        if let HtmlNode::Element(td) = cell {
            let tag = tag_name(td);
            if tag == "td" || tag == "th" {
                let is_header = tag == "th";
                // Check if <td> children include <p> elements for multi-paragraph cells
                let paragraphs = convert_cell_paragraphs(td, is_header, ctx);

                // Parse colspan and rowspan attributes
                let colspan = get_attr_value(td, "colspan")
                    .and_then(|v| v.parse::<u32>().ok())
                    .unwrap_or(1);
                let rowspan = get_attr_value(td, "rowspan")
                    .and_then(|v| v.parse::<u32>().ok())
                    .unwrap_or(1);

                let vmerge = if rowspan > 1 {
                    VMerge::Restart
                } else {
                    VMerge::None
                };

                if rowspan > 1 {
                    span_info.push((cell_idx, rowspan, colspan));
                }

                // Check for nested tables within the cell
                let (final_paragraphs, cell_content) =
                    extract_cell_content_with_nested_tables(td, paragraphs, ctx);

                cells.push(TableCell {
                    paragraphs: final_paragraphs,
                    content: cell_content,
                    colspan,
                    vmerge,
                    width_pct: None,
                    vertical_align: None,
                    shading: None,
                    width_twips: None,
                    borders: None,
                    margins: None,
                });
                cell_idx += 1;
            }
        }
    }
    if cells.is_empty() {
        None
    } else {
        Some((
            TableRow {
                cells,
                height_twips: None,
                is_header: false,
            },
            span_info,
        ))
    }
}

/// Convert a `<td>` or `<th>` element's children into paragraphs.
///
/// If the cell contains `<p>` child elements, each `<p>` becomes a separate paragraph.
/// Otherwise, all inline content is collected into a single paragraph.
pub(super) fn convert_cell_paragraphs(
    td: &HtmlElement,
    is_header: bool,
    ctx: &mut WalkCtx,
) -> Vec<Paragraph> {
    // Every paragraph collected below shares the same formatting: bold iff this
    // is a header cell, nothing else set.
    let fmt = InlineFmt {
        bold: is_header,
        ..InlineFmt::default()
    };

    // Typst's HTML export drops every equation, leaving inline math as `equation`
    // Tag siblings between the cell's <p> text fragments. The per-<p> path below
    // would consume only the <p>s — dropping those equation siblings and stacking
    // a single math-bearing line into several paragraphs. When the cell carries
    // inline math, collect the whole cell as one paragraph instead, so the
    // equations are spliced back in document order. The shared inline collector
    // already turns an `equation` Tag into OMML and recurses through <p> wrappers
    // to pick up the surrounding text.
    let has_inline_equation =
        (0..td.children.len()).any(|i| is_inline_equation_at(&td.children, i, ctx.html_doc));
    if has_inline_equation {
        let mut para = Paragraph::new();
        collect_inlines(
            &td.children,
            &mut para,
            None,
            InlineOptions::generic(fmt, Some(ctx.html_doc)),
        );
        return vec![para];
    }

    // Walk the cell's children in document order: a `<p>` becomes a paragraph,
    // a `<ul>`/`<ol>` becomes list paragraphs carrying `w:numPr` (upstream
    // #31), and contiguous inline content accumulates into one paragraph.
    // Nested `<table>`s are skipped here — the nested-table path rebuilds them.
    let mut paragraphs = Vec::new();
    let mut range_start = 0;
    for idx in 0..=td.children.len() {
        if idx == td.children.len() {
            push_cell_inline_paragraph(&td.children, range_start..idx, fmt, ctx, &mut paragraphs);
            break;
        }
        let HtmlNode::Element(el) = &td.children[idx] else {
            continue;
        };
        let tag = tag_name(el);
        if tag == "p" {
            push_cell_inline_paragraph(&td.children, range_start..idx, fmt, ctx, &mut paragraphs);
            let mut para = Paragraph::new();
            collect_inlines(
                &el.children,
                &mut para,
                None,
                InlineOptions::generic(fmt, Some(ctx.html_doc)),
            );
            if !para.inlines.is_empty() {
                paragraphs.push(para);
            }
            range_start = idx + 1;
        } else if tag == "ul" || tag == "ol" {
            push_cell_inline_paragraph(&td.children, range_start..idx, fmt, ctx, &mut paragraphs);
            let ordered = tag == "ol";
            // The cell enum's numbering pattern lives on its Tag marker, a
            // sibling immediately before this `<ol>` (same layout as the
            // nested-enum case in lists.rs).
            let numbering = enum_location_before(&td.children, idx)
                .and_then(|loc| enum_numbering_at(ctx.html_doc, loc));
            paragraphs.extend(convert_html_list_to_paragraphs(
                el, ordered, numbering, ctx, fmt,
            ));
            range_start = idx + 1;
        } else if tag == "table" {
            push_cell_inline_paragraph(&td.children, range_start..idx, fmt, ctx, &mut paragraphs);
            range_start = idx + 1;
        }
    }
    if paragraphs.is_empty() {
        // Fallback: collect all content as one paragraph
        let mut para = Paragraph::new();
        collect_inlines(
            &td.children,
            &mut para,
            None,
            InlineOptions::generic(fmt, Some(ctx.html_doc)),
        );
        vec![para]
    } else {
        paragraphs
    }
}

/// Collect an inline range of a cell's children into one paragraph (if it has
/// content) and push it — the shared flush step of the cell walk.
fn push_cell_inline_paragraph(
    children: &[HtmlNode],
    range: std::ops::Range<usize>,
    fmt: InlineFmt,
    ctx: &WalkCtx,
    paragraphs: &mut Vec<Paragraph>,
) {
    if range.is_empty() {
        return;
    }
    let mut para = Paragraph::new();
    collect_inlines(
        &children[range],
        &mut para,
        None,
        InlineOptions::generic(fmt, Some(ctx.html_doc)),
    );
    if !para.inlines.is_empty() {
        paragraphs.push(para);
    }
}

/// Check if a `<td>`/`<th>` element contains nested `<table>` elements and,
/// if so, build a `Vec<CellContent>` that interleaves paragraphs and nested
/// tables in document order.
///
/// Returns `(paragraphs, content)` where:
/// - `paragraphs` is the original paragraph list (kept for backward compat)
/// - `content` is non-empty only when nested tables are present
pub(super) fn extract_cell_content_with_nested_tables(
    td: &HtmlElement,
    paragraphs: Vec<Paragraph>,
    ctx: &mut WalkCtx,
) -> (Vec<Paragraph>, Vec<CellContent>) {
    // Check if any child (direct or nested in a wrapper div/span) is a <table>
    // `subtree_has_element` also sees tables represented as flattened
    // `Tag::Start` markers, which an element-only walk missed.
    let has_nested_table = subtree_has_element(&td.children, "table");
    if !has_nested_table {
        return (paragraphs, Vec::new());
    }

    // Walk children in order, collecting paragraphs and nested tables
    let mut content: Vec<CellContent> = Vec::new();
    collect_cell_content_recursive(&td.children, &mut content, ctx);

    // Also build the flat paragraphs list for backward compat
    let flat_paragraphs: Vec<Paragraph> = content
        .iter()
        .filter_map(|c| {
            if let CellContent::Paragraph(p) = c {
                Some(p.clone())
            } else {
                None
            }
        })
        .collect();

    let final_paragraphs = if flat_paragraphs.is_empty() {
        vec![Paragraph::new()]
    } else {
        flat_paragraphs
    };

    (final_paragraphs, content)
}

/// Recursively collect cell content (paragraphs and nested tables) from HTML
/// children, preserving document order.
pub(super) fn collect_cell_content_recursive(
    children: &[HtmlNode],
    content: &mut Vec<CellContent>,
    ctx: &mut WalkCtx,
) {
    for (idx, child) in children.iter().enumerate() {
        match child {
            HtmlNode::Element(el) => {
                let tag = tag_name(el);
                if tag == "table" {
                    // Convert this as a nested table
                    let table = convert_html_table_to_model(el, ctx);
                    if let Some(t) = table {
                        content.push(CellContent::Table(t));
                    }
                } else if tag == "ul" || tag == "ol" {
                    // A list inside the cell: same list handling as body
                    // lists, so its items carry `w:numPr` (upstream #31).
                    // The enum's numbering pattern comes from its Tag
                    // marker, a sibling immediately before this `<ol>`.
                    let ordered = tag == "ol";
                    let numbering = enum_location_before(children, idx)
                        .and_then(|loc| enum_numbering_at(ctx.html_doc, loc));
                    let paras = convert_html_list_to_paragraphs(
                        el,
                        ordered,
                        numbering,
                        ctx,
                        InlineFmt::default(),
                    );
                    for para in paras {
                        content.push(CellContent::Paragraph(para));
                    }
                } else if tag == "p" {
                    let mut para = Paragraph::new();
                    collect_inlines(
                        &el.children,
                        &mut para,
                        None,
                        InlineOptions::generic(InlineFmt::default(), Some(ctx.html_doc)),
                    );
                    if !para.inlines.is_empty() {
                        content.push(CellContent::Paragraph(para));
                    }
                } else if tag != "math" {
                    // Recurse into wrapper elements (div, span, etc.). A bare
                    // `<math>` (equation outside a `<p>`) is skipped: its OMML
                    // comes from the sibling equation Tag below — recursing
                    // would leak the MathML glyphs as literal cell text.
                    collect_cell_content_recursive(&el.children, content, ctx);
                }
            }
            HtmlNode::Text(text, _) => {
                let trimmed = text.as_str().trim();
                if !trimmed.is_empty() {
                    let mut para = Paragraph::new();
                    para.add_run(trimmed);
                    content.push(CellContent::Paragraph(para));
                }
            }
            HtmlNode::Tag(tag) => {
                // A bare equation in the cell (outside any `<p>`): convert it
                // through the introspector like the inline collector does.
                if let Tag::Start(c, _) = tag
                    && c.elem().name() == "equation"
                    && let Some(eq) = content_at_location(ctx.html_doc, tag.location())
                {
                    let omml = tydocx_math::equation_to_omml(&eq);
                    let mut para = Paragraph::new();
                    para.add_math(omml);
                    content.push(CellContent::Paragraph(para));
                }
            }
            HtmlNode::Frame(frame) => {
                // Layouted-opaque content in a cell: rasterize in place.
                if let Some(img) = image::rasterize_html_frame(frame) {
                    // U21 (C1 only): the frame's vector/text content is no
                    // longer editable. The frame carries no span or page,
                    // so the degradation is span-less without a page hint.
                    if ctx.compat == CompatMode::C1 {
                        ctx.degradations.push(Degradation::new(
                            None,
                            "html-frame",
                            DegradationAction::Approximated,
                            "HTML frame rasterized to PNG; content is no longer editable text",
                        ));
                    }
                    let mut para = Paragraph::new();
                    para.add_image(img);
                    content.push(CellContent::Paragraph(para));
                }
            }
        }
    }
}

/// Convert an HTML `<table>` element into a `Table` model (without adding to doc).
/// Returns `None` if the table has no rows.
pub(super) fn convert_html_table_to_model(elem: &HtmlElement, ctx: &mut WalkCtx) -> Option<Table> {
    let raw_rows = collect_table_rows(elem, ctx);
    (!raw_rows.is_empty()).then(|| postprocess_rowspans(raw_rows))
}

/// Collect direct and section-wrapped HTML table rows once for every table path.
pub(super) fn collect_table_rows(elem: &HtmlElement, ctx: &mut WalkCtx) -> Vec<RawTableRow> {
    let mut raw_rows = Vec::new();
    for child in &elem.children {
        if let HtmlNode::Element(row_or_section) = child {
            let tag = tag_name(row_or_section);
            if tag == "tr" {
                if let Some(result) = convert_table_row(row_or_section, ctx) {
                    raw_rows.push(result);
                }
            } else if tag == "thead" || tag == "tbody" || tag == "tfoot" {
                for inner in &row_or_section.children {
                    if let HtmlNode::Element(tr) = inner
                        && tag_name(tr) == "tr"
                        && let Some(result) = convert_table_row(tr, ctx)
                    {
                        raw_rows.push(result);
                    }
                }
            }
        }
    }
    raw_rows
}
