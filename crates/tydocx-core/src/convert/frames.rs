//! Shared depth-first traversal of paged frames.

use typst::layout::{Frame, FrameItem, GroupItem, Point};
use typst_library::introspection::Tag;

/// Visit frame items in paint order, reporting each item's absolute position.
///
/// When `skip_curve_groups` is true, groups containing curves are omitted with
/// their descendants. Recovery uses this for drawing canvases whose text and
/// lines are already represented by a rasterized image.
pub(super) fn visit_frame_items<'a>(
    frame: &'a Frame,
    skip_curve_groups: bool,
    visitor: &mut impl FnMut(Point, &'a FrameItem),
) {
    visit_frame_items_at(frame, Point::zero(), skip_curve_groups, visitor);
}

fn visit_frame_items_at<'a>(
    frame: &'a Frame,
    offset: Point,
    skip_curve_groups: bool,
    visitor: &mut impl FnMut(Point, &'a FrameItem),
) {
    for (position, item) in frame.items() {
        let absolute = Point::new(offset.x + position.x, offset.y + position.y);
        if let FrameItem::Group(group) = item
            && skip_curve_groups
            && super::image::frame_has_curve(&group.frame)
        {
            continue;
        }
        visitor(absolute, item);
        if let FrameItem::Group(group) = item {
            visit_frame_items_at(&group.frame, absolute, skip_curve_groups, visitor);
        }
    }
}

/// Union bounding box of frame items, in page points.
///
/// Shared by `#place` measurement and page-furniture extraction: both walk
/// frame items between tag brackets and need the painted extent of whatever
/// content rendered there.
#[derive(Default)]
pub(super) struct ItemBounds {
    min: Option<Point>,
    max: Option<Point>,
}

impl ItemBounds {
    pub(super) fn add(&mut self, position: Point, item: &FrameItem) {
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

    /// `(top_left, size)` in pt, when the union has positive area.
    pub(super) fn rect(&self) -> Option<(f64, f64, f64, f64)> {
        let (min, max) = (self.min?, self.max?);
        let width = max.x - min.x;
        let height = max.y - min.y;
        if width <= typst::layout::Abs::zero() || height <= typst::layout::Abs::zero() {
            return None;
        }
        Some((min.x.to_pt(), min.y.to_pt(), width.to_pt(), height.to_pt()))
    }
}

/// Whether a frame group is a footnote entry: it has a parent (the
/// marker location) and directly contains a `Tag::Start("entry")`.
/// (Outline entries carry an `"entry"` tag too, but their groups have
/// no parent, so they never match here.) Font and run-style detectors
/// use this to exclude footnote text — it renders at 0.85em and must
/// not drive body-size detection.
pub(super) fn is_footnote_entry_group(group: &GroupItem) -> bool {
    group.parent.is_some()
        && group.frame.items().any(|(_, item)| {
            matches!(item, FrameItem::Tag(Tag::Start(elem, _)) if elem.elem().name() == "entry")
        })
}
