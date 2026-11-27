use std::io::{self, Write};

use quick_xml::Writer;

use super::WriteCtx;
use super::paragraph::write_paragraph;
use crate::ooxml::document::{CellContent, Table, TableCellMargins, VMerge};

const MAX_AUTO_COLUMNS: u32 = 63;

#[allow(clippy::too_many_lines, reason = "writes the schema-ordered table XML")]
pub(super) fn write_table<W: Write>(
    writer: &mut Writer<W>,
    table: &Table,
    ctx: &WriteCtx,
) -> io::Result<()> {
    let fixed = table.col_widths_twips.is_some();
    let num_cols = table
        .rows
        .iter()
        .map(|row| {
            row.cells
                .iter()
                .map(|cell| {
                    if fixed {
                        cell.colspan
                    } else {
                        cell.colspan.max(1)
                    }
                })
                .fold(0, u32::saturating_add)
        })
        .max()
        .unwrap_or(0);
    let num_cols = if fixed {
        num_cols
    } else {
        num_cols.min(MAX_AUTO_COLUMNS)
    };

    writer.create_element("w:tbl").write_inner_content(|w| {
        // Table properties with borders
        let tbl_width = table.width_pct.unwrap_or(5000).to_string();
        w.create_element("w:tblPr").write_inner_content(|tpr| {
            if let Some(float) = table.float {
                let (x, y, dist) = (
                    float.x_twips.to_string(),
                    float.y_twips.to_string(),
                    float.distance_twips.to_string(),
                );
                tpr.create_element("w:tblpPr")
                    .with_attribute(("w:leftFromText", dist.as_str()))
                    .with_attribute(("w:rightFromText", dist.as_str()))
                    .with_attribute(("w:topFromText", dist.as_str()))
                    .with_attribute(("w:bottomFromText", dist.as_str()))
                    .with_attribute(("w:vertAnchor", "page"))
                    .with_attribute(("w:horzAnchor", "page"))
                    .with_attribute(("w:tblpX", x.as_str()))
                    .with_attribute(("w:tblpY", y.as_str()))
                    .write_empty()?;
            }
            if let Some(dxa) = table.width_twips {
                let w_str = dxa.to_string();
                tpr.create_element("w:tblW")
                    .with_attribute(("w:w", w_str.as_str()))
                    .with_attribute(("w:type", "dxa"))
                    .write_empty()?;
            } else {
                tpr.create_element("w:tblW")
                    .with_attribute(("w:w", tbl_width.as_str()))
                    .with_attribute(("w:type", "pct"))
                    .write_empty()?;
            }
            if let Some(alignment) = table.alignment {
                tpr.create_element("w:jc")
                    .with_attribute(("w:val", alignment.ooxml_value()))
                    .write_empty()?;
            }
            tpr.create_element("w:tblBorders")
                .write_inner_content(|bdr| {
                    // Per-side thicknesses: from detected `borders` when present
                    // (so a three-line table stays three-line), else a uniform grid.
                    let uniform = Some(table.border_size.unwrap_or(4));
                    // CT_TblBorders is an xsd:sequence — child order is fixed:
                    // top, left, bottom, right, insideH, insideV.
                    let sides: [(&str, Option<u32>); 6] = match &table.borders {
                        Some(tb) => [
                            ("w:top", tb.top),
                            ("w:left", tb.left),
                            ("w:bottom", tb.bottom),
                            ("w:right", tb.right),
                            ("w:insideH", tb.inside_h),
                            ("w:insideV", tb.inside_v),
                        ],
                        None => [
                            ("w:top", uniform),
                            ("w:left", uniform),
                            ("w:bottom", uniform),
                            ("w:right", uniform),
                            ("w:insideH", uniform),
                            ("w:insideV", uniform),
                        ],
                    };
                    for (name, sz) in sides {
                        let el = bdr.create_element(name);
                        if let Some(sz) = sz {
                            let s = sz.to_string();
                            el.with_attribute(("w:val", "single"))
                                .with_attribute(("w:sz", s.as_str()))
                                .with_attribute(("w:space", "0"))
                                .write_empty()?;
                        } else {
                            el.with_attribute(("w:val", "nil")).write_empty()?;
                        }
                    }
                    Ok(())
                })?;
            if fixed {
                tpr.create_element("w:tblLayout")
                    .with_attribute(("w:type", "fixed"))
                    .write_empty()?;
            }
            if let Some(margins) = &table.cell_margins {
                write_margins(tpr, "w:tblCellMar", margins)?;
            }
            Ok(())
        })?;
        write_table_grid(w, table.col_widths_twips.as_deref(), num_cols)?;
        // Table rows
        for (row_idx, row) in table.rows.iter().enumerate() {
            w.create_element("w:tr").write_inner_content(|tr_w| {
                if row.height_twips.is_some() || row.is_header {
                    tr_w.create_element("w:trPr").write_inner_content(|trpr| {
                        if let Some(h) = row.height_twips {
                            let h_str = h.to_string();
                            trpr.create_element("w:trHeight")
                                .with_attribute(("w:val", h_str.as_str()))
                                .with_attribute(("w:hRule", "exact"))
                                .write_empty()?;
                        }
                        if row.is_header {
                            trpr.create_element("w:tblHeader").write_empty()?;
                        }
                        Ok(())
                    })?;
                }
                let mut extra_columns =
                    num_cols.saturating_sub(u32::try_from(row.cells.len()).unwrap_or(u32::MAX));
                for cell in &row.cells {
                    let colspan = if fixed {
                        cell.colspan
                    } else {
                        let extra = cell.colspan.saturating_sub(1).min(extra_columns);
                        extra_columns -= extra;
                        1 + extra
                    };
                    tr_w.create_element("w:tc").write_inner_content(|tc_w| {
                        // Emit cell properties (width, merges)
                        let has_colspan = colspan > 1;
                        let has_vmerge = cell.vmerge != VMerge::None;
                        // Always emit tcPr to set cell width
                        tc_w.create_element("w:tcPr").write_inner_content(|tcpr| {
                            if let Some(dxa) = cell.width_twips {
                                let width_str = dxa.to_string();
                                tcpr.create_element("w:tcW")
                                    .with_attribute(("w:w", width_str.as_str()))
                                    .with_attribute(("w:type", "dxa"))
                                    .write_empty()?;
                            } else {
                                let cell_width = cell
                                    .width_pct
                                    .unwrap_or_else(|| (5000 / num_cols.max(1)) * colspan);
                                let width_str = cell_width.to_string();
                                tcpr.create_element("w:tcW")
                                    .with_attribute(("w:w", width_str.as_str()))
                                    .with_attribute(("w:type", "pct"))
                                    .write_empty()?;
                            }
                            if has_colspan {
                                let span_str = colspan.to_string();
                                tcpr.create_element("w:gridSpan")
                                    .with_attribute(("w:val", span_str.as_str()))
                                    .write_empty()?;
                            }
                            if has_vmerge {
                                match &cell.vmerge {
                                    VMerge::Restart => {
                                        tcpr.create_element("w:vMerge")
                                            .with_attribute(("w:val", "restart"))
                                            .write_empty()?;
                                    }
                                    VMerge::Continue => {
                                        tcpr.create_element("w:vMerge").write_empty()?;
                                    }
                                    VMerge::None => {}
                                }
                            }
                            let header_sep = table.borders.as_ref().and_then(|tb| {
                                (tb.header_rows > 0 && row_idx + 1 == tb.header_rows)
                                    .then_some(tb.header_sep)
                                    .flatten()
                            });
                            if cell.borders.is_some() || header_sep.is_some() {
                                tcpr.create_element("w:tcBorders")
                                    .write_inner_content(|tbw| {
                                        let sides = cell.borders.clone().unwrap_or_default();
                                        for (name, side) in [
                                            ("w:top", sides.top.as_ref()),
                                            ("w:left", sides.left.as_ref()),
                                            ("w:bottom", sides.bottom.as_ref()),
                                            ("w:right", sides.right.as_ref()),
                                        ] {
                                            let sep_here =
                                                name == "w:bottom" && header_sep.is_some();
                                            match (side, sep_here) {
                                                (Some(b), _) => {
                                                    let s = b.sz_eighths.to_string();
                                                    let val =
                                                        if b.dashed { "dashed" } else { "single" };
                                                    let el = tbw
                                                        .create_element(name)
                                                        .with_attribute(("w:val", val))
                                                        .with_attribute(("w:sz", s.as_str()))
                                                        .with_attribute(("w:space", "0"));
                                                    if let Some(color) = &b.color_hex {
                                                        el.with_attribute((
                                                            "w:color",
                                                            color.as_str(),
                                                        ))
                                                        .write_empty()?;
                                                    } else {
                                                        el.write_empty()?;
                                                    }
                                                }
                                                (None, true) => {
                                                    let s =
                                                        header_sep.unwrap_or_default().to_string();
                                                    tbw.create_element(name)
                                                        .with_attribute(("w:val", "single"))
                                                        .with_attribute(("w:sz", s.as_str()))
                                                        .with_attribute(("w:space", "0"))
                                                        .write_empty()?;
                                                }
                                                (None, false) => {
                                                    if cell.borders.is_some() {
                                                        tbw.create_element(name)
                                                            .with_attribute(("w:val", "nil"))
                                                            .write_empty()?;
                                                    }
                                                }
                                            }
                                        }
                                        Ok(())
                                    })?;
                            }
                            // Cell shading (w:shd comes after tcBorders and
                            // before vAlign in the CT_TcPr schema order).
                            if let Some(shading) = &cell.shading {
                                tcpr.create_element("w:shd")
                                    .with_attribute(("w:val", "clear"))
                                    .with_attribute(("w:fill", shading.as_str()))
                                    .write_empty()?;
                            }
                            if let Some(margins) = &cell.margins {
                                write_margins(tcpr, "w:tcMar", margins)?;
                            }
                            // Vertical cell alignment (w:vAlign comes after
                            // tcBorders in the CT_TcPr schema order).
                            if let Some(va) = cell.vertical_align {
                                tcpr.create_element("w:vAlign")
                                    .with_attribute(("w:val", va.ooxml_value()))
                                    .write_empty()?;
                            }
                            Ok(())
                        })?;
                        if !cell.content.is_empty() {
                            // Cell has structured content (paragraphs + nested tables)
                            let mut has_trailing_para = false;
                            for item in &cell.content {
                                match item {
                                    CellContent::Paragraph(para) => {
                                        write_paragraph(tc_w, para, ctx)?;
                                        has_trailing_para = true;
                                    }
                                    CellContent::Table(nested_tbl) => {
                                        write_table(tc_w, nested_tbl, ctx)?;
                                        has_trailing_para = false;
                                    }
                                }
                            }
                            // OOXML requires a trailing w:p after a nested w:tbl in a cell
                            if !has_trailing_para {
                                tc_w.create_element("w:p").write_empty()?;
                            }
                        } else if cell.paragraphs.is_empty() {
                            // OOXML requires at least one paragraph per cell
                            tc_w.create_element("w:p").write_empty()?;
                        } else {
                            for para in &cell.paragraphs {
                                write_paragraph(tc_w, para, ctx)?;
                            }
                        }
                        Ok(())
                    })?;
                }
                Ok(())
            })?;
        }
        Ok(())
    })?;
    Ok(())
}

fn write_margins<W: Write>(
    writer: &mut Writer<W>,
    element: &str,
    margins: &TableCellMargins,
) -> io::Result<()> {
    writer.create_element(element).write_inner_content(|mar| {
        for (name, value) in [
            ("w:top", margins.top),
            ("w:left", margins.left),
            ("w:bottom", margins.bottom),
            ("w:right", margins.right),
        ] {
            let v = value.to_string();
            mar.create_element(name)
                .with_attribute(("w:w", v.as_str()))
                .with_attribute(("w:type", "dxa"))
                .write_empty()?;
        }
        Ok(())
    })?;
    Ok(())
}

fn write_table_grid<W: Write>(
    writer: &mut Writer<W>,
    widths: Option<&[u32]>,
    num_cols: u32,
) -> io::Result<()> {
    let grid = writer.create_element("w:tblGrid");
    if let Some(widths) = widths {
        grid.write_inner_content(|w| {
            for width in widths {
                w.create_element("w:gridCol")
                    .with_attribute(("w:w", width.to_string().as_str()))
                    .write_empty()?;
            }
            Ok(())
        })?;
    } else if num_cols > 0 {
        grid.write_inner_content(|w| {
            for _ in 0..num_cols {
                w.create_element("w:gridCol").write_empty()?;
            }
            Ok(())
        })?;
    } else {
        grid.write_empty()?;
    }
    Ok(())
}
