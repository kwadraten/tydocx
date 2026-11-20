//! `#place` → `wp:anchor` geometry.
//!
//! Typst emits each rendered float as a frame group whose `parent` is the
//! `PlaceElem`'s introspection `Location`. A non-floating `#place` instead
//! leaves its `Tag::Start`/`Tag::End` brackets in the flow frame with the
//! placed items laid out between them at their absolute positions — the
//! place is overlaid, it displaces nothing. This pass measures both forms
//! in the paged frames — page-absolute position and extent — and stamps the
//! result onto `IrBlock::Place::anchor`. The lowerer turns a measured place
//! into a `wp:anchor`; an unmeasured one keeps the flow fallback.

use std::collections::{HashMap, HashSet};

use typort_ir::{IrAnchor, IrBlock, IrDocument, pt_to_emu};
use typst_layout::PagedDocument;
use typst_library::introspection::Tag;
use typst_library::layout::{Abs, Frame, FrameItem, Point};

use super::collect::location_hash64;
use crate::convert::frames::visit_frame_items;

/// Fill `anchor` on every `IrBlock::Place` whose `Location` hash matches a
/// rendered placement in the paged document.
pub fn patch_places(doc: &mut IrDocument, paged: &PagedDocument) {
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
        // absolute positions the walks report are already page-absolute.
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
                            behind_doc: false,
                        });
                    }
                }
            },
        );
        scan_placed_events(&page.frame, &wanted, &mut geometry);
    }
    if geometry.is_empty() {
        return;
    }
    apply_anchors(&mut doc.body, &geometry);
}

/// A depth-first event list entry: the item's depth in the frame tree, its
/// absolute position, and the item itself.
type FrameEvent<'a> = (usize, Point, &'a FrameItem);

/// Scan one page's frame for non-floating `PlaceElem` tag brackets. The
/// items between a `Start("place")`/`End` pair at the same frame depth are
/// the placed content; their union bounds are the anchor's extent. Word has
/// three layers — `behindDoc` anchors, the body text layer, then front
/// anchors — and Typst's paint order maps onto them as "behind when any
/// text still paints after the place on this page" (images and shapes that
/// paint after only other drawings stay in front, layered among the anchors
/// by document order).
fn scan_placed_events(frame: &Frame, wanted: &HashSet<u64>, geometry: &mut HashMap<u64, IrAnchor>) {
    let mut events = Vec::new();
    collect_events(frame, Point::zero(), 0, &mut events);
    for (i, &(depth, _, item)) in events.iter().enumerate() {
        let FrameItem::Tag(Tag::Start(elem, _)) = item else {
            continue;
        };
        if elem.elem().name() != "place" {
            continue;
        }
        let Some(location) = elem.location() else {
            continue;
        };
        let hash = location_hash64(location);
        if !wanted.contains(&hash) || geometry.contains_key(&hash) {
            continue;
        }
        // Collect the union bounds of items at the same depth until the
        // matching `Tag::End`. A shallower item means the frame ended
        // without the place's closing tag — an unusable bracket.
        let mut bounds = Bounds::default();
        let mut end = None;
        for (j, &(item_depth, position, inner)) in events.iter().enumerate().skip(i + 1) {
            if item_depth < depth {
                break;
            }
            if item_depth == depth {
                if let FrameItem::Tag(Tag::End(end_location, ..)) = inner
                    && *end_location == location
                {
                    end = Some(j);
                    break;
                }
                bounds.add(position, inner);
            }
        }
        let Some(end_index) = end else {
            continue;
        };
        if let Some(anchor) = bounds.anchor(
            events[end_index..]
                .iter()
                .any(|(_, _, it)| matches!(it, FrameItem::Text(_))),
        ) {
            geometry.insert(hash, anchor);
        }
    }
}

fn collect_events<'a>(
    frame: &'a Frame,
    offset: Point,
    depth: usize,
    out: &mut Vec<FrameEvent<'a>>,
) {
    for (position, item) in frame.items() {
        let absolute = Point::new(offset.x + position.x, offset.y + position.y);
        out.push((depth, absolute, item));
        if let FrameItem::Group(group) = item {
            collect_events(&group.frame, absolute, depth + 1, out);
        }
    }
}

/// Union bounding box of placed items, in page points.
#[derive(Default)]
struct Bounds {
    min: Option<Point>,
    max: Option<Point>,
}

impl Bounds {
    fn add(&mut self, position: Point, item: &FrameItem) {
        match item {
            FrameItem::Group(group) => {
                self.extend(position);
                self.extend(position + Point::new(group.frame.size().x, group.frame.size().y));
            }
            FrameItem::Text(text) => {
                // `bbox` is frame-oriented (baseline origin, y-down after the
                // item's own coordinate flip), so `position + min` is the
                // ink's top-left.
                let rect = text.bbox();
                self.extend(position + rect.min);
                self.extend(position + rect.max);
            }
            FrameItem::Shape(shape, _) => {
                let rect = shape.bbox(true);
                self.extend(position + rect.min);
                self.extend(position + rect.max);
            }
            FrameItem::Image(_, size, _) => {
                self.extend(position);
                self.extend(position + Point::new(size.x, size.y));
            }
            FrameItem::Link(..) | FrameItem::Tag(_) => {}
        }
    }

    fn extend(&mut self, point: Point) {
        self.min = Some(self.min.map_or(point, |min| min.min(point)));
        self.max = Some(self.max.map_or(point, |max| max.max(point)));
    }

    fn anchor(&self, behind_doc: bool) -> Option<IrAnchor> {
        let (min, max) = (self.min?, self.max?);
        let width = max.x - min.x;
        let height = max.y - min.y;
        if width <= Abs::zero() || height <= Abs::zero() {
            return None;
        }
        Some(IrAnchor {
            x_emu: emu_i64(min.x.to_pt()),
            y_emu: emu_i64(min.y.to_pt()),
            width_emu: pt_to_emu(width.to_pt()),
            height_emu: pt_to_emu(height.to_pt()),
            behind_doc,
        })
    }
}

/// Visit the `location_hash` of every `IrBlock::Place` in the IR.
fn for_each_place(blocks: &[IrBlock], f: &mut impl FnMut(u64)) {
    for block in blocks {
        match block {
            IrBlock::Place {
                location_hash,
                body,
                ..
            } => {
                if let Some(hash) = location_hash {
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
                location_hash,
                anchor,
                body,
                ..
            } => {
                if let Some(geo) = location_hash.and_then(|hash| geometry.get(&hash)) {
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
