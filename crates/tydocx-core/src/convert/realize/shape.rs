//! Resolved `DrawingML` geometry for bare shapes: `rect`, `square`,
//! `ellipse`, `circle`, `polygon`, `curve`, and geometric `line`.
//!
//! The resolvers mirror `typst-layout`'s `layout_shape`, `layout_line`,
//! `layout_polygon`, and `layout_curve` step for step, on the page-level
//! region (`page_content_*_pt`) — the same basis the grid-row measurement
//! uses — so a shape lands in the `.docx` at the size Typst would draw it.
//! `resolve_shape` returns the [`IrShapeGeometry`] the lowerer pairs with
//! `fill_hex` and the lowered body to build an `InlineShape`, plus a `loss`
//! note for the paint or geometry detail OOXML cannot carry (per-side
//! strokes, non-solid paint, even-odd fill, dash phase, outset) — the
//! lowerer reports it as a `shape` degradation instead of silently
//! dropping it. A `None` geometry means the size or path could not be
//! resolved and the lowerer keeps the shaded-text fallback.

use std::f64::consts::SQRT_2;

use tydocx_ir::{
    IrShapeGeometry, ShapeCap, ShapeInsets, ShapeJoin, ShapeOutline, ShapePathSeg, ShapeStroke,
    pt_to_emu,
};
use typst_library::engine::Engine;
use typst_library::foundations::{Content, Packed, Resolve, Smart, StyleChain};
use typst_library::introspection::SplitLocator;
use typst_library::layout::{Abs, Axes, Corners, Point, Ratio, Region, Rel, Sides, Size, Sizing};
use typst_library::visualize::{
    CircleElem, CloseMode, CurveComponent, CurveElem, EllipseElem, FillRule, FixedStroke, LineCap,
    LineElem, LineJoin, Paint, PolygonElem, RectElem, SquareElem, Stroke,
};
use typst_utils::Numeric;

use super::collect::{page_content_height_pt, page_content_width_pt};
use crate::convert::table_align::paint_to_shd_hex;

/// The result of resolving one bare shape element against the page region.
pub(super) struct ResolvedShape {
    /// The `DrawingML` payload, or `None` when the size or path could not be
    /// resolved — the lowerer keeps the C2 shaded-text fallback.
    pub geometry: Option<IrShapeGeometry>,
    /// Paint or geometry detail the mapping dropped (a `shape` Approximated
    /// degradation at C2/C3), or the reason `geometry` is `None`.
    pub loss: Option<String>,
}

#[derive(Copy, Clone)]
enum BoxKind {
    Rect,
    Square,
    Ellipse,
    Circle,
}

impl BoxKind {
    fn is_quadratic(self) -> bool {
        matches!(self, Self::Square | Self::Circle)
    }

    fn is_round(self) -> bool {
        matches!(self, Self::Ellipse | Self::Circle)
    }
}

/// Resolve one bare shape element (`child` matched by `shape_parts`) into
/// its `DrawingML` geometry. The region base is the page's text area: Typst's
/// `unbreakable_pod` resolves `width:`/`height:` and the freeform
/// coordinates against the enclosing region, which at page level is the
/// text column.
pub(super) fn resolve_shape(
    content: &Content,
    styles: StyleChain<'_>,
    engine: &mut Engine,
    locator: &mut SplitLocator,
) -> ResolvedShape {
    let base = Size::new(
        Abs::pt(page_content_width_pt(styles)),
        Abs::pt(page_content_height_pt(styles)),
    );
    if let Some(elem) = content.to_packed::<RectElem>() {
        return box_shape(
            BoxKind::Rect,
            elem.width.get(styles).into(),
            elem.height.get(styles),
            elem.body.get_ref(styles).as_ref(),
            elem.fill.get_ref(styles).as_ref(),
            elem.stroke.resolve(styles),
            elem.inset.resolve(styles),
            elem.outset.resolve(styles),
            elem.radius.resolve(styles),
            base,
            content,
            styles,
            engine,
            locator,
        );
    }
    if let Some(elem) = content.to_packed::<SquareElem>() {
        return box_shape(
            BoxKind::Square,
            elem.width.get(styles).into(),
            elem.height.get(styles),
            elem.body.get_ref(styles).as_ref(),
            elem.fill.get_ref(styles).as_ref(),
            elem.stroke.resolve(styles),
            elem.inset.resolve(styles),
            elem.outset.resolve(styles),
            elem.radius.resolve(styles),
            base,
            content,
            styles,
            engine,
            locator,
        );
    }
    if let Some(elem) = content.to_packed::<EllipseElem>() {
        return box_shape(
            BoxKind::Ellipse,
            elem.width.get(styles).into(),
            elem.height.get(styles),
            elem.body.get_ref(styles).as_ref(),
            elem.fill.get_ref(styles).as_ref(),
            elem.stroke.resolve(styles).map(|s| Sides::splat(Some(s))),
            elem.inset.resolve(styles),
            elem.outset.resolve(styles),
            Corners::splat(None),
            base,
            content,
            styles,
            engine,
            locator,
        );
    }
    if let Some(elem) = content.to_packed::<CircleElem>() {
        return box_shape(
            BoxKind::Circle,
            elem.width.get(styles).into(),
            elem.height.get(styles),
            elem.body.get_ref(styles).as_ref(),
            elem.fill.get_ref(styles).as_ref(),
            elem.stroke.resolve(styles).map(|s| Sides::splat(Some(s))),
            elem.inset.resolve(styles),
            elem.outset.resolve(styles),
            Corners::splat(None),
            base,
            content,
            styles,
            engine,
            locator,
        );
    }
    if let Some(elem) = content.to_packed::<PolygonElem>() {
        return polygon_shape(elem, styles, base);
    }
    if let Some(elem) = content.to_packed::<CurveElem>() {
        return curve_shape(elem, styles, base);
    }
    if let Some(elem) = content.to_packed::<LineElem>() {
        return line_shape(elem, styles, base);
    }
    ResolvedShape {
        geometry: None,
        loss: None,
    }
}

/// `layout_shape` for the four body shapes. The element's declared
/// `width`/`height` land on the wrapping `BlockElem` in the realize pairs;
/// here they resolve the same way `unbreakable_pod` resolves them — Auto
/// (and `fr`, already folded into the region) takes the region, `Rel`
/// resolves against it — before the body's natural size or the default
/// `45pt`×`30pt` fills whatever is left undeclared.
#[allow(
    clippy::too_many_arguments,
    clippy::option_option,
    reason = "mirrors layout_shape's parameter list and field types"
)]
fn box_shape(
    kind: BoxKind,
    width: Sizing,
    height: Sizing,
    body: Option<&Content>,
    fill: Option<&Paint>,
    stroke: Smart<Sides<Option<Option<Stroke<Abs>>>>>,
    inset: Sides<Option<Rel<Abs>>>,
    outset: Sides<Option<Rel<Abs>>>,
    radius: Corners<Option<Rel<Abs>>>,
    base: Size,
    content: &Content,
    styles: StyleChain<'_>,
    engine: &mut Engine,
    locator: &mut SplitLocator,
) -> ResolvedShape {
    // `unbreakable_pod` with a zero block inset.
    let pod_size = Size::new(
        resolve_sizing(&width, base.x, styles),
        resolve_sizing(&height, base.y, styles),
    );
    let expand = Axes::new(
        width != Sizing::Auto && pod_size.x.is_finite(),
        height != Sizing::Auto && pod_size.y.is_finite(),
    );
    let region = Region::new(pod_size, expand);

    let mut losses: Vec<String> = Vec::new();
    let mut inset_abs = Sides::splat(Abs::zero());
    let mut frame_size;
    if let Some(child) = body {
        let mut inset = inset.unwrap_or_default();
        if kind.is_round() {
            inset = inset.map(|v| v + Ratio::new(0.5 - SQRT_2 / 4.0));
        }
        let has_inset = !inset.is_zero();
        let mut pod = region;
        if has_inset {
            pod.size = shrink_size(region.size, &inset);
        }
        if kind.is_quadratic() {
            let length = if let Some(length) = quadratic_size(pod) {
                length
            } else {
                let measured = typst_layout::layout_frame(
                    engine,
                    child,
                    locator.next(&content.span()),
                    styles,
                    pod,
                );
                match measured {
                    Ok(frame) => frame.size().max_by_side().min(pod.size.min_by_side()),
                    Err(_) => {
                        return unresolvable("the shape body's natural size could not be measured");
                    }
                }
            };
            pod = Region::new(Size::splat(length), Axes::splat(true));
        }
        match typst_layout::layout_frame(engine, child, locator.next(&content.span()), styles, pod)
        {
            Ok(frame) => frame_size = frame.size(),
            Err(_) => {
                return unresolvable("the shape body could not be laid out for sizing");
            }
        }
        if has_inset {
            // `pad::grow`: inverse of `shrink` — pad the natural size back
            // out so the body's inset sits inside the shape.
            let summed = inset.sum_by_axis();
            let padded = frame_size.zip_map(summed, |s, p| (s + p.abs) / (1.0 - p.rel.get()));
            frame_size = padded;
            inset_abs = inset.relative_to(padded);
        }
    } else {
        let default = Size::new(Abs::pt(45.0), Abs::pt(30.0)).min(region.size);
        frame_size = if kind.is_quadratic() {
            Size::splat(quadratic_size(region).unwrap_or_else(|| default.min_by_side()))
        } else {
            region.expand.select(region.size, default)
        };
    }

    // `fill_and_stroke` draws at the frame size grown by `outset`; a
    // `wp:inline` extent cannot paint outside itself, so a non-zero outset
    // is reported and dropped.
    let outset_abs = outset.unwrap_or_default().relative_to(frame_size);
    if !outset_abs.is_zero() {
        losses.push(
            "the shape's outset draws outside the drawing extent and was dropped".to_string(),
        );
    }
    let drawn = frame_size + outset_abs.sum_by_axis();

    let stroke_sides = match stroke {
        Smart::Auto if fill.is_none() => Sides::splat(Some(FixedStroke::default())),
        Smart::Auto => Sides::splat(None),
        Smart::Custom(strokes) => strokes
            .unwrap_or_default()
            .map(|s| s.map(Stroke::unwrap_or_default)),
    };
    let (stroke, stroke_loss) = sides_stroke(&stroke_sides);
    losses.extend(stroke_loss);
    losses.extend(solid_fill_loss(fill));

    let outline = if kind.is_round() {
        ShapeOutline::Ellipse
    } else {
        rounded_outline(&resolved_radii(drawn, &stroke_sides, &radius), drawn)
    };
    finish(drawn, outline, stroke, inset_abs, &losses)
}

/// `layout_polygon`: vertices resolve against the region, the drawn size is
/// the component-wise max, and the path is a closed polygon.
fn polygon_shape(elem: &Packed<PolygonElem>, styles: StyleChain<'_>, base: Size) -> ResolvedShape {
    let points: Vec<Point> = elem
        .vertices
        .iter()
        .map(|c| c.resolve(styles).zip_map(base, Rel::relative_to).to_point())
        .collect();
    let size = points
        .iter()
        .fold(Point::zero(), |max, c| c.max(max))
        .to_size();
    if points.is_empty() {
        // Typst draws an empty frame — nothing to emit, nothing lost.
        return ResolvedShape {
            geometry: None,
            loss: None,
        };
    }
    if !size.is_finite() {
        return unresolvable("the polygon has an infinite size");
    }
    let mut segments = Vec::with_capacity(points.len() + 2);
    segments.push(seg_move(points[0]));
    for point in &points[1..] {
        segments.push(seg_line(*point));
    }
    segments.push(ShapePathSeg::Close);
    let fill = elem.fill.get_ref(styles);
    let (stroke, mut losses) = path_stroke(
        elem.stroke.resolve(styles),
        fill.as_ref(),
        elem.fill_rule.get(styles),
    );
    losses.extend(solid_fill_loss(fill.as_ref()));
    let outline = ShapeOutline::Custom {
        path_w_emu: emu_i(size.x),
        path_h_emu: emu_i(size.y),
        segments,
    };
    finish(size, outline, stroke, Sides::splat(Abs::zero()), &losses)
}

/// `layout_curve` + `CurveBuilder`: `curve.move/line/quad/cubic/close`
/// resolve against the region (relative coordinates accumulate), quads
/// become cubics, and the drawn size tracks every point plus the cubic
/// extrema — exactly the `a:custGeom` path Typst draws.
fn curve_shape(elem: &Packed<CurveElem>, styles: StyleChain<'_>, base: Size) -> ResolvedShape {
    let mut builder = CurveBuilder::new(base, styles);
    for item in &elem.components {
        match item {
            CurveComponent::Move(element) => {
                let relative = element.relative.get(styles);
                let point = builder.resolve_point(element.start, relative);
                builder.move_(point);
            }
            CurveComponent::Line(element) => {
                let relative = element.relative.get(styles);
                let point = builder.resolve_point(element.end, relative);
                builder.line(point);
            }
            CurveComponent::Quad(element) => {
                let relative = element.relative.get(styles);
                let end = builder.resolve_point(element.end, relative);
                let control = match element.control {
                    Smart::Auto => control_c2q(builder.last_point, builder.last_control_from),
                    Smart::Custom(Some(p)) => builder.resolve_point(p, relative),
                    Smart::Custom(None) => end,
                };
                builder.quad(control, end);
            }
            CurveComponent::Cubic(element) => {
                let relative = element.relative.get(styles);
                let end = builder.resolve_point(element.end, relative);
                let c1 = match element.control_start {
                    Some(Smart::Custom(p)) => builder.resolve_point(p, relative),
                    Some(Smart::Auto) => builder.last_control_from,
                    None => builder.last_point,
                };
                let c2 = match element.control_end {
                    Some(p) => builder.resolve_point(p, relative),
                    None => end,
                };
                builder.cubic(c1, c2, end);
            }
            CurveComponent::Close(element) => {
                builder.close(element.mode.get(styles));
            }
        }
    }
    let (segments, size) = builder.finish();
    if segments.is_empty() {
        return ResolvedShape {
            geometry: None,
            loss: None,
        };
    }
    if !size.is_finite() {
        return unresolvable("the curve has an infinite size");
    }
    let fill = elem.fill.get_ref(styles);
    let (stroke, mut losses) = path_stroke(
        elem.stroke.resolve(styles),
        fill.as_ref(),
        elem.fill_rule.get(styles),
    );
    losses.extend(solid_fill_loss(fill.as_ref()));
    let outline = ShapeOutline::Custom {
        path_w_emu: emu_i(size.x),
        path_h_emu: emu_i(size.y),
        segments,
    };
    finish(size, outline, stroke, Sides::splat(Abs::zero()), &losses)
}

/// `layout_line`: `start`/`end` (or `length` + `angle`) resolve against
/// the region; the drawn size covers both endpoints. A corner-to-corner
/// segment maps to the `line` preset with `a:xfrm` flips; anything else
/// becomes a two-point `custGeom`.
fn line_shape(elem: &Packed<LineElem>, styles: StyleChain<'_>, base: Size) -> ResolvedShape {
    let resolve = |axes: Axes<Rel<Abs>>| axes.zip_map(base, Rel::relative_to);
    let start = resolve(elem.start.resolve(styles));
    let delta = elem.end.resolve(styles).map_or_else(
        || {
            let length = elem.length.resolve(styles);
            let angle = elem.angle.get(styles);
            let x = angle.cos() * length;
            let y = angle.sin() * length;
            resolve(Axes::new(x, y))
        },
        |end| resolve(end) - start,
    );
    let size = start.max(start + delta).max(Size::zero());
    if !size.is_finite() {
        return unresolvable("the line has an infinite length");
    }
    let stroke = elem.stroke.resolve(styles).unwrap_or_default();
    let (stroke, losses) = fixed_stroke(&stroke);
    let p0 = start.to_point();
    let p1 = (start + delta).to_point();
    let w = size.x;
    let h = size.y;
    let zero = Abs::zero();
    let outline = if p0 == Point::new(zero, zero) && p1 == Point::new(w, h) {
        ShapeOutline::Line {
            flip_h: false,
            flip_v: false,
        }
    } else if p0 == Point::new(w, zero) && p1 == Point::new(zero, h) {
        ShapeOutline::Line {
            flip_h: true,
            flip_v: false,
        }
    } else if p0 == Point::new(zero, h) && p1 == Point::new(w, zero) {
        ShapeOutline::Line {
            flip_h: false,
            flip_v: true,
        }
    } else if p0 == Point::new(w, h) && p1 == Point::new(zero, zero) {
        ShapeOutline::Line {
            flip_h: true,
            flip_v: true,
        }
    } else {
        ShapeOutline::Custom {
            path_w_emu: emu_i(w),
            path_h_emu: emu_i(h),
            segments: vec![seg_move(p0), seg_line(p1)],
        }
    };
    finish(size, outline, stroke, Sides::splat(Abs::zero()), &losses)
}

/// Sizing resolution from `unbreakable_pod`: `Auto` and `fr` take the
/// region (the `fr` share is already folded into it), `Rel` resolves
/// against it.
fn resolve_sizing(sizing: &Sizing, base: Abs, styles: StyleChain<'_>) -> Abs {
    match sizing {
        Sizing::Auto | Sizing::Fr(_) => base,
        Sizing::Rel(rel) => rel.resolve(styles).relative_to(base),
    }
}

/// `pad::shrink` — subtract the per-axis inset resolved against the size.
fn shrink_size(size: Size, inset: &Sides<Rel<Abs>>) -> Size {
    size - inset.sum_by_axis().relative_to(size)
}

/// `quadratic_size` — the forced length of a square/circle: both axes
/// declared → the smaller; one declared → that one.
fn quadratic_size(region: Region) -> Option<Abs> {
    if region.expand.x && region.expand.y {
        Some(region.size.x.min(region.size.y))
    } else if region.expand.x {
        Some(region.size.x)
    } else if region.expand.y {
        Some(region.size.y)
    } else {
        None
    }
}

/// `clip_rect`'s corner resolution: the radius cap per corner is half the
/// drawn size plus the thinner adjacent stroke, then each declared radius
/// resolves against twice the cap and clamps to it.
fn resolved_radii(
    size: Size,
    stroke: &Sides<Option<FixedStroke>>,
    radius: &Corners<Option<Rel<Abs>>>,
) -> Corners<Abs> {
    let stroke_widths = stroke
        .as_ref()
        .map(|s| s.as_ref().map(|s| s.thickness / 2.0));
    let base_radius = size.x.min(size.y) / 2.0;
    let corner_max =
        stroke_widths.map_corners(|a, b| base_radius + a.min(b).unwrap_or(Abs::zero()));
    radius
        .map(Option::unwrap_or_default)
        .zip(corner_max)
        .map(|(value, max)| value.relative_to(max * 2.0).min(max))
}

/// Preset selection for a rounded rect: all-equal corners → `roundRect`,
/// top/bottom pairs → `round2SameRect` (`adj1` top, `adj2` bottom);
/// anything else (a single non-top corner, diagonal pairs, three distinct
/// radii) is a freeform path of cubic-approximated corner arcs — the exact
/// outline rather than a wrong preset.
fn rounded_outline(radii: &Corners<Abs>, size: Size) -> ShapeOutline {
    let ss = size.x.min(size.y);
    if !ss.is_finite() || ss <= Abs::zero() {
        return ShapeOutline::Rect;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "adj is a non-negative 0–50000 fraction"
    )]
    let to_adj = |r: Abs| (r / ss * 100_000.0).round().clamp(0.0, 50_000.0) as u32;
    let (tl, tr, br, bl) = (
        radii.top_left,
        radii.top_right,
        radii.bottom_right,
        radii.bottom_left,
    );
    if tl.is_zero() && tr.is_zero() && br.is_zero() && bl.is_zero() {
        ShapeOutline::Rect
    } else if tl == tr && tl == br && tl == bl {
        ShapeOutline::RoundRect { adj: to_adj(tl) }
    } else if tl == tr && br == bl {
        ShapeOutline::Round2SameRect {
            adj1: to_adj(tl),
            adj2: to_adj(br),
        }
    } else {
        ShapeOutline::Custom {
            path_w_emu: emu_i(size.x),
            path_h_emu: emu_i(size.y),
            segments: rounded_rect_segments(size.x, size.y, radii),
        }
    }
}

/// Freeform path for a rect with non-preset corner radii: straight edges
/// joined by quarter-circle cubic arcs (`κ = 4/3·(√2−1)`, the standard
/// circle approximation).
fn rounded_rect_segments(w: Abs, h: Abs, radii: &Corners<Abs>) -> Vec<ShapePathSeg> {
    const KAPPA: f64 = 0.552_284_749_830_793_6;
    let (tl, tr, br, bl) = (
        radii.top_left,
        radii.top_right,
        radii.bottom_right,
        radii.bottom_left,
    );
    let zero = Abs::zero();
    let mut segs = Vec::with_capacity(9);
    segs.push(seg_move(Point::new(tl, zero)));
    segs.push(seg_line(Point::new(w - tr, zero)));
    if !tr.is_zero() {
        let k = tr * KAPPA;
        segs.push(seg_cubic(
            Point::new(w - tr + k, zero),
            Point::new(w, tr - k),
            Point::new(w, tr),
        ));
    }
    segs.push(seg_line(Point::new(w, h - br)));
    if !br.is_zero() {
        let k = br * KAPPA;
        segs.push(seg_cubic(
            Point::new(w, h - br + k),
            Point::new(w - br + k, h),
            Point::new(w - br, h),
        ));
    }
    segs.push(seg_line(Point::new(bl, h)));
    if !bl.is_zero() {
        let k = bl * KAPPA;
        segs.push(seg_cubic(
            Point::new(bl - k, h),
            Point::new(zero, h - bl + k),
            Point::new(zero, h - bl),
        ));
    }
    segs.push(seg_line(Point::new(zero, tl)));
    if !tl.is_zero() {
        let k = tl * KAPPA;
        segs.push(seg_cubic(
            Point::new(zero, tl - k),
            Point::new(tl - k, zero),
            Point::new(tl, zero),
        ));
    }
    segs.push(ShapePathSeg::Close);
    segs
}

/// A `Sides<Option<FixedStroke>>` → one `a:ln`. Uniform sides map
/// directly; per-side strokes cannot ride on a single OOXML shape and are
/// reported and dropped (the fill and geometry still draw).
fn sides_stroke(sides: &Sides<Option<FixedStroke>>) -> (Option<ShapeStroke>, Vec<String>) {
    if !sides.is_uniform() {
        return (
            None,
            vec![
                "per-side strokes are not representable on one DrawingML shape; \
                 the stroke was dropped"
                    .to_string(),
            ],
        );
    }
    match &sides.top {
        Some(fixed) => fixed_stroke(fixed),
        None => (None, Vec::new()),
    }
}

/// `FixedStroke` → `a:ln`: thickness, cap, join (with the miter limit),
/// solid colour, and the dash array as `custDash` pairs. A non-solid paint
/// or a non-zero dash `phase` has no `a:ln` equivalent — reported.
fn fixed_stroke(fixed: &FixedStroke) -> (Option<ShapeStroke>, Vec<String>) {
    let mut losses = Vec::new();
    let color_hex = if let Paint::Solid(_) = &fixed.paint {
        paint_to_shd_hex(&fixed.paint)
    } else {
        losses.push(
            "a non-solid stroke paint (gradient/tiling) is not representable; \
             the stroke draws black"
                .to_string(),
        );
        None
    };
    let dash = match &fixed.dash {
        None => None,
        Some(pattern) if pattern.array.is_empty() => None,
        Some(pattern) => {
            if !pattern.phase.is_zero() {
                losses.push(
                    "a dash pattern phase is not representable; the dash starts at zero"
                        .to_string(),
                );
            }
            Some(dash_pairs(&pattern.array, fixed.thickness))
        }
    };
    let cap = match fixed.cap {
        LineCap::Butt => ShapeCap::Flat,
        LineCap::Round => ShapeCap::Round,
        LineCap::Square => ShapeCap::Square,
    };
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the miter limit is a small positive ratio"
    )]
    let join = match fixed.join {
        LineJoin::Miter => ShapeJoin::Miter {
            lim: (fixed.miter_limit.get() * 100_000.0).round().max(0.0) as u32,
        },
        LineJoin::Round => ShapeJoin::Round,
        LineJoin::Bevel => ShapeJoin::Bevel,
    };
    (
        Some(ShapeStroke {
            width_emu: pt_to_emu(fixed.thickness.to_pt()),
            color_hex,
            cap,
            join,
            dash,
        }),
        losses,
    )
}

/// `a:ds` pairs from a dash array: each entry is an absolute length
/// (`DashLength::LineWidth` was already folded to the thickness), written
/// in 100000ths of the stroke width. An odd-length array repeats, so it is
/// doubled before pairing — SVG/typst semantics exactly.
fn dash_pairs(array: &[Abs], thickness: Abs) -> Vec<(u32, u32)> {
    let width_pt = thickness.to_pt().max(f64::EPSILON);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "dash lengths are non-negative"
    )]
    let to_pct = |a: &Abs| (a.to_pt() / width_pt * 100_000.0).round().max(0.0) as u32;
    let mut values: Vec<u32> = array.iter().map(to_pct).collect();
    if values.len() % 2 == 1 {
        let doubled = values.clone();
        values.extend(doubled);
    }
    values
        .as_chunks::<2>()
        .0
        .iter()
        .map(|[dash, gap]| (*dash, *gap))
        .collect()
}

/// Polygon/curve stroke resolution: `auto` gives the `1pt` black default
/// only when there is no fill (same as `layout_curve`/`layout_polygon`).
fn path_stroke(
    stroke: Smart<Option<Stroke<Abs>>>,
    fill: Option<&Paint>,
    fill_rule: FillRule,
) -> (Option<ShapeStroke>, Vec<String>) {
    let fixed = match stroke {
        Smart::Auto if fill.is_none() => Some(FixedStroke::default()),
        Smart::Auto => None,
        Smart::Custom(stroke) => stroke.map(Stroke::unwrap_or_default),
    };
    let (shape_stroke, mut losses) = match &fixed {
        Some(fixed) => fixed_stroke(fixed),
        None => (None, Vec::new()),
    };
    if fill.is_some() && fill_rule == FillRule::EvenOdd {
        losses.push(
            "the even-odd fill rule is not representable; the path fills non-zero".to_string(),
        );
    }
    (shape_stroke, losses)
}

/// A non-solid fill (`gradient`/`tiling`/pattern) has no `a:solidFill`;
/// the shape draws unfilled and the loss is reported. The hex itself stays
/// on `IrBlock::Shape::fill_hex` / `IrInline::Shape::fill_hex` — this only
/// detects the loss.
fn solid_fill_loss(fill: Option<&Paint>) -> Option<String> {
    match fill {
        None | Some(Paint::Solid(_)) => None,
        Some(_) => Some(
            "a non-solid fill (gradient/tiling) is not representable; the shape is unfilled"
                .to_string(),
        ),
    }
}

/// Assemble the [`IrShapeGeometry`], treating a non-finite or empty drawn
/// size as unresolvable (Typst bails on infinite geometry; a zero-size
/// shape draws nothing).
fn finish(
    size: Size,
    outline: ShapeOutline,
    stroke: Option<ShapeStroke>,
    inset: Sides<Abs>,
    losses: &[String],
) -> ResolvedShape {
    if !size.is_finite() {
        return unresolvable("the shape's size could not be resolved to a finite extent");
    }
    if size.x <= Abs::zero() || size.y <= Abs::zero() {
        return ResolvedShape {
            geometry: None,
            loss: None,
        };
    }
    ResolvedShape {
        geometry: Some(IrShapeGeometry {
            width_emu: pt_to_emu(size.x.to_pt()),
            height_emu: pt_to_emu(size.y.to_pt()),
            outline,
            stroke,
            inset_emu: ShapeInsets {
                left_emu: pt_to_emu(inset.left.to_pt()),
                top_emu: pt_to_emu(inset.top.to_pt()),
                right_emu: pt_to_emu(inset.right.to_pt()),
                bottom_emu: pt_to_emu(inset.bottom.to_pt()),
            },
        }),
        loss: joined_loss(losses),
    }
}

fn unresolvable(reason: &str) -> ResolvedShape {
    ResolvedShape {
        geometry: None,
        loss: Some(format!(
            "{reason}; the shape's text was kept without its geometry"
        )),
    }
}

fn joined_loss(losses: &[String]) -> Option<String> {
    if losses.is_empty() {
        None
    } else {
        Some(losses.join("; "))
    }
}

/// Signed pt → EMU for path coordinates — `pt_to_emu` clamps to zero, but
/// `a:pt` legitimately carries negative curve control points.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "path EMU magnitudes are far below i64 range"
)]
fn emu_i(abs: Abs) -> i64 {
    (abs.to_pt() * 12_700.0).round() as i64
}

fn seg_move(p: Point) -> ShapePathSeg {
    ShapePathSeg::MoveTo {
        x_emu: emu_i(p.x),
        y_emu: emu_i(p.y),
    }
}

fn seg_line(p: Point) -> ShapePathSeg {
    ShapePathSeg::LineTo {
        x_emu: emu_i(p.x),
        y_emu: emu_i(p.y),
    }
}

fn seg_cubic(c1: Point, c2: Point, end: Point) -> ShapePathSeg {
    ShapePathSeg::CubicTo {
        c1x_emu: emu_i(c1.x),
        c1y_emu: emu_i(c1.y),
        c2x_emu: emu_i(c2.x),
        c2y_emu: emu_i(c2.y),
        x_emu: emu_i(end.x),
        y_emu: emu_i(end.y),
    }
}

enum Seg {
    Move(Point),
    Line(Point),
    Cubic(Point, Point, Point),
    Close,
}

/// `CurveBuilder` mirrored on [`Seg`] so quads, auto controls, smooth
/// closes, and the bounds bookkeeping match `layout_curve` exactly.
struct CurveBuilder<'a> {
    curve: Vec<Seg>,
    size: Size,
    region: Size,
    styles: StyleChain<'a>,
    start_point: Point,
    start_control_into: Point,
    last_point: Point,
    last_control_from: Point,
    is_started: bool,
    is_empty: bool,
}

impl<'a> CurveBuilder<'a> {
    fn new(region: Size, styles: StyleChain<'a>) -> Self {
        Self {
            curve: Vec::new(),
            size: Size::zero(),
            region,
            styles,
            start_point: Point::zero(),
            start_control_into: Point::zero(),
            last_point: Point::zero(),
            last_control_from: Point::zero(),
            is_started: false,
            is_empty: true,
        }
    }

    fn finish(self) -> (Vec<ShapePathSeg>, Size) {
        let segments = self
            .curve
            .into_iter()
            .map(|seg| match seg {
                Seg::Move(p) => seg_move(p),
                Seg::Line(p) => seg_line(p),
                Seg::Cubic(c1, c2, end) => seg_cubic(c1, c2, end),
                Seg::Close => ShapePathSeg::Close,
            })
            .collect();
        (segments, self.size)
    }

    fn move_(&mut self, point: Point) {
        self.expand_bounds(point);
        self.start_point = point;
        self.start_control_into = point;
        self.last_point = point;
        self.last_control_from = point;
        self.is_started = true;
        self.is_empty = true;
    }

    fn line(&mut self, point: Point) {
        if self.is_empty {
            self.start_component();
            self.start_control_into = self.start_point;
        }
        self.curve.push(Seg::Line(point));
        self.expand_bounds(point);
        self.last_point = point;
        self.last_control_from = point;
    }

    fn quad(&mut self, control: Point, end: Point) {
        let c1 = control_q2c(self.last_point, control);
        let c2 = control_q2c(end, control);
        self.cubic(c1, c2, end);
    }

    fn cubic(&mut self, c1: Point, c2: Point, end: Point) {
        if self.is_empty {
            self.start_component();
            self.start_control_into = mirror_c(self.start_point, c1);
        }
        self.curve.push(Seg::Cubic(c1, c2, end));
        let (x_max, y_max) = cubic_axis_max(self.last_point, c1, c2, end);
        self.size.x.set_max(Abs::raw(x_max));
        self.size.y.set_max(Abs::raw(y_max));
        self.last_point = end;
        self.last_control_from = mirror_c(end, c2);
    }

    fn close(&mut self, mode: CloseMode) {
        if self.is_started && !self.is_empty {
            if mode == CloseMode::Smooth {
                self.cubic(
                    self.last_control_from,
                    self.start_control_into,
                    self.start_point,
                );
            }
            self.curve.push(Seg::Close);
            self.last_point = self.start_point;
            self.last_control_from = self.start_point;
        }
        self.is_started = false;
        self.is_empty = true;
    }

    fn start_component(&mut self) {
        self.curve.push(Seg::Move(self.start_point));
        self.is_empty = false;
        self.is_started = true;
    }

    fn expand_bounds(&mut self, point: Point) {
        self.size.x.set_max(point.x);
        self.size.y.set_max(point.y);
    }

    fn resolve_point(&self, point: Axes<Rel>, relative: bool) -> Point {
        let mut p = point
            .resolve(self.styles)
            .zip_map(self.region, Rel::relative_to)
            .to_point();
        if relative {
            p += self.last_point;
        }
        p
    }
}

fn control_c2q(p: Point, c: Point) -> Point {
    c * 1.5 - p * 0.5
}

fn control_q2c(p: Point, c: Point) -> Point {
    (p + c * 2.0) / 3.0
}

fn mirror_c(p: Point, c: Point) -> Point {
    p * 2.0 - c
}

/// The maximum extent of a cubic Bézier segment per axis — kurbo's
/// `CubicBez::bounding_box` max corner, computed from the derivative's
/// roots so the shape's extent covers the rendered ink.
fn cubic_axis_max(p0: Point, c1: Point, c2: Point, p1: Point) -> (f64, f64) {
    (
        cubic_axis_max_1d(p0.x.to_raw(), c1.x.to_raw(), c2.x.to_raw(), p1.x.to_raw()),
        cubic_axis_max_1d(p0.y.to_raw(), c1.y.to_raw(), c2.y.to_raw(), p1.y.to_raw()),
    )
}

fn cubic_axis_max_1d(p0: f64, c1: f64, c2: f64, p1: f64) -> f64 {
    let mut best = p0.max(p1);
    // B'(t)/3 = A t² + B t + C — solve for t in (0,1).
    let a = -p0 + 3.0 * c1 - 3.0 * c2 + p1;
    let b = 2.0 * (p0 - 2.0 * c1 + c2);
    let c = c1 - p0;
    for t in quadratic_roots(a, b, c) {
        if t > 0.0 && t < 1.0 {
            let mt = 1.0 - t;
            let v =
                mt * mt * mt * p0 + 3.0 * mt * mt * t * c1 + 3.0 * mt * t * t * c2 + t * t * t * p1;
            best = best.max(v);
        }
    }
    best
}

fn quadratic_roots(a: f64, b: f64, c: f64) -> Vec<f64> {
    const EPS: f64 = 1e-12;
    if a.abs() < EPS {
        if b.abs() < EPS {
            return Vec::new();
        }
        return vec![-c / b];
    }
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return Vec::new();
    }
    let sq = disc.sqrt();
    vec![(-b + sq) / (2.0 * a), (-b - sq) / (2.0 * a)]
}
