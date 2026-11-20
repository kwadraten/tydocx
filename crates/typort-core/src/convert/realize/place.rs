//! M3: `#place(.., float: true)` → `wp:anchor` geometry.
//!
//! Typst emits each rendered float as a frame group whose `parent` is the
//! `PlaceElem`'s introspection `Location`. This pass measures those groups in
//! the paged frames — page-absolute position and extent — and stamps the
//! result onto `IrBlock::Place::anchor`. The lowerer turns a measured float
//! into a `wp:anchor`; an unmeasured one keeps the flow fallback.

use std::collections::{HashMap, HashSet};

use typort_ir::{IrAnchor, IrBlock, IrDocument, pt_to_emu};
use typst_layout::PagedDocument;
use typst_library::layout::{FrameItem, Point};

use super::collect::location_hash64;
use crate::convert::frames::visit_frame_items;

/// Fill `anchor` on every floating `IrBlock::Place` whose `Location` hash
/// matches a rendered float group in the paged document.
pub fn patch_floats(doc: &mut IrDocument, paged: &PagedDocument) {
    let mut wanted = HashSet::new();
    for_each_place(&doc.body, &mut |hash| {
        wanted.insert(hash);
    });
    if wanted.is_empty() {
        return;
    }
    let mut geometry = HashMap::new();
    for page in paged.pages() {
        // The page frame's origin is the paper's top-left corner, so the
        // absolute positions the walk reports are already page-absolute.
        visit_frame_items(
            &page.frame,
            false,
            &mut |absolute: Point, item: &FrameItem| {
                if let FrameItem::Group(group) = item
                    && let Some(parent) = group.parent
                {
                    let hash = location_hash64(parent.location);
                    if wanted.contains(&hash) {
                        geometry.entry(hash).or_insert_with(|| IrAnchor {
                            x_emu: emu_i64(absolute.x.to_pt()),
                            y_emu: emu_i64(absolute.y.to_pt()),
                            width_emu: pt_to_emu(group.frame.width().to_pt()),
                            height_emu: pt_to_emu(group.frame.height().to_pt()),
                        });
                    }
                }
            },
        );
    }
    if geometry.is_empty() {
        return;
    }
    apply_anchors(&mut doc.body, &geometry);
}

/// Visit the `location_hash` of every floating `IrBlock::Place` in the IR.
fn for_each_place(blocks: &[IrBlock], f: &mut impl FnMut(u64)) {
    for block in blocks {
        match block {
            IrBlock::Place {
                float,
                location_hash,
                body,
                ..
            } => {
                if *float && let Some(hash) = location_hash {
                    f(*hash);
                }
                for_each_place(body, f);
            }
            IrBlock::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        for_each_place(&cell.body, f);
                    }
                }
            }
            IrBlock::Grid(grid) => {
                for row in &grid.rows {
                    for cell in &row.cells {
                        for_each_place(&cell.body, f);
                    }
                }
            }
            IrBlock::Figure { body, .. }
            | IrBlock::Quote { body }
            | IrBlock::Shape { body, .. } => for_each_place(body, f),
            IrBlock::List(list) => {
                for item in &list.items {
                    for_each_place(&item.body, f);
                }
            }
            IrBlock::Enum(enumer) => {
                for item in &enumer.items {
                    for_each_place(&item.body, f);
                }
            }
            _ => {}
        }
    }
}

fn apply_anchors(blocks: &mut [IrBlock], geometry: &HashMap<u64, IrAnchor>) {
    for block in blocks {
        match block {
            IrBlock::Place {
                float,
                location_hash,
                anchor,
                body,
                ..
            } => {
                if *float && let Some(geo) = location_hash.and_then(|hash| geometry.get(&hash)) {
                    *anchor = Some(*geo);
                }
                apply_anchors(body, geometry);
            }
            IrBlock::Table(table) => {
                for row in &mut table.rows {
                    for cell in &mut row.cells {
                        apply_anchors(&mut cell.body, geometry);
                    }
                }
            }
            IrBlock::Grid(grid) => {
                for row in &mut grid.rows {
                    for cell in &mut row.cells {
                        apply_anchors(&mut cell.body, geometry);
                    }
                }
            }
            IrBlock::Figure { body, .. }
            | IrBlock::Quote { body }
            | IrBlock::Shape { body, .. } => apply_anchors(body, geometry),
            IrBlock::List(list) => {
                for item in &mut list.items {
                    apply_anchors(&mut item.body, geometry);
                }
            }
            IrBlock::Enum(enumer) => {
                for item in &mut enumer.items {
                    apply_anchors(&mut item.body, geometry);
                }
            }
            _ => {}
        }
    }
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "page coordinates are small pt values"
)]
fn emu_i64(pt: f64) -> i64 {
    (pt * 12_700.0).round() as i64
}
