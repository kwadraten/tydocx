//! Rendered-line extraction from paged frames.

use std::collections::BTreeMap;

use typort_ooxml::document::{BlockElement, Document, ParagraphStyle, Run};
use typst::foundations::{Content, StyleChain};
use typst::introspection::Tag;
use typst::layout::{Frame, FrameItem};
use typst_layout::PagedDocument;
use typst_library::layout::{
    BlockElem, BoxElem, GridElem, MoveElem, PlaceElem, RotateElem, ScaleElem,
};
use typst_library::visualize::{CircleElem, CurveElem, LineElem, PolygonElem, RectElem};
use typst_syntax::Span;

use crate::compat::DegradationAction;

/// Minimum substantive emitted-heading run length used for title detection.
const MIN_HEADING_RUN_CHARS: usize = 2;
/// Default body size in points when the recovery detector has no sample.
const DEFAULT_RECOVERY_BODY_SIZE_PT: f64 = 10.5;
/// Vertical bucket height in points used to assemble rendered lines.
const LINE_Y_BUCKET_PT: f64 = 8.0;
/// Default page width in points when paged geometry has no first page.
const DEFAULT_PAGE_WIDTH_PT: f64 = 595.0;
/// Page-width fraction contributing to the x-cluster gap threshold.
const CLUSTER_PAGE_WIDTH_RATIO: f64 = 0.06;
/// Absolute minimum x-cluster gap in points.
const MIN_CLUSTER_GAP_PT: f64 = 20.0;
/// Font-size multiple contributing to the x-cluster gap threshold.
const CLUSTER_FONT_SIZE_MULTIPLE: f64 = 5.0;
/// Minimum x-cluster count needed to recognize multi-column content.
pub(super) const MIN_COLUMN_CLUSTERS: usize = 2;
/// Size multiple above which a multi-cluster line is treated as one title line.
const LARGE_TEXT_BODY_SIZE_MULTIPLE: f64 = 1.3;
/// Body-size fraction below which a recovered run is superscript.
const SUPERSCRIPT_BODY_SIZE_RATIO: f64 = 0.8;

/// What a recovered line's text was inside of in the Typst layout: the
/// enclosing element whose aspect C1 drops. Recorded per text item while
/// collecting frame items; a line is attributed only when all its attributed
/// items agree (see the aggregation in [`extract_lines_from_all_pages`]).
/// Items with no attribution (ordinary body text) never veto a line's
/// attribution — a y-bucket can hold grid text next to body text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LineAttribution {
    /// Stable construct identifier, as in `Degradation::construct`.
    pub construct: &'static str,
    /// How the aspect was degraded.
    pub action: DegradationAction,
    /// Span of the enclosing element, when it has one.
    pub span: Option<Span>,
    /// Human-readable detail, as in `Degradation::message`.
    pub message: &'static str,
}

/// A text line extracted from a `PagedDocument` frame, preserving run-level info.
#[derive(Debug, Clone)]
pub(super) struct FrameLine {
    pub text: String,
    pub runs: Vec<Run>,
    pub x_clusters: Vec<XCluster>,
    pub page_idx: usize,
    pub y_pt: f64,
    pub all_math_font: bool,
    /// The dropped-aspect attribution shared by the line's attributed items,
    /// or `None` when the line holds no attributed text or its attributed
    /// items disagree.
    pub attribution: Option<LineAttribution>,
}

#[derive(Debug, Clone)]
pub(super) struct XCluster {
    pub x_pt: f64,
    pub runs: Vec<Run>,
}

struct FrameTextItem {
    y: f64,
    x: f64,
    text: String,
    size_pt: f64,
    /// The face Typst shaped this item with carries an OpenType `MATH` table
    /// (`FontFlags::MATH`) — the same signal `run_style` already uses (#26).
    /// A name substring ("Math") is not how a math font is identified; the
    /// table is.
    is_math_font: bool,
    /// The dropped-aspect attribution of the innermost enclosing tracked
    /// element at this item's position, or `None` for ordinary body text.
    attribution: Option<LineAttribution>,
}

fn cluster_by_x<'a>(
    items: &[&'a FrameTextItem],
    gap_threshold: f64,
) -> Vec<Vec<&'a FrameTextItem>> {
    if items.is_empty() {
        return Vec::new();
    }
    let mut sorted: Vec<&FrameTextItem> = items.to_vec();
    sorted.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));

    let mut clusters: Vec<Vec<&FrameTextItem>> = Vec::new();
    let mut current: Vec<&FrameTextItem> = vec![sorted[0]];

    for item in &sorted[1..] {
        let last_x = current.last().map_or(0.0, |i| i.x);
        if item.x - last_x > gap_threshold {
            clusters.push(std::mem::take(&mut current));
        }
        current.push(item);
    }
    if !current.is_empty() {
        clusters.push(current);
    }
    clusters
}

pub(super) fn extract_lines_from_all_pages(
    paged: &PagedDocument,
    margins: super::super::page::MarginsPt,
) -> Vec<FrameLine> {
    let mut all_lines = Vec::new();

    // The body size is the same one the document-style detector finds (#27):
    // the shared glyph-weighted half-point detector over the first pages.
    // The old probe read only the first page, weighted by byte length, and
    // bucketed in tenths of a point, so a title page or CJK body text could
    // move it away from the document body size this classification relies on.
    let body_size = {
        let mut items = Vec::new();
        for page in paged
            .pages()
            .iter()
            .take(super::super::page::BODY_SIZE_SAMPLE_PAGES)
        {
            collect_text_items_with_pos(&page.frame, &mut items);
        }
        super::super::page::dominant_body_size_half_pt(
            items
                .iter()
                .map(|item| (item.size_pt, item.text.chars().count())),
        )
        .map_or(DEFAULT_RECOVERY_BODY_SIZE_PT, |half_pt| {
            f64::from(half_pt) / 2.0
        })
    };

    for (page_idx, page) in paged.pages().iter().enumerate() {
        let mut text_items = Vec::new();
        collect_text_items_with_pos(&page.frame, &mut text_items);

        // Drop header/footer chrome (running heads, page numbers) before it
        // becomes a candidate body line. We use the *same* margin boundary that
        // `detect_page_numbering`/`extract_footer` use to LOCATE the footer —
        // the document's resolved margins — so anything outside the body zone
        // is by definition margin content, and body content inside small
        // author margins (`#set page(margin: 1cm)`) is never thrown away.
        let (body_top, body_bottom) = super::super::page::find_body_zone(
            page.frame.width().to_pt(),
            page.frame.height().to_pt(),
            Some(margins.top),
            Some(margins.bottom),
        );
        text_items.retain(|item| body_top <= item.y && item.y <= body_bottom);

        let mut y_groups: BTreeMap<u64, Vec<&FrameTextItem>> = BTreeMap::new();
        for item in &text_items {
            let y_key = (item.y / LINE_Y_BUCKET_PT).round().to_bits();
            y_groups.entry(y_key).or_default().push(item);
        }

        let page_width_pt = paged
            .pages()
            .first()
            .map_or(DEFAULT_PAGE_WIDTH_PT, |p| p.frame.width().to_pt());
        for items in y_groups.values() {
            if let Some(line) = build_line_from_items(items, page_idx, page_width_pt, body_size) {
                all_lines.push(line);
            }
        }
    }
    all_lines
}

/// Build one `FrameLine` from the text items sharing a y-bucket, or `None`
/// for a whitespace-only group. Clustering, run styling, and attribution
/// agreement live here so `extract_lines_from_all_pages` stays small.
fn build_line_from_items(
    items: &[&FrameTextItem],
    page_idx: usize,
    page_width_pt: f64,
    body_size: f64,
) -> Option<FrameLine> {
    let max_font_size = items.iter().map(|i| i.size_pt).fold(0.0_f64, f64::max);
    let gap_threshold = (page_width_pt * CLUSTER_PAGE_WIDTH_RATIO)
        .max(MIN_CLUSTER_GAP_PT)
        .max(max_font_size * CLUSTER_FONT_SIZE_MULTIPLE);
    let raw_clusters = cluster_by_x(items, gap_threshold);

    let clusters = if raw_clusters.len() >= MIN_COLUMN_CLUSTERS {
        let max_size = raw_clusters
            .iter()
            .flat_map(|c| c.iter().map(|i| i.size_pt))
            .fold(0.0_f64, f64::max);
        if max_size > body_size * LARGE_TEXT_BODY_SIZE_MULTIPLE {
            vec![raw_clusters.into_iter().flatten().collect()]
        } else {
            raw_clusters
        }
    } else {
        raw_clusters
    };

    let mut x_clusters = Vec::new();
    let mut all_runs = Vec::new();
    let mut full_text = String::new();

    for cluster in &clusters {
        let mut cluster_runs = Vec::new();
        let cluster_x: f64 = cluster.first().map_or(0.0, |i| i.x);
        for item in cluster {
            let is_super = item.size_pt < body_size * SUPERSCRIPT_BODY_SIZE_RATIO;
            let mut run = Run::new(&item.text);
            run.superscript = is_super;
            let half_pt = super::super::page::pt_to_half_pt(item.size_pt);
            if half_pt != super::super::page::pt_to_half_pt(body_size) {
                run.size_half_pt = Some(half_pt);
            }
            cluster_runs.push(run.clone());
            all_runs.push(run);
            full_text.push_str(&item.text);
        }
        x_clusters.push(XCluster {
            x_pt: cluster_x,
            runs: cluster_runs,
        });
    }

    let trimmed = full_text.trim().to_string();
    let item_count = u32::try_from(items.len()).map_or(f64::from(u32::MAX), f64::from);
    let avg_y = items.iter().map(|i| i.y).sum::<f64>() / item_count;
    let all_math_font = items.iter().all(|i| i.is_math_font);
    // Attribute the line only when all its attributed items agree;
    // unattributed (body) items never veto, differing attributions do.
    let mut attribution: Option<LineAttribution> = None;
    let mut mixed = false;
    for item in items {
        if let Some(attr) = &item.attribution {
            match &attribution {
                None => attribution = Some(attr.clone()),
                Some(existing) if existing == attr => {}
                Some(_) => {
                    mixed = true;
                    break;
                }
            }
        }
    }
    let attribution = if mixed { None } else { attribution };
    if trimmed.is_empty() {
        return None;
    }
    Some(FrameLine {
        text: trimmed,
        runs: all_runs,
        x_clusters,
        page_idx,
        y_pt: avg_y,
        all_math_font,
        attribution,
    })
}

fn collect_text_items_with_pos(frame: &Frame, items: &mut Vec<FrameTextItem>) {
    // Track the enclosing elements whose aspect C1 drops, by their Tag
    // brackets: a Start pushes, the matching End pops (guarded — an End for
    // an untracked element must not pop a tracked one). The stack top is the
    // innermost enclosing tracked element at the item's position. Elements
    // without a location are not pushed: their End cannot be matched, and a
    // leaked entry would misattribute everything after them.
    let mut stack: Vec<(typst::introspection::Location, LineAttribution)> = Vec::new();
    super::super::frames::visit_frame_items(frame, true, &mut |position, item| match item {
        FrameItem::Tag(Tag::Start(content, _)) => {
            if let Some(attribution) = attribution_for_element(content)
                && let Some(location) = content.location()
            {
                stack.push((location, attribution));
            }
        }
        FrameItem::Tag(Tag::End(location, ..)) => {
            if stack.last().is_some_and(|(top, _)| top == location) {
                stack.pop();
            }
        }
        FrameItem::Text(text_item) => {
            let text = text_item.text.to_string();
            if !text.is_empty() {
                let is_math_font = text_item
                    .font
                    .info()
                    .flags
                    .contains(typst_library::text::FontFlags::MATH);
                items.push(FrameTextItem {
                    y: position.y.to_pt(),
                    x: position.x.to_pt(),
                    text,
                    size_pt: text_item.size.to_pt(),
                    is_math_font,
                    attribution: stack.last().map(|(_, attr)| attr.clone()),
                });
            }
        }
        _ => {}
    });
}

/// The dropped-aspect attribution of one Tag-bracketed enclosing element,
/// or `None` for elements C1 keeps (or cannot see). Element fields are
/// resolved against the default style chain (the precedent in
/// `table_align::apply_cell_fill`): an explicitly set value wins; an unset
/// field reads its default. A `#set` rule in scope that flips a field is
/// not seen here — a false negative, never a false positive.
///
/// Measured coverage (Tag-stream probe, 2026-11-16): only `GridElem` and
/// non-float `PlaceElem` bracket their content in the paged frame, so only
/// those two attributions fire in practice. A float `PlaceElem` leaves an
/// EMPTY bracket (its content moves to a separate untagged group);
/// `BoxElem`/`BlockElem`, `MoveElem`/`RotateElem`/`ScaleElem`, and the shape
/// elements leave NO bracket at all — their text is bare `Text` items, and
/// attributing it needs position-based matching (element body locations via
/// the introspector), which is Phase 2 work. The arms below are kept: they
/// fire if a future Typst version brackets these elements, and they document
/// the intended construct for each. Pure shapes are still reported by the
/// `detect_dropped_shapes` scan, which reads shape spans, not Tags.
fn attribution_for_element(elem: &Content) -> Option<LineAttribution> {
    let span = if elem.span().is_detached() {
        None
    } else {
        Some(elem.span())
    };
    if let Some(place) = elem.to_packed::<PlaceElem>() {
        let float = place.float.get(StyleChain::default());
        return Some(LineAttribution {
            construct: if float {
                "floating-graphic"
            } else {
                "absolute-position"
            },
            action: DegradationAction::Skipped,
            span,
            message: if float {
                "floating content kept as flow text; float position dropped"
            } else {
                "absolute positioning dropped; content kept as flow text"
            },
        });
    }
    if elem.to_packed::<GridElem>().is_some() {
        return Some(LineAttribution {
            construct: "grid-layout",
            action: DegradationAction::Skipped,
            span,
            message: "grid layout dropped; cell text kept as paragraphs",
        });
    }
    if elem.to_packed::<MoveElem>().is_some()
        || elem.to_packed::<RotateElem>().is_some()
        || elem.to_packed::<ScaleElem>().is_some()
    {
        return Some(LineAttribution {
            construct: "transform",
            action: DegradationAction::Skipped,
            span,
            message: "transform (move, rotate, scale) dropped; content kept as flow text",
        });
    }
    if elem.to_packed::<RectElem>().is_some()
        || elem.to_packed::<CircleElem>().is_some()
        || elem.to_packed::<LineElem>().is_some()
        || elem.to_packed::<CurveElem>().is_some()
        || elem.to_packed::<PolygonElem>().is_some()
    {
        return Some(LineAttribution {
            construct: "shape-text",
            action: DegradationAction::Skipped,
            span,
            message: "shape dropped; inner text kept as flow text",
        });
    }
    if let Some(boxed) = elem.to_packed::<BoxElem>() {
        return box_decoration_attribution(
            boxed.fill.get_ref(StyleChain::default()).as_ref(),
            &boxed.stroke.get_cloned(StyleChain::default()),
            &boxed.radius.get_cloned(StyleChain::default()),
            boxed.clip.get(StyleChain::default()),
            span,
        );
    }
    if let Some(block) = elem.to_packed::<BlockElem>() {
        return box_decoration_attribution(
            block.fill.get_ref(StyleChain::default()).as_ref(),
            &block.stroke.get_cloned(StyleChain::default()),
            &block.radius.get_cloned(StyleChain::default()),
            block.clip.get(StyleChain::default()),
            span,
        );
    }
    None
}

/// The box-decoration attribution for a `BoxElem`/`BlockElem` whose fill,
/// stroke, radius, or clip is set; `None` for a plain box (no decoration
/// is lost when the box itself is dropped).
#[allow(
    clippy::option_option,
    reason = "stroke type mirrors Typst's BoxElem/BlockElem field type"
)]
fn box_decoration_attribution(
    fill: Option<&typst_library::visualize::Paint>,
    stroke: &typst_library::layout::Sides<Option<Option<typst_library::visualize::Stroke>>>,
    radius: &typst_library::layout::Corners<
        Option<typst_library::layout::Rel<typst_library::layout::Length>>,
    >,
    clip: bool,
    span: Option<Span>,
) -> Option<LineAttribution> {
    if box_has_decoration(fill, stroke, radius, clip) {
        Some(LineAttribution {
            construct: "box-decoration",
            action: DegradationAction::Skipped,
            span,
            message: "box decoration (fill, stroke, radius, clip) dropped; content kept as flow text",
        })
    } else {
        None
    }
}

/// Whether a box/block carries any decoration that dropping it would lose:
/// a fill, any set stroke side, any set corner radius, or clipping.
/// `Some(None)` (explicitly set to none) counts as unset on both tracks.
#[allow(
    clippy::option_option,
    reason = "stroke type mirrors Typst's BoxElem/BlockElem field type"
)]
fn box_has_decoration(
    fill: Option<&typst_library::visualize::Paint>,
    stroke: &typst_library::layout::Sides<Option<Option<typst_library::visualize::Stroke>>>,
    radius: &typst_library::layout::Corners<
        Option<typst_library::layout::Rel<typst_library::layout::Length>>,
    >,
    clip: bool,
) -> bool {
    if fill.is_some() || clip {
        return true;
    }
    if [&stroke.left, &stroke.top, &stroke.right, &stroke.bottom]
        .iter()
        .any(|side| side.as_ref().is_some_and(Option::is_some))
    {
        return true;
    }
    [
        &radius.top_left,
        &radius.top_right,
        &radius.bottom_right,
        &radius.bottom_left,
    ]
    .iter()
    .any(|corner| corner.is_some())
}

pub(super) fn count_title_lines(paged_lines: &[FrameLine], doc: &Document) -> usize {
    let mut count = 0;
    for line in paged_lines {
        let is_heading = doc.body.elements.iter().any(|e| {
            if let BlockElement::Paragraph(p) = e
                && matches!(p.style, Some(ParagraphStyle::Heading(_)))
            {
                // Match on substantive heading text only. A heading's number prefix
                // ("A ", "1 ") is now a separate one-char run; matching on it would
                // misclassify any title-page line that merely starts with "A"/"1".
                p.text_runs().any(|r| {
                    r.text.trim().chars().count() >= MIN_HEADING_RUN_CHARS
                        && line.text.contains(&r.text)
                })
            } else {
                false
            }
        });
        if is_heading {
            count += 1;
        } else {
            break;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use typst_library::layout::{Abs, Corners, Length, Rel, Sides};
    use typst_library::visualize::{Color, Paint, Stroke};

    fn no_decoration() -> (
        Option<Paint>,
        Sides<Option<Option<Stroke>>>,
        Corners<Option<Rel<Length>>>,
        bool,
    ) {
        (None, Sides::splat(None), Corners::splat(None), false)
    }

    #[test]
    fn box_has_decoration_plain_box() {
        let (fill, stroke, radius, clip) = no_decoration();
        assert!(!box_has_decoration(fill.as_ref(), &stroke, &radius, clip));
    }

    #[test]
    fn box_has_decoration_fill() {
        let (mut fill, stroke, radius, clip) = no_decoration();
        fill = Some(Paint::Solid(Color::BLACK));
        assert!(box_has_decoration(fill.as_ref(), &stroke, &radius, clip));
    }

    #[test]
    fn box_has_decoration_clip() {
        let (fill, stroke, radius, _) = no_decoration();
        assert!(box_has_decoration(fill.as_ref(), &stroke, &radius, true));
    }

    #[test]
    fn box_has_decoration_radius() {
        let (fill, stroke, _, clip) = no_decoration();
        let radius = Corners::splat(Some(Rel::from(Abs::pt(2.0))));
        assert!(box_has_decoration(fill.as_ref(), &stroke, &radius, clip));
    }

    #[test]
    fn box_has_decoration_stroke() {
        let (fill, _, radius, clip) = no_decoration();
        let stroke = Sides::splat(Some(Some(Stroke::from_pair(
            Color::BLACK,
            Length::from(Abs::pt(1.0)),
        ))));
        assert!(box_has_decoration(fill.as_ref(), &stroke, &radius, clip));
    }

    #[test]
    fn box_has_decoration_explicit_none_stroke_is_unset() {
        let (fill, _, radius, clip) = no_decoration();
        let stroke = Sides::splat(Some(None));
        assert!(!box_has_decoration(fill.as_ref(), &stroke, &radius, clip));
    }
}
