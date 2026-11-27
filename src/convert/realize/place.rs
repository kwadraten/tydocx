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

use crate::ir::{IrAnchor, IrBlock, IrDocument, IrInline, IrShapeKind, pt_to_emu};
use typst_layout::PagedDocument;
use typst_library::introspection::Tag;
use typst_library::layout::{Frame, FrameItem, Point};

use super::collect::location_hash64;
use crate::convert::frames::{ItemBounds, visit_frame_items};
use crate::convert::table_align::paint_to_shd_hex;

/// What the page paints under a placed unit, insofar as the OOXML lowering
/// needs to know: a solid `rect` whose fill a box above must carry to cover
/// what Typst shows through, or an image no flat colour can stand in for.
enum Underlay {
    Fill(String),
    Image,
}

/// Classify a place body whose own paint is a single flat layer under
/// whatever paints later.
fn place_underlay(body: &[IrBlock]) -> Option<Underlay> {
    match body {
        [
            IrBlock::Shape {
                kind: IrShapeKind::Rectangle,
                fill_hex: Some(hex),
                ..
            },
        ] => Some(Underlay::Fill(hex.clone())),
        [IrBlock::Paragraph(para)] if matches!(para.inlines.as_slice(), [IrInline::Image(_)]) => {
            Some(Underlay::Image)
        }
        _ => None,
    }
}

/// Fill `anchor` on every `IrBlock::Place` whose `Location` hash matches a
/// rendered placement in the paged document.
pub fn patch_places(doc: &mut IrDocument, paged: &PagedDocument) {
    let mut wanted = HashSet::new();
    let mut underlays = HashMap::new();
    for_each_place(&doc.body, &mut |hash, body| {
        wanted.insert(hash);
        if let Some(underlay) = place_underlay(body) {
            underlays.insert(hash, underlay);
        }
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
                            backdrop_hex: None,
                        });
                    }
                }
            },
        );
        // The box's underlay colour: a solid `page(fill:)` when declared,
        // else paper white; a non-solid fill has no shape-fill stand-in.
        let page_backdrop = match page.fill_or_transparent() {
            None => Some(String::from("FFFFFF")),
            Some(paint) => paint_to_shd_hex(&paint),
        };
        scan_placed_events(
            &page.frame,
            &wanted,
            &underlays,
            page_backdrop.as_deref(),
            &mut geometry,
        );
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
fn scan_placed_events(
    frame: &Frame,
    wanted: &HashSet<u64>,
    underlays: &HashMap<u64, Underlay>,
    page_backdrop: Option<&str>,
    geometry: &mut HashMap<u64, IrAnchor>,
) {
    let mut events = Vec::new();
    collect_events(frame, Point::zero(), 0, &mut events);
    // Non-float places measured on this page, in paint order — a new anchor
    // paints over them, so the latest one covering its bounds is its underlay.
    let mut placed: Vec<(u64, IrAnchor)> = Vec::new();
    let mut brackets: Vec<(usize, usize)> = Vec::new();
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
        let mut bounds = ItemBounds::default();
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
        if let Some(mut anchor) = anchor_from(
            &bounds,
            events[end_index..]
                .iter()
                .any(|(_, _, it)| matches!(it, FrameItem::Text(_))),
        ) {
            // Earlier content that is not itself a measured place and that
            // overlaps this anchor — the underlay the paper fallback must not
            // paint over.
            let under_content = events[..i].iter().enumerate().any(|(k, &(_, pos, it))| {
                if matches!(it, FrameItem::Tag(_)) || brackets.iter().any(|&(s, e)| k > s && k < e)
                {
                    return false;
                }
                let mut b = ItemBounds::default();
                b.add(pos, it);
                bounds
                    .rect()
                    .is_some_and(|anchor_rect| b.rect().is_some_and(|r| overlaps(r, anchor_rect)))
            });
            let fallback = if under_content { None } else { page_backdrop };
            anchor.backdrop_hex = backdrop_hex(&placed, underlays, &anchor, fallback);
            placed.push((hash, anchor.clone()));
            brackets.push((i, end_index));
            geometry.insert(hash, anchor);
        }
    }
}

/// The colour a transparent `wps:txbx` must carry so the unit covers what
/// paints under it: the latest covering fill rect wins, an image keeps the
/// box unfilled, unfilled text boxes let the search continue further down.
fn backdrop_hex(
    placed: &[(u64, IrAnchor)],
    underlays: &HashMap<u64, Underlay>,
    anchor: &IrAnchor,
    page_backdrop: Option<&str>,
) -> Option<String> {
    for (hash, earlier) in placed.iter().rev() {
        if !covers(earlier, anchor) {
            continue;
        }
        match underlays.get(hash) {
            Some(Underlay::Fill(hex)) => return Some(hex.clone()),
            Some(Underlay::Image) => return None,
            None => {}
        }
    }
    page_backdrop.map(str::to_owned)
}

/// Two `(x, y, w, h)` pt rects intersect.
fn overlaps(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> bool {
    a.0 < b.0 + b.2 && b.0 < a.0 + a.2 && a.1 < b.1 + b.3 && b.1 < a.1 + a.3
}

/// `outer` fully covers `inner` (page-absolute EMU rects).
fn covers(outer: &IrAnchor, inner: &IrAnchor) -> bool {
    outer.x_emu <= inner.x_emu
        && outer.y_emu <= inner.y_emu
        && outer
            .x_emu
            .saturating_add(i64::try_from(outer.width_emu).unwrap_or(i64::MAX))
            >= inner
                .x_emu
                .saturating_add(i64::try_from(inner.width_emu).unwrap_or(i64::MAX))
        && outer
            .y_emu
            .saturating_add(i64::try_from(outer.height_emu).unwrap_or(i64::MAX))
            >= inner
                .y_emu
                .saturating_add(i64::try_from(inner.height_emu).unwrap_or(i64::MAX))
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

/// The union bounds of a place's items as an `IrAnchor`, when non-empty.
fn anchor_from(bounds: &ItemBounds, behind_doc: bool) -> Option<IrAnchor> {
    let (x, y, width, height) = bounds.rect()?;
    Some(IrAnchor {
        x_emu: emu_i64(x),
        y_emu: emu_i64(y),
        width_emu: pt_to_emu(width),
        height_emu: pt_to_emu(height),
        behind_doc,
        backdrop_hex: None,
    })
}

/// Location hashes of every `IrBlock::Place` in the document body — the set
/// the furniture classifier must treat as inner content, not page furniture.
pub(crate) fn place_location_hashes(blocks: &[IrBlock]) -> HashSet<u64> {
    let mut hashes = HashSet::new();
    for_each_place(blocks, &mut |hash, _| {
        hashes.insert(hash);
    });
    hashes
}

/// Visit the `location_hash` of every `IrBlock::Place` in the IR, with its
/// body so the caller can classify what the place paints.
fn for_each_place(blocks: &[IrBlock], f: &mut impl FnMut(u64, &[IrBlock])) {
    for block in blocks {
        match block {
            IrBlock::Place {
                location_hash,
                body,
                ..
            } => {
                if let Some(hash) = location_hash {
                    f(*hash, body);
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
            | IrBlock::Shape { body, .. }
            | IrBlock::Columns { body, .. } => for_each_place(body, f),
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
                    *anchor = Some(geo.clone());
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
            | IrBlock::Shape { body, .. }
            | IrBlock::Columns { body, .. } => apply_anchors(body, geometry),
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
