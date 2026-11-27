//! Faithful table-cell alignment from the semantic `TableElem`.
//!
//! Typst's HTML export emits bare `<td>` with no alignment, so cell alignment is
//! lost on the HTML path. The alignment the author actually set lives in the
//! semantic `TableElem` (queried from the introspector), so we read it there and
//! apply it to the IR: HTML supplies table *structure*, the introspector supplies
//! the *styling* the HTML dropped — mirroring Pandoc's AST→writer split, adapted to
//! tydocx's three-source model.
//!
//! Nothing here is hardcoded: each cell gets exactly the alignment Typst resolves
//! for it (per-cell override, else the table-level value/array/closure), or nothing
//! when the author leaves it `auto`.

use crate::ooxml::document::{Alignment, CellContent, Table, TableCell, VMerge, VerticalAlign};
use typst::World;
use typst::comemo::Track;
use typst::engine::{Engine, Route, Sink, Traced};
use typst::foundations::{Smart, StyleChain};
use typst::introspection::Introspector;
use typst_html::HtmlDocument;
use typst_library::layout::{Alignment as TstAlignment, Celled, HAlignment, VAlignment};
use typst_library::model::{TableCell as TstCell, TableChild, TableElem, TableItem};
use typst_library::visualize::Paint;
use typst_utils::Protected;

use crate::world::TydocxWorld;
/// Apply each cell's authored alignment (from the semantic `TableElem`) to the IR
/// table — vertical → `vertical_align` (`w:vAlign`), horizontal → the cell
/// paragraphs' alignment (`w:jc`). Cells the author left unaligned are untouched.
pub(super) fn apply_cell_alignment(
    table: &mut Table,
    table_elem: &TableElem,
    world: &TydocxWorld,
    html_doc: &HtmlDocument,
) {
    let semantic_cells = collect_cells(table_elem);
    let table_align = table_elem.align.get_ref(StyleChain::default());
    let mut sink = Sink::new();
    let traced = Traced::default();
    let introspector: &dyn Introspector = &**html_doc.introspector();
    let mut engine = Engine {
        world: (world as &dyn World).track(),
        library: world.library(),
        introspector: Protected::new(introspector.track()),
        traced: traced.track(),
        sink: sink.track_mut(),
        route: Route::default(),
    };

    // Walk the IR's origin cells (skipping `vMerge` continuation placeholders, which
    // have no semantic counterpart) in document order, alongside the semantic cells.
    let mut sem = semantic_cells.iter();
    for (row_idx, row) in table.rows.iter_mut().enumerate() {
        let mut col = 0usize;
        for cell in &mut row.cells {
            if cell.vmerge == VMerge::Continue {
                col += cell.colspan as usize;
                continue;
            }
            let Some(sc) = sem.next() else { return };
            if let Some(align) = resolve_align(sc, table_align, col, row_idx, &mut engine) {
                apply_align(cell, align);
            }
            col += cell.colspan as usize;
        }
    }
}

/// Map a Typst `Paint` to the 6-digit uppercase hex for `w:shd` cell
/// shading. Only solid colors map; gradients and tilings have no cell-level
/// OOXML equivalent. Pure: no I/O, no global state. The hex comparison
/// ignores alpha, matching the existing foreground-color convention
/// (`extract_non_black_color`) and `highlight_style`.
#[must_use]
pub fn paint_to_shd_hex(paint: &Paint) -> Option<String> {
    let Paint::Solid(color) = paint else {
        return None;
    };
    let hex = color.to_hex();
    Some(
        hex.strip_prefix('#')
            .map_or("", |digits| &digits[..6.min(digits.len())])
            .to_uppercase(),
    )
}

/// Apply each cell's authored fill (from the semantic `TableElem`) to the IR
/// table — solid colors become `w:shd` cell shading. Cells the author left
/// unfilled are untouched. Mirrors `apply_cell_alignment`: HTML supplies
/// table *structure*, the introspector supplies the *styling* the HTML
/// dropped.
///
/// Returns the number of cells that received a shading, so the C1 mode gate
/// can report (and then strip) the dropped fills.
pub(super) fn apply_cell_fill(
    table: &mut Table,
    table_elem: &TableElem,
    world: &TydocxWorld,
    html_doc: &HtmlDocument,
) -> usize {
    let semantic_cells = collect_cells(table_elem);
    let table_fill = table_elem.fill.get_ref(StyleChain::default());
    let mut sink = Sink::new();
    let traced = Traced::default();
    let introspector: &dyn Introspector = &**html_doc.introspector();
    let mut engine = Engine {
        world: (world as &dyn World).track(),
        library: world.library(),
        introspector: Protected::new(introspector.track()),
        traced: traced.track(),
        sink: sink.track_mut(),
        route: Route::default(),
    };

    // Walk the IR's origin cells (skipping `vMerge` continuation placeholders, which
    // have no semantic counterpart) in document order, alongside the semantic cells.
    let mut filled = 0usize;
    let mut sem = semantic_cells.iter();
    for (row_idx, row) in table.rows.iter_mut().enumerate() {
        let mut col = 0usize;
        for cell in &mut row.cells {
            if cell.vmerge == VMerge::Continue {
                col += cell.colspan as usize;
                continue;
            }
            let Some(sc) = sem.next() else { return filled };
            if let Some(hex) = resolve_fill(sc, table_fill, col, row_idx, &mut engine) {
                cell.shading = Some(hex);
                filled += 1;
            }
            col += cell.colspan as usize;
        }
    }
    filled
}

/// Resolve a cell's fill: a per-cell override wins; otherwise the table-level
/// fill resolved by Typst at the cell coordinate. Returns `None` for `auto`,
/// `none`, gradients, tilings, or when evaluating the authored closure fails.
fn resolve_fill(
    cell: &TstCell,
    table_fill: &Celled<Option<Paint>>,
    col: usize,
    row: usize,
    engine: &mut Engine,
) -> Option<String> {
    if let Smart::Custom(fill) = cell.fill.get_ref(StyleChain::default()) {
        return fill.as_ref().and_then(paint_to_shd_hex);
    }
    let paint = table_fill
        .resolve(engine, StyleChain::default(), col, row)
        .ok()??;
    paint_to_shd_hex(&paint)
}

/// Flatten the table's semantic cells in document order (headers/footers included,
/// `hline`/`vline` skipped), matching the HTML cell order the IR was built from.
fn collect_cells(table_elem: &TableElem) -> Vec<TstCell> {
    let mut out = Vec::new();
    for child in &table_elem.children {
        match child {
            TableChild::Item(item) => push_cell(item, &mut out),
            TableChild::Header(header) => {
                for item in &header.children {
                    push_cell(item, &mut out);
                }
            }
            TableChild::Footer(footer) => {
                for item in &footer.children {
                    push_cell(item, &mut out);
                }
            }
        }
    }
    out
}

fn push_cell(item: &TableItem, out: &mut Vec<TstCell>) {
    if let TableItem::Cell(cell) = item {
        out.push((**cell).clone());
    }
}

/// Resolve a cell's alignment: a per-cell override wins; otherwise the table-level
/// alignment resolved by Typst at the cell coordinate. Returns `None` for `auto`
/// or when evaluating the authored closure fails.
fn resolve_align(
    cell: &TstCell,
    table_align: &Celled<Smart<TstAlignment>>,
    col: usize,
    row: usize,
    engine: &mut Engine,
) -> Option<TstAlignment> {
    if let Smart::Custom(a) = cell.align.get_ref(StyleChain::default()) {
        return Some(*a);
    }
    match table_align
        .resolve(engine, StyleChain::default(), col, row)
        .ok()?
    {
        Smart::Custom(align) => Some(align),
        Smart::Auto => None,
    }
}

/// Vertical → `vertical_align` (`w:vAlign`); horizontal → cell paragraphs' alignment
/// (`w:jc`), exactly as Pandoc does horizontal table alignment.
fn apply_align(cell: &mut TableCell, align: TstAlignment) {
    if let Some(v) = align.y() {
        cell.vertical_align = Some(match v {
            VAlignment::Top => VerticalAlign::Top,
            VAlignment::Horizon => VerticalAlign::Center,
            VAlignment::Bottom => VerticalAlign::Bottom,
        });
    }
    if let Some(h) = align.x() {
        let ha = match h {
            HAlignment::Left | HAlignment::Start => Alignment::Left,
            HAlignment::Center => Alignment::Center,
            HAlignment::Right | HAlignment::End => Alignment::Right,
        };
        for para in &mut cell.paragraphs {
            para.alignment = Some(ha);
        }
        for item in &mut cell.content {
            if let CellContent::Paragraph(para) = item {
                para.alignment = Some(ha);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use typst_library::layout::Angle;
    use typst_library::visualize::{
        Color, ColorSpace, Gradient, LinearGradient, Paint, ProcessColorSpace,
    };

    #[test]
    fn solid_color_maps_to_uppercase_hex() {
        assert_eq!(
            paint_to_shd_hex(&Paint::Solid(Color::from_u8(0xdd, 0xee, 0xff, 0xff))),
            Some("DDEEFF".to_string())
        );
        // Alpha is ignored: the same RGB with any alpha maps the same way.
        assert_eq!(
            paint_to_shd_hex(&Paint::Solid(Color::from_u8(0xdd, 0xee, 0xff, 0x80))),
            Some("DDEEFF".to_string())
        );
    }

    #[test]
    fn gradient_has_no_cell_shading() {
        let gradient = Gradient::Linear(Arc::new(LinearGradient {
            stops: vec![],
            angle: Angle::deg(0.0),
            space: ColorSpace::Process(ProcessColorSpace::Srgb),
            relative: Smart::Auto,
            anti_alias: true,
        }));
        assert_eq!(paint_to_shd_hex(&Paint::Gradient(gradient)), None);
    }
}
