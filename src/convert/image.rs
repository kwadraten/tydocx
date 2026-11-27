use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use crate::ooxml::document::{ImageData, ImageFormat, SrcRect};
use typst::foundations::{Packed, StyleChain};
use typst::introspection::{Location, Tag};
use typst::layout::{Abs, Frame, FrameItem, Point, Rect};
use typst_html::HtmlFrame;
use typst_layout::PagedDocument;
use typst_library::visualize::{ImageElem, ImageFit};
use typst_syntax::Span;

use crate::compat::{Degradation, DegradationAction};

/// One rendered image's display size and optional crop window.
#[derive(Clone, PartialEq, Eq)]
pub(super) struct ImagePlacement {
    /// On-page size of the visible (post-clip) area, EMU.
    pub size_emu: (u64, u64),
    /// `a:srcRect` crop derived from an enclosing `Frame::clip` window — the
    /// way Typst encodes both `block/box(clip:)` and `image(fit: "cover")`.
    /// The full image bytes stay embedded; Word crops at display time.
    pub src_rect: Option<SrcRect>,
}

#[derive(Default)]
pub(super) struct ImagePlacements {
    source: HashMap<u64, Option<ImagePlacement>>,
    span: HashMap<(Span, u64), Option<ImagePlacement>>,
    bytes: HashMap<u64, Option<ImagePlacement>>,
}

impl ImagePlacements {
    pub(super) fn get(
        &self,
        source: u64,
        span: Option<Span>,
        bytes: u64,
    ) -> Option<&ImagePlacement> {
        self.source
            .get(&source)
            .or_else(|| span.and_then(|span| self.span.get(&(span, bytes))))
            .or_else(|| self.bytes.get(&bytes))
            .and_then(Option::as_ref)
    }
}

pub(super) fn image_source_key(
    image: &Packed<ImageElem>,
    styles: StyleChain<'_>,
    bytes: u64,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    (
        image.span(),
        image.width.get(styles),
        image.height.get(styles),
        image.fit.get(styles),
        bytes,
    )
        .hash(&mut hasher);
    hasher.finish()
}

pub(super) fn collect_image_placements(paged: &PagedDocument) -> ImagePlacements {
    let mut placements = ImagePlacements::default();
    for page in paged.pages() {
        collect_frame_images(
            &page.frame,
            Point::zero(),
            None,
            &mut Vec::new(),
            &mut |span, bytes, source, placement| {
                if let Some(source) = source {
                    record_placement(&mut placements.source, source, placement);
                }
                record_placement(&mut placements.span, (span, bytes), placement);
                record_placement(&mut placements.bytes, bytes, placement);
            },
        );
    }
    placements
}

fn record_placement<K: Eq + Hash>(
    map: &mut HashMap<K, Option<ImagePlacement>>,
    key: K,
    placement: &ImagePlacement,
) {
    map.entry(key)
        .and_modify(|value| {
            if value.as_ref() != Some(placement) {
                *value = None;
            }
        })
        .or_insert_with(|| Some(placement.clone()));
}

/// Image CONTENT comes from the HTML `<img>` src data-URL (the semantic
/// source, see `image_data_from_src`); the paged frames contribute only the
/// on-page DISPLAY SIZE and crop geometry. This maps a hash of each image's
/// raw bytes to its layouted size in EMU, so an `<img>` can look up how large
/// Typst actually drew it. Content and size are matched by bytes, not by
/// position — there is no ordering to desync.
pub(super) fn collect_image_sizes(paged: &PagedDocument) -> HashMap<u64, ImagePlacement> {
    let mut sizes = HashMap::new();
    for page in paged.pages() {
        collect_frame_images(
            &page.frame,
            Point::zero(),
            None,
            &mut Vec::new(),
            &mut |_, bytes, _, placement| {
                sizes.entry(bytes).or_insert_with(|| placement.clone());
            },
        );
    }
    sizes
}

/// `offset` is the frame's absolute page position; `clip` is the visible
/// window accumulated from enclosing `GroupItem::clip` curves, in the same
/// coordinates.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "positive pt sizes rounded to whole pixels"
)]
fn collect_frame_images<'a>(
    frame: &'a Frame,
    offset: Point,
    clip: Option<Rect>,
    tags: &mut Vec<&'a Packed<ImageElem>>,
    record: &mut impl FnMut(Span, u64, Option<u64>, &ImagePlacement),
) {
    for (pos, item) in frame.items() {
        let absolute = offset + *pos;
        match item {
            FrameItem::Tag(Tag::Start(content, _)) => {
                if let Some(image) = content.to_packed::<ImageElem>() {
                    tags.push(image);
                }
            }
            FrameItem::Tag(Tag::End(location, ..)) => {
                if tags.last().and_then(|image| image.location()) == Some(*location) {
                    tags.pop();
                }
            }
            FrameItem::Image(img, size, span) => {
                let bytes: Option<&[u8]> = match img.kind() {
                    typst_library::visualize::ImageKind::Raster(r) => Some(r.data().as_ref()),
                    typst_library::visualize::ImageKind::Svg(s) => Some(s.data().as_ref()),
                    typst_library::visualize::ImageKind::Pdf(_) => None,
                };
                if let Some(bytes) = bytes {
                    let bounds = Rect::from_pos_size(absolute, *size);
                    let visible = clip.map_or(bounds, |window| intersect(window, bounds));
                    let hash = hash_bytes(bytes);
                    let source = tags
                        .last()
                        .filter(|tag| tag.span() == *span)
                        .map(|tag| image_source_key(tag, StyleChain::default(), hash));
                    record(
                        *span,
                        hash,
                        source,
                        &ImagePlacement {
                            size_emu: (
                                (visible.size().x.to_pt() * 12700.0) as u64,
                                (visible.size().y.to_pt() * 12700.0) as u64,
                            ),
                            src_rect: (visible != bounds).then(|| crop_fractions(bounds, visible)),
                        },
                    );
                }
            }
            FrameItem::Group(group) => {
                // A non-identity transform moves contents off their declared
                // positions, so a clip window cannot be propagated through it.
                let sub_clip = if group.transform.is_identity() {
                    match &group.clip {
                        Some(curve) => {
                            let window = offset_rect(curve.bbox(None), absolute);
                            Some(clip.map_or(window, |outer| intersect(outer, window)))
                        }
                        None => clip,
                    }
                } else {
                    None
                };
                collect_frame_images(&group.frame, absolute, sub_clip, tags, record);
            }
            _ => {}
        }
    }
}

fn intersect(a: Rect, b: Rect) -> Rect {
    Rect::new(
        Point::new(a.min.x.max(b.min.x), a.min.y.max(b.min.y)),
        Point::new(a.max.x.min(b.max.x), a.max.y.min(b.max.y)),
    )
}

fn offset_rect(rect: Rect, offset: Point) -> Rect {
    Rect::new(offset + rect.min, offset + rect.max)
}

/// The visible window as a fraction of the full image bounds, in `a:srcRect`
/// units (thousandths of a percent).
#[allow(
    clippy::cast_possible_truncation,
    reason = "a crop fraction clamps into 0..=100000"
)]
fn crop_fractions(bounds: Rect, visible: Rect) -> SrcRect {
    let width = bounds.size().x.to_pt();
    let height = bounds.size().y.to_pt();
    let side = |num: Abs, den: f64| {
        (num.to_pt() / den.max(f64::EPSILON) * 100_000.0)
            .round()
            .clamp(0.0, 100_000.0) as i32
    };
    SrcRect {
        l: side(visible.min.x - bounds.min.x, width),
        t: side(visible.min.y - bounds.min.y, height),
        r: side(bounds.max.x - visible.max.x, width),
        b: side(bounds.max.y - visible.max.y, height),
    }
}

pub(crate) fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

/// Build embeddable `ImageData` from an HTML `<img>` src attribute.
///
/// typst-html 0.15 embeds every image as `data:<mime>;base64,<data>` — the
/// image's own bytes travel WITH the `<img>` element, so content can never be
/// attached to the wrong tag (the failure mode of the old paged-order FIFO).
/// PNG/JPEG embed directly; GIF/WebP re-encode to PNG (Word can't embed
/// them); SVG (including PDFs, which typst-html converts to SVG) rasterizes
/// to PNG. `sizes` supplies the on-page display size by content hash, with
/// the intrinsic dimensions from the decoded data as fallback.
pub(super) fn image_data_from_src(
    src: &str,
    sizes: &HashMap<u64, ImagePlacement>,
) -> Option<ImageData> {
    use base64::Engine as _;

    let rest = src.strip_prefix("data:")?;
    let (mime, b64) = rest.split_once(";base64,")?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    let placement = sizes.get(&hash_bytes(&bytes));
    let layout_size = placement.map(|p| p.size_emu);
    let src_rect = placement.and_then(|p| p.src_rect);

    // Intrinsic pixel size → EMU (96 px/inch), used when the paged size is
    // unavailable (e.g. HTML-only conversion).
    let px_to_emu = |px: u32| u64::from(px) * 9525;

    match mime {
        "image/png" | "image/jpeg" => {
            let format = if mime == "image/png" {
                ImageFormat::Png
            } else {
                ImageFormat::Jpeg
            };
            let (w, h) = layout_size.or_else(|| {
                let dims = ::image::load_from_memory(&bytes).ok()?;
                Some((px_to_emu(dims.width()), px_to_emu(dims.height())))
            })?;
            Some(ImageData {
                bytes,
                format,
                svg: None,
                width_emu: w,
                height_emu: h,
                src_rect,
                alt: None,
            })
        }
        "image/gif" | "image/webp" => {
            let decoded = ::image::load_from_memory(&bytes).ok()?;
            let (w, h) =
                layout_size.unwrap_or((px_to_emu(decoded.width()), px_to_emu(decoded.height())));
            let mut png = Vec::new();
            decoded
                .write_to(
                    &mut std::io::Cursor::new(&mut png),
                    ::image::ImageFormat::Png,
                )
                .ok()?;
            Some(ImageData {
                bytes: png,
                format: ImageFormat::Png,
                svg: None,
                width_emu: w,
                height_emu: h,
                src_rect,
                alt: None,
            })
        }
        // Intentional f32→u32: SVG canvas dimensions are small positive values.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        "image/svg+xml" => {
            let mut options = resvg::usvg::Options::default();
            options.fontdb_mut().load_system_fonts();
            let tree = resvg::usvg::Tree::from_data(&bytes, &options).ok()?;
            let svg_size = tree.size();
            let pixel_w = svg_size.width().ceil() as u32;
            let pixel_h = svg_size.height().ceil() as u32;
            if pixel_w == 0 || pixel_h == 0 {
                return None;
            }
            let mut pixmap = tiny_skia::Pixmap::new(pixel_w, pixel_h)?;
            resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());
            let png = pixmap.encode_png().ok()?;
            let (w, h) = layout_size.unwrap_or((px_to_emu(pixel_w), px_to_emu(pixel_h)));
            Some(ImageData {
                bytes: png,
                format: ImageFormat::Png,
                svg: Some(bytes),
                width_emu: w,
                height_emu: h,
                src_rect,
                alt: None,
            })
        }
        _ => None,
    }
}

/// Whether an image `src` data-URL is a vector image (SVG) whose vector
/// aspect C1 drops by rasterizing it to PNG. Raster formats (PNG/JPEG)
/// keep their aspect; GIF/WebP re-encode to PNG, losing only animation.
#[must_use]
pub fn is_vector_image_src(src: &str) -> bool {
    src.starts_with("data:image/svg+xml")
}

/// In C1, report the aspects of one image the OOXML intersection drops
/// (U19): a non-cover `fit` (cover derives an `a:srcRect` from the measured
/// clip; `contain` letterboxes — neither behavior is declared on the OOXML
/// image) and a non-empty `alt` text (no alt-text slot is written). The image
/// itself is kept; only these aspects are reported.
pub(super) fn report_image_aspects(
    image: &Packed<ImageElem>,
    span: Option<Span>,
    degradations: &mut Vec<Degradation>,
) {
    if let Some(fit) = image.fit.as_option()
        && *fit != ImageFit::Cover
    {
        degradations.push(Degradation::new(
            span,
            "image-fit",
            DegradationAction::Skipped,
            "image fit is not cover; the fit behavior is dropped",
        ));
    }
    if let Some(Some(alt)) = image.alt.as_option()
        && !alt.is_empty()
    {
        degradations.push(Degradation::new(
            span,
            "image-alt",
            DegradationAction::Skipped,
            "image alt text is dropped",
        ));
    }
}

/// Rasterize an `HtmlNode::Frame` — layouted content explicitly embedded into
/// the HTML export (`html.frame`), handed over as a laid-out frame in
/// document order. The docx embeds it as a PNG at its layout size.
pub(super) fn rasterize_html_frame(html_frame: &HtmlFrame) -> Option<ImageData> {
    rasterize_frame(&html_frame.inner)
}

/// Rasterize each figure's vector-drawing canvas, keyed by the OWNING
/// `FigureElem`'s introspection `Location`.
///
/// A drawing body (`#place`d curves, `CeTZ` canvases) is dropped entirely from
/// the HTML export — it exists only in the paged frames, whose tag brackets
/// (`Tag::Start`/`Tag::End` of each `FigureElem`) tell us exactly which
/// figure a canvas belongs to. Keyed consumption replaces the old page-order
/// FIFO, which attached rasters to whatever figure popped next: a quote or
/// code-listing figure stole the following canvas's image, and every later
/// drawing shifted one slot. Rasters of figures the HTML walk converts
/// through other paths (tables, images) simply stay unconsumed.
pub(super) fn extract_figure_rasters(paged: &PagedDocument) -> HashMap<Location, ImageData> {
    let mut rasters = HashMap::new();
    let mut stack: Vec<Location> = Vec::new();
    for page in paged.pages() {
        collect_figure_canvases(&page.frame, &mut stack, &mut rasters);
    }
    rasters
}

fn collect_figure_canvases(
    frame: &Frame,
    stack: &mut Vec<Location>,
    rasters: &mut HashMap<Location, ImageData>,
) {
    use typst::foundations::NativeElement;
    for (_, item) in frame.items() {
        match item {
            FrameItem::Tag(Tag::Start(content, _)) => {
                if content.elem() == typst_library::model::FigureElem::ELEM
                    && let Some(loc) = content.location()
                {
                    stack.push(loc);
                }
            }
            FrameItem::Tag(Tag::End(loc, ..)) => {
                if stack.last() == Some(loc) {
                    stack.pop();
                }
            }
            FrameItem::Group(group) => {
                if let Some(&owner) = stack.last()
                    && frame_has_curve(&group.frame)
                {
                    // Outermost curve-bearing group inside this figure: the
                    // canvas. First one wins; don't descend into it.
                    if !rasters.contains_key(&owner)
                        && let Some(data) = rasterize_frame(&group.frame)
                    {
                        rasters.insert(owner, data);
                    }
                } else {
                    collect_figure_canvases(&group.frame, stack, rasters);
                }
            }
            _ => {}
        }
    }
}

/// True if a frame (recursively) contains a Bézier curve shape — the signature
/// of vector line art. Tables and horizontal rules use only straight `Line`
/// geometry, so this never matches a table grid. Used to spot figure canvases
/// for rasterization.
pub(super) fn frame_has_curve(frame: &Frame) -> bool {
    frame.items().any(|(_, item)| match item {
        FrameItem::Shape(shape, _) => {
            matches!(shape.geometry, typst::visualize::Geometry::Curve(_))
        }
        FrameItem::Group(group) => frame_has_curve(&group.frame),
        _ => false,
    })
}

/// Rasterize a single drawing frame to a PNG, sized at its on-page layout
/// dimensions (so Word reproduces the figure's footprint).
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "positive pt sizes rounded to whole pixels / EMU"
)]
fn rasterize_frame(frame: &Frame) -> Option<ImageData> {
    use typst::foundations::{Content, Smart};
    use typst::layout::{Abs, Sides};
    use typst::utils::Scalar;
    use typst_layout::Page;
    use typst_render::RenderOptions;

    let size = frame.size();
    let (width_pt, height_pt) = (size.x.to_pt(), size.y.to_pt());
    if width_pt <= 0.0 || height_pt <= 0.0 {
        return None;
    }
    // 150 DPI: crisp line art without bloating the document.
    let pixel_per_pt = 150.0 / 72.0;
    let page = Page {
        frame: frame.clone(),
        // No bleed: we rasterize the figure's own footprint exactly.
        bleed: Sides::splat(Abs::zero()),
        fill: Smart::Auto, // -> white background for raster
        numbering: None,
        supplement: Content::empty(),
        number: 1,
    };
    // `render` now takes `&RenderOptions` (an `f32` scale was removed in 0.15);
    // set the pixels-per-point scale on a defaulted options value.
    let opts = RenderOptions {
        pixel_per_pt: Scalar::new(pixel_per_pt),
        ..RenderOptions::default()
    };
    let pixmap = typst_render::render(&page, &opts);
    let bytes = pixmap.encode_png().ok()?;
    let svg = Some(frame_svg(&page));
    Some(ImageData {
        bytes,
        format: ImageFormat::Png,
        svg,
        alt: None,
        width_emu: (width_pt * 12700.0) as u64,
        height_emu: (height_pt * 12700.0) as u64,
        src_rect: None,
    })
}

/// Serialize a drawing frame to SVG — the vector-preserving companion to the
/// PNG rasterization.
fn frame_svg(page: &typst_layout::Page) -> Vec<u8> {
    let opts = typst_svg::SvgOptions::default();
    typst_svg::svg(page, &opts).into_bytes()
}
