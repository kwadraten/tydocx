//! Pure grid → Word-table geometry planning.
//!
//! A resolved `IrGrid` is compressed into `PlannedTable`s: rows bounded by no
//! declared cell edge, line, or stroke are run-length-merged (the 5-minute
//! slots of a timetable collapse into single rows), and adjacent columns that
//! share occupancy across every row coalesce into one lane (the interval
//! graph of a timetable's sub-columns). Cells that span lanes keep their span
//! as `w:gridSpan` fillers; cells that span rows keep it as `w:vMerge`
//! continuations. Tables over [`MAX_COLUMNS`] lanes split at lane boundaries.

use std::collections::{BTreeMap, BTreeSet};

use crate::types::{
    IrBorder, IrCellBorders, IrGrid, IrGridLine, IrTableCell, IrTrackSize, IrVMerge,
};

/// Word's practical column limit; grids wider than this are split at lane
/// boundaries (a week's days become separate tables).
pub const MAX_COLUMNS: usize = 63;

#[derive(Debug, Clone, PartialEq)]
pub struct GridPlan {
    pub tables: Vec<PlannedTable>,
    /// `false` when compression changed nothing and heights need no pinning:
    /// the caller lowers the grid through the generic table path.
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlannedTable {
    /// Absolute lane widths in twips when the content width is known;
    /// `None` falls back to `column_widths_pct`.
    pub col_widths_twips: Option<Vec<u32>>,
    pub rows: Vec<PlannedRow>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlannedRow {
    /// Sum of the merged source rows' heights — emitted as `w:trHeight`
    /// `exact` (slot heights round down, never inflate).
    pub height_twips: Option<u32>,
    pub is_header: bool,
    pub cells: Vec<PlannedCell>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlannedCell {
    pub kind: PlannedCellKind,
    /// `w:gridSpan` over the merged lanes.
    pub col_span: u32,
    /// `w:vMerge` span over the merged rows (`1` = no vertical merge).
    pub row_span: u32,
    pub borders: IrCellBorders,
    /// Fill inherited by span continuations so coloured bands stay solid.
    pub fill_hex: Option<String>,
    /// Zero cell margins + a 1pt paragraph keep empty rows from inflating
    /// to Word's minimum row height.
    pub zero_margins: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlannedCellKind {
    /// A declared cell: `row`/`index` address it in `grid.rows[row].cells`.
    Real { row: usize, index: usize },
    /// Continues a `col_span` of the `Real` cell to its left.
    Filler,
    /// Continues a row span (`w:vMerge` without `restart`).
    VContinue,
    /// Uncovered whitespace.
    Empty,
}

/// Plan a resolved grid into fixed-layout tables. `content_width_twips` is
/// the width the grid occupies; without it lane widths stay proportional.
#[must_use]
pub fn plan_grid(grid: &IrGrid, content_width_twips: Option<u32>) -> GridPlan {
    let n_rows = grid.rows.len();
    let n_cols = grid.col_sizes.len().max(max_col(grid));
    if n_rows == 0 || n_cols == 0 {
        return GridPlan {
            tables: Vec::new(),
            changed: false,
        };
    }

    let occupancy = occupancy_grid(grid, n_rows, n_cols);
    let row_boundaries = row_boundary_cuts(grid, n_rows);
    let col_boundaries = col_boundary_cuts(grid, &occupancy, n_rows, n_cols);

    let n_out_rows = row_boundaries.len() - 1;
    let n_out_cols = col_boundaries.len() - 1;

    let hbound = horizontal_boundary_map(grid, &occupancy, n_cols);
    let vbound = vertical_boundary_map(grid, &occupancy, n_rows);

    let heights = merged_row_heights(grid, &row_boundaries);
    let lane_widths = lane_widths_twips(grid, &col_boundaries, content_width_twips);

    // Emit one out-table per <=MAX_COLUMNS lane chunk.
    let mut chunks: Vec<(usize, usize)> = Vec::new();
    let mut start = 0_usize;
    while start < n_out_cols {
        let end = (start + MAX_COLUMNS).min(n_out_cols);
        chunks.push((start, end));
        start = end;
    }

    let scaffold = PlanScaffold {
        occupancy: &occupancy,
        row_boundaries: &row_boundaries,
        col_boundaries: &col_boundaries,
        hbound: &hbound,
        vbound: &vbound,
        heights: &heights,
    };
    let mut tables = Vec::new();
    for (lane0, lane1) in chunks {
        let rows = planned_rows(grid, &scaffold, lane0, lane1);
        tables.push(PlannedTable {
            col_widths_twips: lane_widths.as_ref().map(|w| w[lane0..lane1].to_vec()),
            rows,
        });
    }

    let wants_heights = heights.iter().any(Option::is_some)
        && grid
            .row_sizes
            .iter()
            .any(|s| !matches!(s, IrTrackSize::Auto));
    // A source row with no declared cells would be dropped by the generic
    // table path, so its presence also forces the planned path.
    let has_empty_rows = grid
        .rows
        .iter()
        .any(|row| row.cells.iter().all(|c| !is_declared(c)));
    GridPlan {
        changed: n_out_rows < n_rows
            || n_out_cols < n_cols
            || n_out_cols > MAX_COLUMNS
            || wants_heights
            || has_empty_rows,
        tables,
    }
}

/// Planning state shared by every lane chunk of one grid.
struct PlanScaffold<'a> {
    occupancy: &'a [Vec<Option<(usize, usize)>>],
    row_boundaries: &'a [usize],
    col_boundaries: &'a [usize],
    hbound: &'a BTreeMap<(usize, usize), ResolvedEdge>,
    vbound: &'a BTreeMap<(usize, usize), ResolvedEdge>,
    heights: &'a [Option<u32>],
}

impl PlanScaffold<'_> {
    fn out_col(&self, c: usize) -> usize {
        out_index(self.col_boundaries, c)
    }

    fn out_row(&self, r: usize) -> usize {
        out_index(self.row_boundaries, r)
    }

    fn band_borders(&self, r0: usize, r1: usize, c0: usize, c1: usize) -> IrCellBorders {
        cell_borders(self.hbound, self.vbound, r0, r1, c0, c1)
    }
}

/// Position inside the band loop: output row `o` spanning source rows
/// `r0..r1`, current lane `j` starting at source column `c0`, and `lane1`
/// the table's right edge in output lanes.
struct BandPos {
    o: usize,
    j: usize,
    r0: usize,
    r1: usize,
    c0: usize,
    lane1: usize,
}

/// A cell with no body and no fill of its own. Typst materializes one per
/// position no declared cell covers, so these are mergeable whitespace, not
/// real cells. A stroke on such a cell still pins the boundary it sits on —
/// strokes live in the boundary maps, not in the occupancy grid.
fn is_whitespace(cell: &IrTableCell) -> bool {
    cell.body.is_empty() && cell.vmerge == IrVMerge::None && cell.fill_hex.is_none()
}

/// A declared cell that starts a span (or occupies one track): the only kind
/// that cuts boundaries and occupies lanes. `Restart` marks a real cell's
/// span start; `Continue` marks the positions it covers.
fn is_declared(cell: &IrTableCell) -> bool {
    cell.vmerge != IrVMerge::Continue && !is_whitespace(cell)
}

fn planned_rows(
    grid: &IrGrid,
    scaffold: &PlanScaffold<'_>,
    lane0: usize,
    lane1: usize,
) -> Vec<PlannedRow> {
    let row_boundaries = scaffold.row_boundaries;
    let col_boundaries = scaffold.col_boundaries;
    let hbound = scaffold.hbound;
    let vbound = scaffold.vbound;
    let heights = scaffold.heights;
    let mut rows = Vec::with_capacity(row_boundaries.len() - 1);
    for (o, band) in row_boundaries.windows(2).enumerate() {
        let (r0, r1) = (band[0], band[1]);
        let mut cells = Vec::new();
        let mut j = lane0;
        while j < lane1 {
            let c0 = col_boundaries[j];
            let lane_borders = |jj: usize| {
                cell_borders(
                    hbound,
                    vbound,
                    r0,
                    r1,
                    col_boundaries[jj],
                    col_boundaries[jj + 1],
                )
            };
            let pos = BandPos {
                o,
                j,
                r0,
                r1,
                c0,
                lane1,
            };
            if let Some((mut planned, span)) = real_cells(grid, scaffold, &pos, &lane_borders)
                .or_else(|| covered_cells(grid, scaffold, &pos, &lane_borders))
            {
                cells.append(&mut planned);
                j += span;
                continue;
            }
            cells.push(PlannedCell {
                kind: PlannedCellKind::Empty,
                col_span: 1,
                row_span: 1,
                borders: lane_borders(j),
                fill_hex: None,
                zero_margins: true,
            });
            j += 1;
        }
        rows.push(PlannedRow {
            height_twips: heights.get(o).copied().flatten(),
            is_header: grid.rows.get(r0).is_some_and(|r| r.is_header),
            cells,
        });
    }
    rows
}

/// The real cell starting at `pos.c0`, planned as one cell plus `Filler`
/// markers for the extra lanes it covers. Returns the cells and the number
/// of lanes consumed.
fn real_cells(
    grid: &IrGrid,
    scaffold: &PlanScaffold<'_>,
    pos: &BandPos,
    lane_borders: &dyn Fn(usize) -> IrCellBorders,
) -> Option<(Vec<PlannedCell>, usize)> {
    let (row, index) = real_cell_starting(grid, pos.r0, pos.c0)?;
    let src = &grid.rows[row].cells[index];
    let lane_end = scaffold.out_col(pos.c0 + usize::try_from(src.colspan).unwrap_or(1) - 1) + 1;
    let span = lane_end.saturating_sub(pos.j).min(pos.lane1 - pos.j);
    let row_span =
        scaffold.out_row(pos.r0 + usize::try_from(src.rowspan).unwrap_or(1).saturating_sub(1)) + 1
            - pos.o;
    let cell_row_end = pos.r0 + usize::try_from(src.rowspan).unwrap_or(1);
    let cell_col_end = src.x + usize::try_from(src.colspan).unwrap_or(1);
    let mut cells = vec![PlannedCell {
        kind: PlannedCellKind::Real { row, index },
        col_span: u32::try_from(span).unwrap_or(u32::MAX),
        row_span: u32::try_from(row_span).unwrap_or(u32::MAX),
        borders: IrCellBorders {
            top: line_over(
                scaffold.hbound,
                pos.r0,
                src.x..cell_col_end,
                true,
                EdgeOwner::Inner,
            ),
            bottom: line_over(
                scaffold.hbound,
                cell_row_end,
                src.x..cell_col_end,
                true,
                EdgeOwner::Outer,
            ),
            left: line_over(
                scaffold.vbound,
                src.x,
                pos.r0..cell_row_end,
                false,
                EdgeOwner::Inner,
            ),
            right: line_over(
                scaffold.vbound,
                cell_col_end,
                pos.r0..cell_row_end,
                false,
                EdgeOwner::Outer,
            ),
        },
        fill_hex: None,
        zero_margins: false,
    }];
    for k in 1..span {
        cells.push(PlannedCell {
            kind: PlannedCellKind::Filler,
            col_span: 1,
            row_span: 1,
            borders: lane_borders(pos.j + k),
            fill_hex: src.fill_hex.clone(),
            zero_margins: true,
        });
    }
    Some((cells, span))
}

/// The declared cell covering `(r0, c0)` from above, planned as a `vMerge`
/// continuation that keeps its `gridSpan` plus `Filler` lane markers.
fn covered_cells(
    grid: &IrGrid,
    scaffold: &PlanScaffold<'_>,
    pos: &BandPos,
    lane_borders: &dyn Fn(usize) -> IrCellBorders,
) -> Option<(Vec<PlannedCell>, usize)> {
    let cell_id = scaffold
        .occupancy
        .get(pos.r0)
        .and_then(|row| row.get(pos.c0))
        .copied()
        .flatten()?;
    let cov = cell_by_id(grid, cell_id);
    let fill = cov.and_then(|c| c.fill_hex.clone());
    let cov_end = cov.map_or(pos.c0 + 1, |c| {
        c.x + usize::try_from(c.colspan).unwrap_or(1)
    });
    let cov_lane_end = scaffold.out_col(cov_end.saturating_sub(1)) + 1;
    let span = cov_lane_end
        .saturating_sub(pos.j)
        .max(1)
        .min(pos.lane1 - pos.j);
    let mut cells = vec![PlannedCell {
        kind: PlannedCellKind::VContinue,
        col_span: u32::try_from(span).unwrap_or(u32::MAX),
        row_span: 1,
        borders: scaffold.band_borders(
            pos.r0,
            pos.r1,
            pos.c0,
            scaffold.col_boundaries[pos.j + span],
        ),
        fill_hex: fill.clone(),
        zero_margins: true,
    }];
    for k in 1..span {
        cells.push(PlannedCell {
            kind: PlannedCellKind::Filler,
            col_span: 1,
            row_span: 1,
            borders: lane_borders(pos.j + k),
            fill_hex: fill.clone(),
            zero_margins: true,
        });
    }
    Some((cells, span))
}

fn max_col(grid: &IrGrid) -> usize {
    grid.rows
        .iter()
        .flat_map(|r| r.cells.iter())
        .map(|c| c.x + usize::try_from(c.colspan).unwrap_or(1))
        .max()
        .unwrap_or(0)
}

/// `occupancy[r][c]` = id of the declared cell covering `(r, c)`, following
/// row/col spans. `None` marks mergeable whitespace.
fn occupancy_grid(grid: &IrGrid, n_rows: usize, n_cols: usize) -> Vec<Vec<Option<(usize, usize)>>> {
    let mut occ = vec![vec![None; n_cols]; n_rows];
    for (r, row) in grid.rows.iter().enumerate() {
        for (index, cell) in row.cells.iter().enumerate() {
            if !is_declared(cell) {
                continue;
            }
            let r_end = (r + usize::try_from(cell.rowspan).unwrap_or(1)).min(n_rows);
            let c_end = (cell.x + usize::try_from(cell.colspan).unwrap_or(1)).min(n_cols);
            for occ_row in occ.iter_mut().take(r_end).skip(r) {
                for slot in occ_row.iter_mut().take(c_end).skip(cell.x) {
                    *slot = Some((r, index));
                }
            }
        }
    }
    occ
}

fn cell_by_id(grid: &IrGrid, id: (usize, usize)) -> Option<&IrTableCell> {
    grid.rows.get(id.0)?.cells.get(id.1)
}

/// Horizontal boundary `b` is cut when a grid `hline` sits on it, a declared
/// cell starts or ends there, or a stroke-only cell draws its top/bottom edge
/// on it — everything else can merge.
fn row_boundary_cuts(grid: &IrGrid, n_rows: usize) -> Vec<usize> {
    let mut cuts = BTreeSet::from([0, n_rows]);
    for line in &grid.hlines {
        cuts.insert(line.index.min(n_rows));
    }
    for (r, row) in grid.rows.iter().enumerate() {
        for cell in &row.cells {
            let end = r + usize::try_from(cell.rowspan).unwrap_or(1);
            if is_declared(cell) {
                cuts.insert(r);
                cuts.insert(end);
            } else {
                if cell.borders.top.is_some() {
                    cuts.insert(r);
                }
                if cell.borders.bottom.is_some() {
                    cuts.insert(end);
                }
            }
        }
    }
    cuts.iter().copied().filter(|&b| b <= n_rows).collect()
}

/// Vertical boundary `b` is cut by a `vline` or whenever the occupancy of the
/// tracks on either side differs in any row (a cell edge or the edge of an
/// empty run).
fn col_boundary_cuts(
    grid: &IrGrid,
    occupancy: &[Vec<Option<(usize, usize)>>],
    n_rows: usize,
    n_cols: usize,
) -> Vec<usize> {
    let mut cuts = BTreeSet::from([0, n_cols]);
    for line in &grid.vlines {
        cuts.insert(line.index.min(n_cols));
    }
    for row in &grid.rows {
        for cell in &row.cells {
            if cell.borders.left.is_some() {
                cuts.insert(cell.x.min(n_cols));
            }
            if cell.borders.right.is_some() {
                cuts.insert((cell.x + usize::try_from(cell.colspan).unwrap_or(1)).min(n_cols));
            }
        }
    }
    for b in 1..n_cols {
        for occ_row in occupancy.iter().take(n_rows) {
            if occ_row[b - 1] != occ_row[b] {
                cuts.insert(b);
                break;
            }
        }
    }
    cuts.iter().copied().collect()
}

fn out_index(boundaries: &[usize], track: usize) -> usize {
    boundaries.partition_point(|&b| b <= track) - 1
}

/// A cell's stroke on one side of a boundary plus whether the cell declared
/// that side explicitly — Typst gives a declared edge precedence over a
/// stroke the cell merely inherited from the grid's `stroke:` callback.
#[derive(Clone)]
struct EdgeStroke {
    border: IrBorder,
    declared: bool,
}

/// Which adjacent cell's edge won a boundary fold — `Outer` is the cell above
/// or to the left of the boundary, `Inner` the cell below or to the right.
/// Only the winning side emits the border in the OOXML output: painting the
/// winning stroke onto the losing cell's edge double-declares it, and an
/// undeclared neighbouring cell must not acquire a border it does not own.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum EdgeOwner {
    Outer,
    Inner,
}

struct ResolvedEdge {
    border: IrBorder,
    owner: EdgeOwner,
}

/// Fold the two cell strokes meeting on a boundary into the one Typst draws:
/// a declared outer edge beats an undeclared inner one, and on ties the
/// `inner` side wins — the lower cell's top edge for horizontal boundaries,
/// the right cell's left edge for vertical ones. A missing side folds to the
/// other.
fn fold_edge(outer: Option<&EdgeStroke>, inner: Option<&EdgeStroke>) -> Option<ResolvedEdge> {
    match (outer, inner) {
        (Some(o), Some(i)) if o.declared && !i.declared => Some(ResolvedEdge {
            border: o.border.clone(),
            owner: EdgeOwner::Outer,
        }),
        (_, Some(i)) => Some(ResolvedEdge {
            border: i.border.clone(),
            owner: EdgeOwner::Inner,
        }),
        (Some(o), None) => Some(ResolvedEdge {
            border: o.border.clone(),
            owner: EdgeOwner::Outer,
        }),
        (None, None) => None,
    }
}

/// Merge per-side cell candidates and the grid's explicit lines into the
/// winning stroke per boundary segment. Lines win outright and emit on the
/// inner side; otherwise the outer/inner candidates fold per [`fold_edge`].
/// `covered(boundary, track)` reports whether the segment lies inside a
/// spanning cell, where Typst draws no line at all.
fn resolve_boundary_map(
    lines: &[IrGridLine],
    outer: &BTreeMap<(usize, usize), EdgeStroke>,
    inner: &BTreeMap<(usize, usize), EdgeStroke>,
    covered: impl Fn(usize, usize) -> bool,
    track_limit: usize,
    horizontal: bool,
) -> BTreeMap<(usize, usize), ResolvedEdge> {
    let mut keys: BTreeSet<(usize, usize)> = outer.keys().chain(inner.keys()).copied().collect();
    let mut line_map: BTreeMap<(usize, usize), ResolvedEdge> = BTreeMap::new();
    for line in lines {
        for t in line.start..line.end.min(track_limit) {
            let key = if horizontal {
                (line.index, t)
            } else {
                (t, line.index)
            };
            line_map.insert(
                key,
                ResolvedEdge {
                    border: line.border.clone(),
                    owner: EdgeOwner::Inner,
                },
            );
            keys.insert(key);
        }
    }
    keys.into_iter()
        .filter(|key| !covered(key.0, key.1))
        .filter_map(|key| {
            line_map
                .get(&key)
                .map(|e| ResolvedEdge {
                    border: e.border.clone(),
                    owner: e.owner,
                })
                .or_else(|| fold_edge(outer.get(&key), inner.get(&key)))
                .map(|edge| (key, edge))
        })
        .collect()
}

/// `hbound[(b, c)]` = the resolved edge on horizontal boundary `b` across
/// column track `c`, from `hline`s and adjacent cell strokes folded the way
/// Typst resolves shared cell edges.
fn horizontal_boundary_map(
    grid: &IrGrid,
    occupancy: &[Vec<Option<(usize, usize)>>],
    n_cols: usize,
) -> BTreeMap<(usize, usize), ResolvedEdge> {
    let mut upper: BTreeMap<(usize, usize), EdgeStroke> = BTreeMap::new();
    let mut lower: BTreeMap<(usize, usize), EdgeStroke> = BTreeMap::new();
    for (r, row) in grid.rows.iter().enumerate() {
        for cell in &row.cells {
            let bottom = r + usize::try_from(cell.rowspan).unwrap_or(1);
            for c in cell.x..cell.x + usize::try_from(cell.colspan).unwrap_or(1) {
                if c >= n_cols {
                    continue;
                }
                if let Some(border) = &cell.borders.top {
                    lower.insert(
                        (r, c),
                        EdgeStroke {
                            border: border.clone(),
                            declared: cell.borders_declared.top,
                        },
                    );
                }
                if let Some(border) = &cell.borders.bottom {
                    upper.insert(
                        (bottom, c),
                        EdgeStroke {
                            border: border.clone(),
                            declared: cell.borders_declared.bottom,
                        },
                    );
                }
            }
        }
    }
    resolve_boundary_map(
        &grid.hlines,
        &upper,
        &lower,
        |b, c| {
            b > 0
                && occupancy
                    .get(b - 1)
                    .and_then(|row| row.get(c))
                    .copied()
                    .flatten()
                    .is_some_and(|up| {
                        occupancy
                            .get(b)
                            .and_then(|row| row.get(c))
                            .copied()
                            .flatten()
                            == Some(up)
                    })
        },
        n_cols,
        true,
    )
}

/// `vbound[(r, b)]` = the resolved edge on vertical boundary `b` across row
/// track `r`, from `vline`s and adjacent cell strokes folded the way Typst
/// resolves shared cell edges.
fn vertical_boundary_map(
    grid: &IrGrid,
    occupancy: &[Vec<Option<(usize, usize)>>],
    n_rows: usize,
) -> BTreeMap<(usize, usize), ResolvedEdge> {
    let mut left: BTreeMap<(usize, usize), EdgeStroke> = BTreeMap::new();
    let mut right: BTreeMap<(usize, usize), EdgeStroke> = BTreeMap::new();
    for (r, row) in grid.rows.iter().enumerate() {
        for cell in &row.cells {
            let right_b = cell.x + usize::try_from(cell.colspan).unwrap_or(1);
            for rr in r..r + usize::try_from(cell.rowspan).unwrap_or(1) {
                if rr >= n_rows {
                    continue;
                }
                if let Some(border) = &cell.borders.left {
                    right.insert(
                        (rr, cell.x),
                        EdgeStroke {
                            border: border.clone(),
                            declared: cell.borders_declared.left,
                        },
                    );
                }
                if let Some(border) = &cell.borders.right {
                    left.insert(
                        (rr, right_b),
                        EdgeStroke {
                            border: border.clone(),
                            declared: cell.borders_declared.right,
                        },
                    );
                }
            }
        }
    }
    resolve_boundary_map(
        &grid.vlines,
        &left,
        &right,
        |r, b| {
            b > 0
                && occupancy
                    .get(r)
                    .and_then(|row| row.get(b - 1))
                    .copied()
                    .flatten()
                    .is_some_and(|lft| {
                        occupancy
                            .get(r)
                            .and_then(|row| row.get(b))
                            .copied()
                            .flatten()
                            == Some(lft)
                    })
        },
        n_rows,
        false,
    )
}

/// Strongest stroke `side` owns on boundary `b` over `range`: tracks with no
/// resolved edge (neither neighbor stroked them) simply do not count, and only
/// segments the requested side won qualify — the losing side paints nothing.
/// Whole-edge OOXML borders cannot express a partial edge, so an edge that
/// owns at least one track emits its best owned stroke.
fn line_over(
    map: &BTreeMap<(usize, usize), ResolvedEdge>,
    b: usize,
    range: std::ops::Range<usize>,
    horizontal: bool,
    side: EdgeOwner,
) -> Option<IrBorder> {
    let mut best: Option<&IrBorder> = None;
    for t in range {
        let key = if horizontal { (b, t) } else { (t, b) };
        if let Some(edge) = map.get(&key)
            && edge.owner == side
            && best.is_none_or(|e| edge.border.sz_eighths > e.sz_eighths)
        {
            best = Some(&edge.border);
        }
    }
    best.cloned()
}

fn cell_borders(
    hbound: &BTreeMap<(usize, usize), ResolvedEdge>,
    vbound: &BTreeMap<(usize, usize), ResolvedEdge>,
    r0: usize,
    r1: usize,
    c0: usize,
    c1: usize,
) -> IrCellBorders {
    IrCellBorders {
        top: line_over(hbound, r0, c0..c1, true, EdgeOwner::Inner),
        bottom: line_over(hbound, r1, c0..c1, true, EdgeOwner::Outer),
        left: line_over(vbound, c0, r0..r1, false, EdgeOwner::Inner),
        right: line_over(vbound, c1, r0..r1, false, EdgeOwner::Outer),
    }
}

fn real_cell_starting(grid: &IrGrid, r: usize, c: usize) -> Option<(usize, usize)> {
    let row = grid.rows.get(r)?;
    row.cells
        .iter()
        .enumerate()
        .find(|(_, cell)| cell.x == c && is_declared(cell))
        .map(|(index, _)| (r, index))
}

/// Per out-row heights: the sum of the merged source rows' heights when all
/// are known (slot heights already rounded down at collection).
fn merged_row_heights(grid: &IrGrid, boundaries: &[usize]) -> Vec<Option<u32>> {
    boundaries
        .windows(2)
        .map(|band| {
            grid.rows[band[0]..band[1].min(grid.rows.len())]
                .iter()
                .try_fold(0_u32, |acc, row| acc.checked_add(row.height_twips?))
        })
        .collect()
}

/// Absolute lane widths: declared fixed/rel tracks resolve directly, `fr`
/// tracks split what remains, `auto` tracks share like a `1fr`.
fn lane_widths_twips(
    grid: &IrGrid,
    boundaries: &[usize],
    content_width_twips: Option<u32>,
) -> Option<Vec<u32>> {
    let content = f64::from(content_width_twips?);
    let n_cols = boundaries.last().copied()?;
    let sizes = &grid.col_sizes;
    if sizes.len() < n_cols {
        return None;
    }
    let fixed: f64 = sizes[..n_cols]
        .iter()
        .map(|s| match s {
            IrTrackSize::Fixed(t) | IrTrackSize::Rel { abs_twips: t, .. } => f64::from(*t),
            _ => 0.0,
        })
        .sum();
    let rel_share: f64 = sizes[..n_cols]
        .iter()
        .map(|s| match s {
            IrTrackSize::Rel { permille, .. } => content * f64::from(*permille) / 1000.0,
            _ => 0.0,
        })
        .sum();
    let fr_total: f64 = sizes[..n_cols]
        .iter()
        .map(|s| match s {
            IrTrackSize::Fr(w) => *w,
            IrTrackSize::Auto => 1.0,
            _ => 0.0,
        })
        .sum();
    let remaining = (content - fixed - rel_share).max(0.0);
    let track_twips: Vec<u32> = sizes[..n_cols]
        .iter()
        .map(|s| {
            let w = match s {
                IrTrackSize::Fixed(t) | IrTrackSize::Rel { abs_twips: t, .. } => f64::from(*t),
                IrTrackSize::Fr(w) => *w,
                IrTrackSize::Auto => 1.0,
            };
            if fr_total > 0.0 && matches!(s, IrTrackSize::Fr(_) | IrTrackSize::Auto) {
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "non-negative width share rounded down to twips"
                )]
                let t = (remaining * w / fr_total) as u32;
                t
            } else if let IrTrackSize::Rel {
                abs_twips,
                permille,
            } = s
            {
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "non-negative width share rounded down to twips"
                )]
                let t = (f64::from(*abs_twips) + content * f64::from(*permille) / 1000.0) as u32;
                t
            } else {
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "non-negative width share rounded down to twips"
                )]
                let t = w as u32;
                t
            }
        })
        .collect();
    Some(
        boundaries
            .windows(2)
            .map(|band| {
                track_twips[band[0]..band[1].min(track_twips.len())]
                    .iter()
                    .sum()
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{IrCellBorderFlags, IrTableCell, IrTableRow, IrVMerge};

    fn cell(x: usize, colspan: u32, rowspan: u32) -> IrTableCell {
        IrTableCell {
            body: vec![crate::types::IrBlock::Paragraph(
                crate::types::IrParagraph {
                    inlines: vec![crate::types::IrInline::Text(
                        tydocx_ooxml::document::Run::new("x"),
                    )],
                    style: None,
                    alignment: None,
                    par: crate::types::IrParStyle::default(),
                    suppress_indent: false,
                    hanging_indent: None,
                    left_indent: None,
                    right_indent: None,
                    keep_lines: false,
                    keep_next: false,
                    code_block: false,
                    horizontal_rule: false,
                    tab_stops: Vec::new(),
                    spacing_before: None,
                    spacing_after: None,
                    bookmark: None,
                },
            )],
            x,
            colspan,
            rowspan,
            vmerge: IrVMerge::None,
            fill_hex: None,
            vertical_align: None,
            inset_twips: None,
            borders: IrCellBorders::default(),
            borders_declared: IrCellBorderFlags::default(),
        }
    }

    fn grid(
        rows: Vec<Vec<IrTableCell>>,
        row_sizes: Vec<IrTrackSize>,
        col_sizes: Vec<IrTrackSize>,
    ) -> IrGrid {
        IrGrid {
            rows: rows
                .into_iter()
                .map(|cells| IrTableRow {
                    cells,
                    height_twips: None,
                    is_header: false,
                })
                .collect(),
            column_widths_pct: None,
            borders: None,
            row_sizes,
            col_sizes,
            hlines: Vec::new(),
            vlines: Vec::new(),
        }
    }

    fn border(sz: u32) -> IrBorder {
        IrBorder {
            sz_eighths: sz,
            color_hex: None,
            dashed: false,
        }
    }

    #[test]
    fn dense_grid_is_identity() {
        let g = grid(
            vec![
                vec![cell(0, 1, 1), cell(1, 1, 1)],
                vec![cell(0, 1, 1), cell(1, 1, 1)],
            ],
            vec![IrTrackSize::Auto; 2],
            vec![IrTrackSize::Fr(1.0); 2],
        );
        let plan = plan_grid(&g, Some(1200));
        assert!(!plan.changed);
    }

    #[test]
    fn empty_rows_rle_merge() {
        // header row + 10 empty slots + footer row
        let mut rows: Vec<Vec<IrTableCell>> = Vec::new();
        rows.push(vec![cell(0, 1, 1)]);
        rows.extend(std::iter::repeat_with(Vec::new).take(10));
        rows.push(vec![cell(0, 1, 1)]);
        let mut g = grid(
            rows,
            vec![IrTrackSize::Fr(1.0); 12],
            vec![IrTrackSize::Fr(1.0)],
        );
        for (i, row) in g.rows.iter_mut().enumerate() {
            row.height_twips = Some(100 + u32::try_from(i).unwrap_or(0));
        }
        let plan = plan_grid(&g, Some(1200));
        assert!(plan.changed);
        let t = &plan.tables[0];
        // boundaries: 0,1 (header end), 11 (footer start), 12 → 3 rows
        assert_eq!(t.rows.len(), 3);
        assert_eq!(t.rows[1].height_twips, Some(1055));
        assert_eq!(t.rows[1].cells.len(), 1);
        assert_eq!(t.rows[1].cells[0].kind, PlannedCellKind::Empty);
    }

    #[test]
    fn rowspan_crossing_a_line_becomes_vmerge() {
        // A cell spanning two rows that an hline cuts in half must surface
        // as a vMerge restart + continuation pair.
        let mut g = grid(
            vec![vec![cell(0, 1, 2), cell(1, 1, 1)], vec![cell(1, 1, 1)]],
            vec![IrTrackSize::Fr(1.0); 2],
            vec![IrTrackSize::Fr(1.0); 2],
        );
        g.hlines.push(IrGridLine {
            index: 1,
            start: 0,
            end: 2,
            border: border(4),
        });
        let plan = plan_grid(&g, Some(900));
        let t = &plan.tables[0];
        assert_eq!(t.rows.len(), 2);
        assert_eq!(
            t.rows[0].cells[0].kind,
            PlannedCellKind::Real { row: 0, index: 0 }
        );
        assert_eq!(t.rows[0].cells[0].row_span, 2);
        assert_eq!(
            t.rows[0].cells[1].kind,
            PlannedCellKind::Real { row: 0, index: 1 }
        );
        assert_eq!(t.rows[1].cells[0].kind, PlannedCellKind::VContinue);
        assert_eq!(
            t.rows[1].cells[1].kind,
            PlannedCellKind::Real { row: 1, index: 0 }
        );
    }

    #[test]
    fn colspan_over_merged_lanes_needs_no_filler() {
        // Columns 1-2 merge into one lane because their occupancy is
        // identical across rows; an event spanning both is a single cell.
        let mut g = grid(
            vec![vec![cell(0, 1, 1), cell(1, 2, 1)], Vec::new()],
            vec![IrTrackSize::Auto; 2],
            vec![IrTrackSize::Fr(1.0); 3],
        );
        g.rows[0].cells[1].x = 1;
        let plan = plan_grid(&g, Some(900));
        let t = &plan.tables[0];
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[0].cells.len(), 2);
        assert_eq!(t.rows[0].cells[1].col_span, 1);
        assert_eq!(t.rows[1].cells[0].kind, PlannedCellKind::Empty);
    }

    #[test]
    fn title_spanning_distinct_lanes_yields_fillers() {
        // A title cell covering four lanes with distinct occupancy gets a
        // `gridSpan` of 4 plus three Filler markers the lowerer skips.
        let g = grid(
            vec![
                vec![cell(0, 4, 1)],
                vec![cell(0, 1, 1), cell(1, 1, 1), cell(2, 1, 1), cell(3, 1, 1)],
            ],
            vec![IrTrackSize::Auto; 2],
            vec![IrTrackSize::Fr(1.0); 4],
        );
        let plan = plan_grid(&g, Some(900));
        let t = &plan.tables[0];
        assert_eq!(t.rows[0].cells.len(), 4);
        assert_eq!(t.rows[0].cells[0].col_span, 4);
        assert!(
            t.rows[0].cells[1..]
                .iter()
                .all(|c| c.kind == PlannedCellKind::Filler)
        );
    }

    #[test]
    fn hline_cuts_and_surfaces_as_border() {
        let mut g = grid(
            vec![vec![cell(0, 2, 1)], Vec::new(), vec![cell(0, 2, 1)]],
            vec![IrTrackSize::Fr(1.0); 3],
            vec![IrTrackSize::Fr(1.0); 2],
        );
        g.hlines.push(IrGridLine {
            index: 2,
            start: 0,
            end: 2,
            border: border(4),
        });
        let plan = plan_grid(&g, Some(600));
        let t = &plan.tables[0];
        assert_eq!(t.rows.len(), 3);
        assert_eq!(t.rows[1].cells[0].borders.bottom, None);
        assert_eq!(
            t.rows[2].cells[0]
                .borders
                .top
                .as_ref()
                .map(|b| b.sz_eighths),
            Some(4)
        );
    }

    #[test]
    fn over_limit_splits_at_lanes() {
        // 70 independent columns (each distinct occupancy) → two tables.
        let cells: Vec<IrTableCell> = (0..70).map(|x| cell(x, 1, 1)).collect();
        let g = grid(
            vec![cells],
            vec![IrTrackSize::Auto],
            vec![IrTrackSize::Auto; 70],
        );
        let plan = plan_grid(&g, None);
        assert!(plan.changed);
        assert_eq!(plan.tables.len(), 2);
        assert_eq!(plan.tables[0].rows[0].cells.len(), 63);
        assert_eq!(plan.tables[1].rows[0].cells.len(), 7);
    }

    #[test]
    fn lane_widths_share_fr() {
        let g = grid(
            vec![vec![cell(0, 1, 1), cell(1, 1, 1)]],
            vec![IrTrackSize::Auto],
            vec![IrTrackSize::Fr(1.0), IrTrackSize::Fr(3.0)],
        );
        let plan = plan_grid(&g, Some(1000));
        let t = &plan.tables[0];
        assert_eq!(t.col_widths_twips, Some(vec![250, 750]));
    }

    #[test]
    fn height_unknown_when_unmeasured() {
        let g = grid(
            vec![vec![cell(0, 1, 1)], Vec::new()],
            vec![IrTrackSize::Auto; 2],
            vec![IrTrackSize::Fr(1.0)],
        );
        let plan = plan_grid(&g, Some(100));
        assert!(plan.changed);
        assert_eq!(
            plan.tables[0]
                .rows
                .iter()
                .filter(|r| r.height_twips.is_none())
                .count(),
            2
        );
    }
}
