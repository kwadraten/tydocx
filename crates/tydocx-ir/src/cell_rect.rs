use std::ops::Range;

use crate::types::{Alignment, IrBlock, IrCellRect, IrGrid, IrInline, IrTableCell, IrVMerge};

#[derive(Clone, Copy)]
pub(super) struct GridRow<'a> {
    pub grid: &'a IrGrid,
    pub index: usize,
    pub allow_bottom: bool,
}

pub(super) struct CellRect<'a> {
    pub body: &'a [IrBlock],
    pub alignment: Option<Alignment>,
    pub paint: &'a IrCellRect,
}

impl GridRow<'_> {
    pub fn rect(self, cell: &IrTableCell) -> Option<CellRect<'_>> {
        if cell.vmerge == IrVMerge::Continue || cell.inset_twips.is_some_and(|inset| inset > 0) {
            return None;
        }
        let [
            IrBlock::Shape {
                body,
                geometry: Some(geometry),
                alignment,
                ..
            },
        ] = cell.body.as_slice()
        else {
            return None;
        };
        let paint = geometry.cell_rect.as_ref()?;
        if body.is_empty()
            || !body.iter().all(|block| {
                matches!(block, IrBlock::Paragraph(p) if !p.horizontal_rule && flows(&p.inlines))
            })
            || (paint.bottom_border.is_some() && !self.bottom_is_free(cell))
        {
            return None;
        }
        Some(CellRect {
            body,
            alignment: *alignment,
            paint,
        })
    }

    fn bottom_is_free(self, cell: &IrTableCell) -> bool {
        if !self.allow_bottom
            || cell.vmerge != IrVMerge::None
            || cell.rowspan != 1
            || cell.borders.bottom.is_some()
            || cell.borders_declared.bottom
        {
            return false;
        }
        let boundary = self.index + 1;
        let tracks = tracks(cell);
        !self
            .grid
            .hlines
            .iter()
            .any(|line| line.index == boundary && overlaps(&tracks, &(line.start..line.end)))
            && self.grid.rows.get(boundary).is_none_or(|row| {
                !row.cells.iter().any(|next| {
                    (next.borders.top.is_some() || next.borders_declared.top)
                        && overlaps(&tracks, &self::tracks(next))
                })
            })
    }
}

pub(super) fn generic_grid_allows_bottom(grid: &IrGrid) -> bool {
    grid.borders.as_ref().is_some_and(|b| {
        [
            b.top,
            b.left,
            b.bottom,
            b.right,
            b.inside_h,
            b.inside_v,
            b.header_sep,
        ]
        .iter()
        .all(Option::is_none)
    })
}

fn tracks(cell: &IrTableCell) -> Range<usize> {
    cell.x..cell.x.saturating_add(cell.colspan as usize)
}

fn overlaps(a: &Range<usize>, b: &Range<usize>) -> bool {
    a.start.max(b.start) < a.end.min(b.end)
}

fn flows(inlines: &[IrInline]) -> bool {
    inlines.iter().all(|inline| match inline {
        IrInline::Text(_)
        | IrInline::Space
        | IrInline::LineBreak { .. }
        | IrInline::Tab
        | IrInline::Ref { .. }
        | IrInline::Cite { .. }
        | IrInline::FootnoteMarker { .. }
        | IrInline::PageField { .. }
        | IrInline::Math { .. } => true,
        IrInline::Field { body, .. } | IrInline::Link { body, .. } => flows(body),
        IrInline::Image(_)
        | IrInline::Shape { .. }
        | IrInline::Overline(_)
        | IrInline::PageBreak
        | IrInline::ColumnBreak => false,
    })
}
