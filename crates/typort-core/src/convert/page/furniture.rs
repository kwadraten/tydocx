//! Page furniture → `wp:anchor` drawings in header/footer parts (C2/C3).
//!
//! Typst's page finalize (`typst-layout src/pages/finalize.rs`) pushes five
//! layers into each page frame, in paint order: `background` (wrapped in an
//! `ArtifactKind::Background` tag bracket), `header` (`ArtifactKind::Header`),
//! the inner body frame, `footer` (`ArtifactKind::Footer`), then `foreground`
//! (unwrapped). Each marginal is either inlined flat into the page frame or
//! pushed as a `FrameItem::Group` at a fixed position — `(margin.left, 0)`
//! for the header, `(margin.left, page.h - h)` for the footer, the bleed
//! origin at full page size for background/foreground, and
//! `(margin.left, margin.top)` for the inner frame.
//!
//! This module classifies every top-level item of each page's frame into its
//! layer, measures the furniture layers the same way `realize/place.rs`
//! measures `#place`, and turns the measured units into `AnchoredFloat`s.
//! Those anchors ride inside header/footer parts (`default`/`even`/`first`
//! slots — exactly the three shapes `ZoneParts` can express) because a
//! `wp:anchor` in a header/footer part paints on every page that references
//! the part, which is the only OOXML construct that repeats page-positioned
//! content.

use std::ops::Range;

use typort_ir::pt_to_emu;
use typort_ooxml::document::{
    AnchorWrap, AnchoredContent, AnchoredFloat, BlockElement, Document, HeaderFooter, ImageData,
    ImageFormat, InlineElement, Paragraph, RelFrom, Run,
};
use typst_layout::{Page, PagedDocument};
use typst_library::introspection::{Location, Tag};
use typst_library::layout::{Frame, FrameItem, GroupItem, Point, Size};
use typst_library::pdf::{ArtifactElem, ArtifactKind};
use typst_library::text::TextItem;
use typst_library::visualize::{Geometry, Image, ImageKind, Paint};

use super::super::frames::ItemBounds;
use super::margin::{MarginZone, MarginsPt};
use super::run_style::effective_weight;
use crate::compat::{Degradation, DegradationAction};
use crate::convert::image::hash_bytes;

/// Tolerance (pt) for matching marginal positions/sizes on the page frame.
const MARGINAL_TOLERANCE_PT: f64 = 2.0;

/// The layers of a page frame, in finalize's paint order.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Zone {
    Background,
    Header,
    Footer,
    Foreground,
    /// Inner body content and leading page tags — never a furniture zone.
    Inner,
}

/// Which furniture slots the source declared — gates the foreground marker
/// heuristic (a `place` tag at the page origin is only foreground content
/// when `page(foreground:)` was actually declared; a zero-margin body place
/// colliding with the origin is the accepted false positive).
#[derive(Clone, Copy, Default)]
pub struct DeclaredFurniture {
    pub foreground: bool,
    pub background: bool,
}

/// One measured furniture unit — an `AnchoredFloat` plus the signature used
/// to classify its page-variation shape.
pub struct FurnAnchor {
    pub float: AnchoredFloat,
    /// Stable content+geometry signature for variation classification.
    pub sig: String,
}

/// Kinds of painted furniture content that could not be represented, rolled
/// into the `page-foreground` degradation message.
#[derive(Default)]
pub struct DropCounts {
    /// Non-rect shapes (curves, stroke-only shapes) and images inside mixed
    /// text units.
    pub shapes: u32,
    /// Link boxes.
    pub links: u32,
    /// Non-representable images (PDF) inside furniture.
    pub images: u32,
    /// `#place` bodies that are lone tables (text kept, grid lost).
    pub lone_tables: u32,
    /// Non-solid `page(fill:)` (gradient/tiling/pattern).
    pub non_solid_fill: u32,
    /// Frame groups carrying a `rotate`/`scale`/`skew` transform or a clip
    /// curve — the text box keeps the content but loses the transform.
    pub transforms: u32,
}

/// The furniture layers of one rendered page.
#[derive(Default)]
pub struct PageFurniture {
    /// Top-level item index ranges excluded from margin-zone text extraction
    /// (the background/foreground layers — flat bracket ranges or the group
    /// index of a pushed marginal frame).
    pub excluded_top: Vec<Range<usize>>,
    /// `#place` units measured inside the header zone.
    pub header: Vec<FurnAnchor>,
    /// `#place` units measured inside the footer zone.
    pub footer: Vec<FurnAnchor>,
    /// Anchored units measured in the background layer.
    pub background: Vec<FurnAnchor>,
    /// Anchored units measured in the foreground layer.
    pub foreground: Vec<FurnAnchor>,
    /// A `page(fill: <solid>)` synthesized as one full-page anchored shape.
    pub fill_units: Vec<FurnAnchor>,
    /// Zone signatures for variation classification.
    pub header_sig: String,
    pub footer_sig: String,
    pub background_sig: String,
    pub foreground_sig: String,
    /// `page(fill:)` signature: the resolved solid hex, or a marker for a
    /// declared non-solid fill (remainder).
    pub fill_sig: String,
    /// The background layer was located in the page frame.
    pub saw_background: bool,
    /// The foreground layer was located in the page frame — `foreground` is
    /// the only marginal without an artifact wrap, so a flat untagged one
    /// with no footer is undetectable and reports a remainder instead.
    pub saw_foreground: bool,
    /// Painted content dropped while measuring.
    pub drops: DropCounts,
}

/// Per-page furniture analysis for the whole document.
pub struct FurnitureMaps {
    pub per_page: Vec<PageFurniture>,
    /// Which furniture slots the source declared — a declared layer that was
    /// never located reports a remainder rather than dropping silently.
    pub declared: DeclaredFurniture,
}

/// Analyze every page's frame into furniture zones. Called only for C2/C3 —
/// C1 keeps its literalize-furniture contract and runs none of this.
#[must_use]
pub fn analyze(
    paged: &PagedDocument,
    margins: &MarginsPt,
    declared: DeclaredFurniture,
) -> FurnitureMaps {
    let per_page = paged
        .pages()
        .iter()
        .map(|page| analyze_page(page, margins, declared))
        .collect();
    FurnitureMaps { per_page, declared }
}

fn analyze_page(page: &Page, margins: &MarginsPt, declared: DeclaredFurniture) -> PageFurniture {
    let frame = &page.frame;
    let page_w = frame.width().to_pt();
    let page_h = frame.height().to_pt();

    let ranges = classify_page(frame, page_w, page_h, margins, declared);
    let mut out = PageFurniture::default();

    for range in &ranges {
        let items = zone_items(frame, range);
        // Header/footer zones only contribute place-bracket units — their
        // loose items stay flat header/footer text on the existing path.
        let loose = matches!(range.zone, Zone::Background | Zone::Foreground);
        let units = measure_zone(&items, loose, &mut out.drops, page_w, page_h);
        match range.zone {
            Zone::Background => {
                out.saw_background = true;
                out.excluded_top.push(range.indices.clone());
                out.background = units;
            }
            Zone::Foreground => {
                out.saw_foreground = true;
                out.excluded_top.push(range.indices.clone());
                out.foreground = units;
            }
            Zone::Header => out.header = units,
            Zone::Footer => out.footer = units,
            Zone::Inner => {}
        }
    }
    out.header_sig = units_sig(&out.header);
    out.footer_sig = units_sig(&out.footer);
    out.background_sig = units_sig(&out.background);
    out.foreground_sig = units_sig(&out.foreground);
    match page_fill(page) {
        PageFill::Solid(hex) => {
            out.fill_units = vec![fill_anchor(&hex, page_w, page_h)];
            out.fill_sig = hex;
        }
        PageFill::NonSolid => {
            out.drops.non_solid_fill += 1;
            out.fill_sig = "non-solid".to_string();
        }
        PageFill::None => {}
    }
    out
}

/// What `page(fill:)` resolves to for anchoring purposes.
enum PageFill {
    None,
    /// A solid colour, as the uppercase hex Word's `srgbClr` takes.
    Solid(String),
    /// Declared but not a solid colour (gradient, tiling, pattern) — a
    /// registered remainder.
    NonSolid,
}

fn page_fill(page: &Page) -> PageFill {
    match page.fill_or_transparent() {
        None => PageFill::None,
        Some(paint @ Paint::Solid(_)) => PageFill::Solid(paint_hex(&paint)),
        Some(_) => PageFill::NonSolid,
    }
}

fn paint_hex(paint: &Paint) -> String {
    let Paint::Solid(color) = paint else {
        return String::new();
    };
    let hex = color.to_hex();
    let digits = hex.as_str().strip_prefix('#').unwrap_or(hex.as_str());
    digits[..6.min(digits.len())].to_uppercase()
}

/// A contiguous run of top-level frame items belonging to one zone.
struct ZoneRange {
    zone: Zone,
    /// Top-level item indices of the whole zone — the `excluded_top` value.
    indices: Range<usize>,
    /// How the zone's items are reached for measurement.
    items: ZoneItems,
}

enum ZoneItems {
    /// Flat items spliced into the page frame (inlined marginal or artifact
    /// tag bracket) — the range equals `indices`.
    Flat,
    /// The marginal frame was pushed as one `FrameItem::Group` — items are
    /// read from `group.frame` at group-offset positions.
    Grouped,
}

/// The items of a zone range as `(absolute position, item)` pairs.
fn zone_items<'a>(frame: &'a Frame, range: &ZoneRange) -> Vec<(Point, &'a FrameItem)> {
    match range.items {
        ZoneItems::Flat => frame
            .items()
            .enumerate()
            .filter(|(i, _)| range.indices.contains(i))
            .map(|(_, &(pos, ref item))| (pos, item))
            .collect(),
        ZoneItems::Grouped => {
            let mut out = Vec::new();
            for (i, &(pos, ref item)) in frame.items().enumerate() {
                if range.indices.contains(&i)
                    && let FrameItem::Group(group) = item
                {
                    out.extend(group.frame.items().map(|&(p, ref item)| (pos + p, item)));
                }
            }
            out
        }
    }
}

/// Classify a page frame's top-level items into finalize's paint layers.
///
/// Two detection mechanisms cover both marginal shapes:
/// - artifact tag brackets (`ArtifactKind::Header/Footer/Background`) —
///   exact for flat marginals and the first tag inside grouped ones;
/// - position+size for `FrameItem::Group` marginals at their finalize push
///   positions, and a non-artifact tag at the page origin for flat
///   foreground (which carries no artifact wrap).
fn classify_page(
    frame: &Frame,
    page_w: f64,
    page_h: f64,
    margins: &MarginsPt,
    declared: DeclaredFurniture,
) -> Vec<ZoneRange> {
    let items: Vec<(Point, &FrameItem)> =
        frame.items().map(|&(pos, ref item)| (pos, item)).collect();
    let n = items.len();
    let mut ranges: Vec<ZoneRange> = Vec::new();
    let mut i = 0;
    let mut saw_inner = false;
    // The inner frame was pushed as a group (its size exceeded the inline
    // limit) — after it, only footer/foreground can follow, so a remaining
    // non-footer item starts the untagged foreground.
    let mut inner_group_seen = false;
    let mut fg_start: Option<usize> = None;
    while i < n {
        let (pos, item) = items[i];
        match item {
            FrameItem::Tag(Tag::Start(elem, _)) => {
                if let Some(zone) = artifact_kind(elem).and_then(zone_for_artifact) {
                    let end = find_bracket_end(&items, i, elem.location()).unwrap_or(i);
                    ranges.push(ZoneRange {
                        zone,
                        indices: i..(end + 1).min(n),
                        items: ZoneItems::Flat,
                    });
                    if zone == Zone::Footer && declared.foreground && end + 1 < n {
                        // Foreground is pushed right after the footer.
                        fg_start = Some(end + 1);
                        break;
                    }
                    i = end + 1;
                    continue;
                }
                if declared.foreground
                    && fg_start.is_none()
                    && (inner_group_seen
                        || (at_origin(pos)
                            && (saw_inner || bracket_has_paint(&items, i, elem.location()))))
                {
                    // A non-artifact tag after the grouped inner frame can only
                    // be foreground furniture. So can a tag at the page origin
                    // once flat inner content began — and on an empty-inner
                    // page, an origin tag whose bracket carries painted items
                    // (leading counter tags bracket nothing).
                    fg_start = Some(i);
                    break;
                }
            }
            FrameItem::Group(group) => {
                match classify_group(pos, group, page_w, page_h, margins, saw_inner) {
                    Zone::Inner => {
                        saw_inner = true;
                        inner_group_seen = true;
                    }
                    zone => {
                        ranges.push(ZoneRange {
                            zone,
                            indices: i..i + 1,
                            items: ZoneItems::Grouped,
                        });
                        if zone == Zone::Footer && declared.foreground && i + 1 < n {
                            fg_start = Some(i + 1);
                            break;
                        }
                    }
                }
            }
            _ => {
                if declared.foreground && inner_group_seen && fg_start.is_none() {
                    // A top-level item after the grouped inner frame that is
                    // not a footer artifact/group is untagged foreground.
                    fg_start = Some(i);
                    break;
                }
                saw_inner = true;
            }
        }
        i += 1;
    }
    if let Some(start) = fg_start.filter(|&s| s < n) {
        ranges.push(ZoneRange {
            zone: Zone::Foreground,
            indices: start..n,
            items: ZoneItems::Flat,
        });
    }
    ranges
}

/// The artifact kind of a `Tag::Start` element, when it is an `ArtifactElem`.
/// `kind` is a settable field — artifacts created via `.artifact(kind)`
/// always carry it set.
fn artifact_kind(elem: &typst_library::foundations::Content) -> Option<ArtifactKind> {
    elem.to_packed::<ArtifactElem>()
        .and_then(|a| *a.kind.as_option())
}

fn zone_for_artifact(kind: ArtifactKind) -> Option<Zone> {
    match kind {
        ArtifactKind::Header => Some(Zone::Header),
        ArtifactKind::Footer => Some(Zone::Footer),
        ArtifactKind::Background => Some(Zone::Background),
        _ => None,
    }
}

/// Position within tolerance of the page origin (covers the bleed origin
/// `(-bleed.left, -bleed.top)` too — bleed is a few pt at most).
fn at_origin(pos: Point) -> bool {
    pos.x.to_pt().abs() <= MARGINAL_TOLERANCE_PT && pos.y.to_pt().abs() <= MARGINAL_TOLERANCE_PT
}

/// Find the `Tag::End` matching the `Tag::Start` at `start`, tracking nesting
/// depth across all tag kinds.
fn find_bracket_end(
    items: &[(Point, &FrameItem)],
    start: usize,
    location: Option<Location>,
) -> Option<usize> {
    let mut depth = 0usize;
    for (j, (_, item)) in items.iter().enumerate().skip(start + 1) {
        match item {
            FrameItem::Tag(Tag::Start(..)) => depth += 1,
            FrameItem::Tag(Tag::End(loc, ..)) => {
                if depth == 0 && Some(*loc) == location {
                    return Some(j);
                }
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
    }
    None
}

/// Whether the tag bracket starting at `start` contains at least one painted
/// (non-tag) item before its matching `Tag::End`.
fn bracket_has_paint(
    items: &[(Point, &FrameItem)],
    start: usize,
    location: Option<Location>,
) -> bool {
    let mut depth = 0usize;
    for (_, item) in items.iter().skip(start + 1) {
        match item {
            FrameItem::Tag(Tag::Start(..)) => depth += 1,
            FrameItem::Tag(Tag::End(loc, ..)) => {
                if depth == 0 && Some(*loc) == location {
                    return false;
                }
                depth = depth.saturating_sub(1);
            }
            _ => return true,
        }
    }
    false
}

/// Classify a top-level `Group`: an artifact tag inside wins (the marginal
/// is `content.artifact(kind)`), else the finalize push position and size.
fn classify_group(
    pos: Point,
    group: &GroupItem,
    page_w: f64,
    page_h: f64,
    margins: &MarginsPt,
    saw_inner: bool,
) -> Zone {
    for (_, item) in group.frame.items() {
        if let FrameItem::Tag(Tag::Start(elem, _)) = item
            && let Some(zone) = artifact_kind(elem).and_then(zone_for_artifact)
        {
            return zone;
        }
    }
    let x = pos.x.to_pt();
    let y = pos.y.to_pt();
    let w = group.frame.size().x.to_pt();
    let h = group.frame.size().y.to_pt();
    let near = |a: f64, b: f64| (a - b).abs() <= MARGINAL_TOLERANCE_PT;

    if near(x, margins.left) && near(y, margins.top) {
        return Zone::Inner;
    }
    if near(x, margins.left) && y.abs() <= MARGINAL_TOLERANCE_PT && h <= margins.top + 1.0 {
        return Zone::Header;
    }
    if near(x, margins.left) && near(y + h, page_h) {
        return Zone::Footer;
    }
    if at_origin(pos) && w >= page_w - 4.0 && h >= page_h - 4.0 {
        return if saw_inner {
            Zone::Foreground
        } else {
            Zone::Background
        };
    }
    Zone::Inner
}

/// Split a zone's items into measured units: each `place` tag bracket is one
/// unit; when `include_loose`, the loose (non-bracket) items form one more.
fn measure_zone(
    items: &[(Point, &FrameItem)],
    include_loose: bool,
    drops: &mut DropCounts,
    page_w: f64,
    page_h: f64,
) -> Vec<FurnAnchor> {
    let mut units = Vec::new();
    let mut loose: Vec<(Point, &FrameItem)> = Vec::new();
    let mut i = 0;
    while i < items.len() {
        let (_, item) = items[i];
        if let FrameItem::Tag(Tag::Start(elem, _)) = item
            && elem.elem().name() == "place"
        {
            let end = find_bracket_end(items, i, elem.location()).unwrap_or(i);
            if let Some(unit) = measure_unit(&items[i + 1..end], Some(elem), drops, page_w, page_h)
            {
                units.push(unit);
            }
            i = end + 1;
            continue;
        }
        loose.push(items[i]);
        i += 1;
    }
    if include_loose
        && loose
            .iter()
            .any(|(_, item)| !matches!(item, FrameItem::Tag(_)))
        && let Some(unit) = measure_unit(&loose, None, drops, page_w, page_h)
    {
        units.push(unit);
    }
    units
}

/// Measure one furniture unit and extract its content.
///
/// Text becomes `txbxContent` paragraphs (one per rendered baseline); a lone
/// raster image becomes `pic:pic`; a lone filled rect becomes a `a:solidFill`
/// shape. Anything else is counted in `drops` for the remainder report.
fn measure_unit(
    items: &[(Point, &FrameItem)],
    place_elem: Option<&typst_library::foundations::Content>,
    drops: &mut DropCounts,
    page_w: f64,
    page_h: f64,
) -> Option<FurnAnchor> {
    let mut bounds = ItemBounds::default();
    for (pos, item) in items {
        bounds.add(*pos, item);
    }
    let (x, y, w, h) = bounds.rect()?;

    if let Some(elem) = place_elem
        && let Some(place) = elem.to_packed::<typst_library::layout::PlaceElem>()
        && body_is_lone_table(&place.body)
    {
        drops.lone_tables += 1;
    }

    let mut texts: Vec<(Point, &TextItem)> = Vec::new();
    let mut images: Vec<(Point, &Image, Size)> = Vec::new();
    let mut fill_hex: Option<String> = None;
    collect_unit_content(items, &mut texts, &mut images, &mut fill_hex, drops);

    let content = if images.len() == 1 && texts.is_empty() && fill_hex.is_none() {
        let (_, image, size) = images[0];
        if let Some(data) = image_data(image, size) {
            AnchoredContent::Image(data)
        } else {
            drops.images += 1;
            return None;
        }
    } else {
        drops.images += u32::try_from(images.len()).unwrap_or(u32::MAX);
        let blocks = text_blocks(&texts);
        if blocks.is_empty() && fill_hex.is_none() {
            return None;
        }
        AnchoredContent::Text { blocks, fill_hex }
    };

    // Text boxes are expanded to a page span keeping the left/top edge —
    // measured ink bounds are tighter than Word's line boxes, and with
    // `wrap="none"` + overflow the text still renders from the kept edge.
    let is_text = matches!(content, AnchoredContent::Text { .. });
    let (x_emu, width_emu) = if is_text {
        (emu(x), pt_to_emu((page_w - x).max(w)))
    } else {
        (emu(x), pt_to_emu(w))
    };
    let (y_emu, height_emu) = if is_text {
        (emu(y), pt_to_emu((page_h - y).max(h)))
    } else {
        (emu(y), pt_to_emu(h))
    };

    let sig = unit_sig(&content, x, y, w, h);
    Some(FurnAnchor {
        float: AnchoredFloat {
            x_emu,
            y_emu,
            horz_rel: RelFrom::Page,
            width_emu,
            height_emu,
            clearance_emu: 0,
            wrap: AnchorWrap::None,
            behind_doc: false,
            z_order: 0,
            content,
        },
        sig,
    })
}

/// Recursively collect the painted content of one unit.
fn collect_unit_content<'a>(
    items: &[(Point, &'a FrameItem)],
    texts: &mut Vec<(Point, &'a TextItem)>,
    images: &mut Vec<(Point, &'a Image, Size)>,
    fill_hex: &mut Option<String>,
    drops: &mut DropCounts,
) {
    for (pos, item) in items {
        match item {
            FrameItem::Group(group) => {
                if !group.transform.is_identity() || group.clip.is_some() {
                    drops.transforms += 1;
                }
                let sub: Vec<(Point, &FrameItem)> = group
                    .frame
                    .items()
                    .map(|&(p, ref item)| (*pos + p, item))
                    .collect();
                collect_unit_content(&sub, texts, images, fill_hex, drops);
            }
            FrameItem::Text(text) => texts.push((*pos, text)),
            FrameItem::Image(image, size, _) => images.push((*pos, image, *size)),
            FrameItem::Shape(shape, _) => {
                if let (Geometry::Rect(_), Some(paint @ Paint::Solid(_))) =
                    (&shape.geometry, &shape.fill)
                {
                    *fill_hex = Some(paint_hex(paint));
                } else {
                    drops.shapes += 1;
                }
            }
            FrameItem::Link(..) => drops.links += 1,
            FrameItem::Tag(_) => {}
        }
    }
}

/// One txbxContent paragraph per rendered baseline: consecutive text items
/// within ~1pt of the same baseline y join as runs of one paragraph.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "font sizes are small non-negative values"
)]
fn text_blocks(texts: &[(Point, &TextItem)]) -> Vec<BlockElement> {
    let mut blocks: Vec<BlockElement> = Vec::new();
    let mut last_y = f64::NAN;
    for (pos, text) in texts {
        if text.text.is_empty() {
            continue;
        }
        let mut run = Run::new(text.text.to_string());
        run.size_half_pt = Some((text.size.to_pt() * 2.0).round().max(1.0) as u32);
        run.bold = effective_weight(&text.font) >= 600;
        run.color = super::run_style::extract_non_black_color(&text.fill);
        let same_line = (pos.y.to_pt() - last_y).abs() <= 1.0;
        if same_line && let Some(BlockElement::Paragraph(para)) = blocks.last_mut() {
            para.inlines.push(InlineElement::Text(run));
            continue;
        }
        last_y = pos.y.to_pt();
        let mut para = Paragraph::new();
        para.spacing_before = Some(0);
        para.spacing_after = Some(0);
        para.inlines.push(InlineElement::Text(run));
        blocks.push(BlockElement::Paragraph(para));
    }
    blocks
}

/// A `page(fill: <solid>)` as one full-page anchored filled shape.
fn fill_anchor(hex: &str, page_w: f64, page_h: f64) -> FurnAnchor {
    let mut para = Paragraph::new();
    para.mark_size_half_pt = Some(2);
    para.spacing_before = Some(0);
    para.spacing_after = Some(0);
    FurnAnchor {
        float: AnchoredFloat {
            x_emu: 0,
            y_emu: 0,
            horz_rel: RelFrom::Page,
            width_emu: pt_to_emu(page_w),
            height_emu: pt_to_emu(page_h),
            clearance_emu: 0,
            wrap: AnchorWrap::None,
            behind_doc: true,
            z_order: 0,
            content: AnchoredContent::Text {
                blocks: vec![BlockElement::Paragraph(para)],
                fill_hex: Some(hex.to_string()),
            },
        },
        sig: format!("fill:{hex}"),
    }
}

/// The comparison signature of one unit: content kind + rounded geometry.
fn unit_sig(content: &AnchoredContent, x: f64, y: f64, w: f64, h: f64) -> String {
    let payload = match content {
        AnchoredContent::Image(img) => format!("img:{:x}", hash_bytes(&img.bytes)),
        AnchoredContent::Text { blocks, fill_hex } => {
            let mut text = String::new();
            for block in blocks {
                if let BlockElement::Paragraph(p) = block {
                    for inline in &p.inlines {
                        if let InlineElement::Text(run) = inline {
                            text.push_str(&run.text);
                        }
                    }
                }
            }
            format!("txt:{text}|fill:{}", fill_hex.as_deref().unwrap_or(""))
        }
    };
    format!(
        "{payload}@{:.0},{:.0},{:.0}x{:.0}",
        x.round(),
        y.round(),
        w.round(),
        h.round()
    )
}

/// The joined signature of a zone's units, or empty when the zone is empty.
fn units_sig(units: &[FurnAnchor]) -> String {
    units.iter().map(|u| u.sig.as_str()).collect()
}

/// A raster (or rasterizable) `typst Image` as `ImageData`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "SVG canvas dimensions are small positive values"
)]
fn image_data(image: &Image, size: Size) -> Option<ImageData> {
    let w_emu = pt_to_emu(size.x.to_pt());
    let h_emu = pt_to_emu(size.y.to_pt());
    let image_data = ImageData {
        bytes: Vec::new(),
        format: ImageFormat::Png,
        svg: None,
        width_emu: w_emu,
        height_emu: h_emu,
        src_rect: None,
    };
    match image.kind() {
        ImageKind::Raster(raster) => {
            let bytes = raster.data().as_slice().to_vec();
            if is_png(&bytes) {
                Some(ImageData {
                    bytes,
                    ..image_data
                })
            } else if is_jpeg(&bytes) {
                Some(ImageData {
                    bytes,
                    format: ImageFormat::Jpeg,
                    ..image_data
                })
            } else {
                let png = reencode_png(&bytes)?;
                Some(ImageData {
                    bytes: png,
                    ..image_data
                })
            }
        }
        ImageKind::Svg(svg_image) => {
            let svg_bytes = svg_image.data().as_slice().to_vec();
            let mut options = resvg::usvg::Options::default();
            options.fontdb_mut().load_system_fonts();
            let tree = resvg::usvg::Tree::from_data(&svg_bytes, &options).ok()?;
            let pixel_w = tree.size().width().ceil() as u32;
            let pixel_h = tree.size().height().ceil() as u32;
            if pixel_w == 0 || pixel_h == 0 {
                return None;
            }
            let mut pixmap = tiny_skia::Pixmap::new(pixel_w, pixel_h)?;
            resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());
            let png = pixmap.encode_png().ok()?;
            Some(ImageData {
                bytes: png,
                svg: Some(svg_bytes),
                ..image_data
            })
        }
        ImageKind::Pdf(_) => None,
    }
}

fn is_png(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes[..4] == [0x89, 0x50, 0x4E, 0x47]
}

fn is_jpeg(bytes: &[u8]) -> bool {
    bytes.len() >= 3 && bytes[..3] == [0xFF, 0xD8, 0xFF]
}

fn reencode_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let decoded = ::image::load_from_memory(bytes).ok()?;
    let mut png = Vec::new();
    decoded
        .write_to(
            &mut std::io::Cursor::new(&mut png),
            ::image::ImageFormat::Png,
        )
        .ok()?;
    Some(png)
}

/// Whether a place body is a lone table (sequences/styled wrappers
/// unwrapped). A lone-table `#place` keeps `w:tblpPr` on the body path; in
/// furniture it can only keep its text — the grid loss is reported.
fn body_is_lone_table(content: &typst_library::foundations::Content) -> bool {
    if content
        .to_packed::<typst_library::model::TableElem>()
        .is_some()
    {
        return true;
    }
    if let Some(seq) = content.to_packed::<typst_library::foundations::SequenceElem>() {
        let mut iter = seq.children.iter();
        return match (iter.next(), iter.next()) {
            (Some(first), None) => body_is_lone_table(first),
            _ => false,
        };
    }
    if let Some(styled) = content.to_packed::<typst_library::foundations::StyledElem>() {
        return body_is_lone_table(&styled.child);
    }
    false
}

/// The header/footer zone's measured anchors (`#place` units).
#[must_use]
pub fn zone_anchors(furn: &PageFurniture, zone: MarginZone) -> &[FurnAnchor] {
    match zone {
        MarginZone::Top => &furn.header,
        MarginZone::Bottom => &furn.footer,
    }
}

/// The header/footer zone's furniture signature — folded into the zone's
/// text comparison so a varying `#place` registers as a variant instead of
/// silently keeping the first page's.
#[must_use]
pub fn zone_sig(furn: &PageFurniture, zone: MarginZone) -> &str {
    match zone {
        MarginZone::Top => &furn.header_sig,
        MarginZone::Bottom => &furn.footer_sig,
    }
}

/// Whether the zone's anchors paint behind body text: the header layer is
/// pushed before the inner frame (behind), the footer after (in front) —
/// matching Typst's paint order.
#[must_use]
pub fn zone_behind_doc(zone: MarginZone) -> bool {
    match zone {
        MarginZone::Top => true,
        MarginZone::Bottom => false,
    }
}

/// Which header part slot a furniture layer's anchors land in.
enum HeaderSlot {
    Default,
    Even,
    First,
}

/// The measured units of one furniture layer on one page.
fn layer_units<'a>(name: &str, page: &'a PageFurniture) -> &'a [FurnAnchor] {
    match name {
        "background" => &page.background,
        "foreground" => &page.foreground,
        _ => &page.fill_units,
    }
}

/// Slot classification for one furniture layer across all pages — the same
/// three representable shapes as `ZoneParts`, then a named remainder.
enum SlotLayout {
    /// Same signature on every page → `default` part only.
    Uniform { rep: usize },
    /// Page 1 differs, pages 2.. uniform → `first` + `default` + `w:titlePg`.
    FirstVsRest { rest: usize },
    /// Odd vs even signatures → `default` + `even` + `w:evenAndOddHeaders`.
    OddEven { odd: usize, even: usize },
    /// Anything else — kept as one representative page in the `first` part
    /// (page-1-only, least-wrong placement) plus a `page-foreground` report.
    Unrepresentable { rep: usize, first_diff: usize },
}

fn classify_sigs(sigs: &[String]) -> Option<SlotLayout> {
    let n = sigs.len();
    if n == 0 || sigs.iter().all(String::is_empty) {
        return None;
    }
    let first = &sigs[0];
    if sigs.iter().all(|s| s == first) {
        return Some(SlotLayout::Uniform { rep: 0 });
    }
    if sigs[1..].iter().all(|s| s == &sigs[1]) {
        return Some(SlotLayout::FirstVsRest { rest: 1 });
    }
    let odd: Vec<usize> = (0..n).step_by(2).collect();
    let even: Vec<usize> = (1..n).step_by(2).collect();
    if odd.iter().all(|&i| sigs[i] == *first)
        && !even.is_empty()
        && even.iter().all(|&i| sigs[i] == sigs[even[0]])
    {
        return Some(SlotLayout::OddEven {
            odd: 0,
            even: even[0],
        });
    }
    let first_diff = sigs.iter().position(|s| s != first).unwrap_or(1) + 1;
    let rep = sigs.iter().position(|s| !s.is_empty()).unwrap_or(0);
    Some(SlotLayout::Unrepresentable { rep, first_diff })
}

/// Result of classifying all furniture layers; consumed by
/// `apply_headers_and_footers`, which sets `w:titlePg`/`w:evenAndOddHeaders`
/// before part propagation and attaches the anchors after it.
pub struct FurniturePlan {
    header_default: Vec<(FurnAnchor, bool)>,
    header_even: Vec<(FurnAnchor, bool)>,
    header_first: Vec<(FurnAnchor, bool)>,
    /// `w:titlePg` needed for a furniture `first` slot.
    pub needs_title_page: bool,
    /// `w:evenAndOddHeaders` needed for a furniture `even` slot.
    pub needs_even_odd: bool,
}

/// Classify the background/foreground/fill layers of every page into a plan.
/// Header/footer-zone `#place` anchors are attached by the margin-zone
/// extraction itself and are not part of the plan.
#[must_use]
pub fn plan(maps: &FurnitureMaps, degradations: &mut Vec<Degradation>) -> FurniturePlan {
    report_remainders(maps, degradations);
    let mut plan = FurniturePlan {
        header_default: Vec::new(),
        header_even: Vec::new(),
        header_first: Vec::new(),
        needs_title_page: false,
        needs_even_odd: false,
    };
    // Each furniture layer classifies independently into header slots.
    for (name, behind) in [
        ("background", true),
        ("foreground", false),
        ("page fill", true),
    ] {
        plan_layer(maps, name, behind, &mut plan, degradations);
    }
    plan
}

/// Every painted remainder — unrepresentable item kinds and declared layers
/// that produced no locatable zone — rolled into one `page-foreground` report.
fn report_remainders(maps: &FurnitureMaps, degradations: &mut Vec<Degradation>) {
    let mut drops = DropCounts::default();
    for page in &maps.per_page {
        drops.shapes += page.drops.shapes;
        drops.links += page.drops.links;
        drops.images += page.drops.images;
        drops.lone_tables += page.drops.lone_tables;
        drops.non_solid_fill += page.drops.non_solid_fill;
        drops.transforms += page.drops.transforms;
    }
    let mut remainder = Vec::new();
    if drops.shapes > 0 {
        remainder.push(format!("{} non-rect shape(s)", drops.shapes));
    }
    if drops.links > 0 {
        remainder.push(format!("{} link(s)", drops.links));
    }
    if drops.images > 0 {
        remainder.push(format!("{} non-raster image(s)", drops.images));
    }
    if drops.lone_tables > 0 {
        remainder.push(format!(
            "{} lone-table place(s) kept as text without the grid",
            drops.lone_tables
        ));
    }
    if drops.non_solid_fill > 0 {
        remainder.push("a non-solid page(fill:)".to_string());
    }
    if drops.transforms > 0 {
        remainder.push(format!(
            "{} transformed or clipped group(s) kept without the transform",
            drops.transforms
        ));
    }
    for (declared, saw, name) in [
        (
            maps.declared.background,
            maps.per_page.iter().any(|p| p.saw_background),
            "page(background:)",
        ),
        (
            maps.declared.foreground,
            maps.per_page.iter().any(|p| p.saw_foreground),
            "page(foreground:)",
        ),
    ] {
        if declared && !saw {
            remainder.push(format!(
                "declared {name} that produced no locatable furniture layer"
            ));
        }
    }
    if !remainder.is_empty() {
        degradations.push(Degradation::new(
            None,
            "page-foreground",
            DegradationAction::Skipped,
            format!(
                "page furniture contains content without an anchored representation: {}",
                remainder.join(", ")
            ),
        ));
    }
}

/// Classify one furniture layer's per-page signatures and queue the
/// representative pages' units into the matching header slots.
fn plan_layer(
    maps: &FurnitureMaps,
    name: &str,
    behind: bool,
    plan: &mut FurniturePlan,
    degradations: &mut Vec<Degradation>,
) {
    let sigs: Vec<String> = maps
        .per_page
        .iter()
        .map(|p| match name {
            "background" => p.background_sig.clone(),
            "foreground" => p.foreground_sig.clone(),
            _ => p.fill_sig.clone(),
        })
        .collect();
    let Some(layout) = classify_sigs(&sigs) else {
        return;
    };
    let mut attach = |slot: HeaderSlot, rep: usize| {
        let anchors = layer_units(name, &maps.per_page[rep]);
        let target = match slot {
            HeaderSlot::Default => &mut plan.header_default,
            HeaderSlot::Even => &mut plan.header_even,
            HeaderSlot::First => &mut plan.header_first,
        };
        for anchor in anchors {
            let mut float = anchor.float.clone();
            float.behind_doc = behind;
            target.push((
                FurnAnchor {
                    float,
                    sig: anchor.sig.clone(),
                },
                behind,
            ));
        }
    };
    match layout {
        SlotLayout::Uniform { rep } => attach(HeaderSlot::Default, rep),
        SlotLayout::FirstVsRest { rest } => {
            plan.needs_title_page = true;
            attach(HeaderSlot::First, 0);
            attach(HeaderSlot::Default, rest);
        }
        SlotLayout::OddEven { odd, even } => {
            plan.needs_even_odd = true;
            attach(HeaderSlot::Default, odd);
            attach(HeaderSlot::Even, even);
        }
        SlotLayout::Unrepresentable { rep, first_diff } => {
            degradations.push(Degradation::new(
                None,
                "page-foreground",
                DegradationAction::Approximated,
                format!(
                    "{name} varies per page in a way Word's default/even/first header parts \
                     cannot express; the {name} of page {} is kept on page 1 only and the \
                     variant at page {first_diff} is not",
                    rep + 1,
                ),
            ));
            plan.needs_title_page = true;
            attach(HeaderSlot::First, rep);
        }
    }
}

/// Attach the planned anchors to the document's header parts, creating parts
/// that do not exist yet. Runs after part propagation so real header/footer
/// content is already in every slot.
pub fn attach(plan: &FurniturePlan, doc: &mut Document) {
    for (slot, anchors) in [
        (&mut doc.header, &plan.header_default),
        (&mut doc.header_even, &plan.header_even),
        (&mut doc.header_first, &plan.header_first),
    ] {
        if anchors.is_empty() {
            continue;
        }
        let part = slot.get_or_insert_with(|| HeaderFooter {
            paragraphs: Vec::new(),
        });
        for (anchor, behind) in anchors {
            let mut float = anchor.float.clone();
            float.behind_doc = *behind;
            float.z_order = u32::try_from(
                part.paragraphs
                    .iter()
                    .filter(|p| {
                        p.inlines
                            .iter()
                            .any(|i| matches!(i, InlineElement::AnchoredFloat(_)))
                    })
                    .count(),
            )
            .unwrap_or(u32::MAX)
            .saturating_add(1);
            let mut anchor_para = Paragraph::new();
            anchor_para.mark_size_half_pt = Some(2);
            anchor_para
                .inlines
                .push(InlineElement::AnchoredFloat(float));
            part.paragraphs.push(anchor_para);
        }
    }
}

/// Build the `AnchoredFloat` paragraph one measured `#place` unit becomes
/// inside a margin zone's header/footer part — used by `margin.rs` zone
/// extraction, which owns that part's classification.
#[must_use]
pub fn anchor_paragraph(anchor: &FurnAnchor, zone: MarginZone, z_order: u32) -> Paragraph {
    let mut float = anchor.float.clone();
    float.behind_doc = zone_behind_doc(zone);
    float.z_order = z_order;
    let mut para = Paragraph::new();
    para.mark_size_half_pt = Some(2);
    para.inlines.push(InlineElement::AnchoredFloat(float));
    para
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "page coordinates are small pt values"
)]
fn emu(pt: f64) -> i64 {
    (pt * 12_700.0).round() as i64
}
