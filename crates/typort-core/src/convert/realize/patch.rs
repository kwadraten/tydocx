//! Companion-Paged patch: footnote bodies and image display sizes.
//!
//! The collector reads semantics from the realize pairs; two things only
//! exist in the companion `PagedDocument`:
//!
//! - Footnote entry bodies: the pairs carry the marker, not the entry
//!   text. An entry lives in a frame group whose `parent` is the marker's
//!   `Location` and which contains a `Tag::Start("entry")`; the entry's
//!   marker number (inside `link-marker` tags) is skipped, the rest is
//!   the body text (probe §9, `paged-frames.txt`).
//! - On-page image display sizes, by content hash (the legacy
//!   `collect_image_sizes` map). Re-encoded images (GIF/WebP/SVG) change
//!   bytes, so a hash miss falls back to intrinsic dimensions at 96 DPI,
//!   exactly as the legacy path does.

use std::collections::HashMap;

use typort_ir::{
    Degradation, DegradationAction, IrBlock, IrDocument, IrFootnote, IrImage, IrInline, IrParagraph,
};
use typort_ooxml::document::{DocumentStyle, Run};
use typst_layout::PagedDocument;
use typst_library::introspection::{Location, Tag};
use typst_library::layout::{Frame, FrameItem};
use typst_library::text::{FontStyle, TextItem};
use typst_library::visualize::ImageKind;

use super::style::{contains_cjk, lower_runs_match};
use crate::convert::image::{collect_image_sizes, hash_bytes};
use crate::convert::page::{effective_style, effective_weight, extract_non_black_color};

/// Patch an IR document with companion-Paged facts.
///
/// Footnote bodies are matched to markers by ordinal (marker order =
/// entry order — never by `Location` across the boundary; the locations
/// are only used to find the entry group *within* this one Paged
/// document). Image sizes are filled by content hash, with an intrinsic
/// fallback. Returns the degradations recorded during patching.
/// Patch an IR document with companion-Paged data: footnote entry bodies
/// and image display sizes.
///
/// `collected_bodies` are the footnote bodies collected from content
/// during the walk (keyed by marker location); they are preferred over
/// the Paged-frame bodies because frames carry only rendered glyphs —
/// math becomes literal Mathematical-Alphanumeric text and hyperlink
/// display text is lost there. A marker with no collected body falls
/// back to the frame entries below.
pub fn patch_document(
    doc: &mut IrDocument,
    markers: &[(Location, String)],
    collected_bodies: &HashMap<Location, Vec<IrParagraph>>,
    paged: &PagedDocument,
    style: &DocumentStyle,
) -> Vec<Degradation> {
    let mut degradations = Vec::new();

    // Footnote bodies, keyed by marker location within this Paged document.
    // A marker realized inside a table/figure fragment carries a location
    // from the fragment's split locator, which never matches the Paged
    // compile's location for the same footnote — so a location miss falls
    // back to the ordinal: markers are collected in document order and
    // entries are listed in page order, which agree.
    let entries = collect_footnote_entries(paged, style, &mut degradations);
    let ordered = &entries.in_order;
    let mut used: Vec<bool> = vec![false; ordered.len()];
    let mut footnotes = Vec::new();
    for (index, (location, rendered)) in markers.iter().enumerate() {
        let ordinal = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let body = match collected_bodies.get(location) {
            Some(body) => {
                if let Some(pos) = ordered.iter().position(|(loc, _)| loc == location) {
                    used[pos] = true;
                }
                Some(body.clone())
            }
            None => match entries.by_location.get(location) {
                Some(body) => {
                    if let Some(pos) = ordered.iter().position(|(loc, _)| loc == location) {
                        used[pos] = true;
                    }
                    Some(body.clone())
                }
                None => {
                    // Ordinal fallback: the entry at this marker's position in
                    // document order, when it has not already been claimed by
                    // a location match.
                    if index < ordered.len() && !used[index] {
                        used[index] = true;
                        Some(ordered[index].1.clone())
                    } else {
                        None
                    }
                }
            },
        };
        match body {
            Some(body) => footnotes.push(IrFootnote {
                ordinal,
                rendered: rendered.clone(),
                body,
            }),
            None => degradations.push(Degradation {
                span: None,
                construct: "footnote-entry-not-found",
                action: DegradationAction::Approximated,
                message: format!(
                    "footnote {ordinal} has no entry group in the paged frames; its marker stays literal"
                ),
            }),
        }
    }
    doc.footnotes = footnotes;

    // Image display sizes.
    let sizes = collect_image_sizes(paged);
    patch_block_images(&mut doc.body, &sizes);
    for footnote in &mut doc.footnotes {
        for para in &mut footnote.body {
            patch_inline_images(&mut para.inlines, &sizes);
        }
    }

    degradations
}

/// Footnote entry bodies from the Paged frames: the map (for location
/// matches) and the same entries in page order (for the ordinal fallback
/// in `patch_document`).
struct FootnoteEntries {
    by_location: HashMap<Location, Vec<IrParagraph>>,
    in_order: Vec<(Location, Vec<IrParagraph>)>,
}

/// Collect footnote entry bodies from the Paged frames, keyed by the
/// marker location (the entry group's `parent`) — and, in the same
/// order, as a list for the ordinal fallback in `patch_document`.
fn collect_footnote_entries(
    paged: &PagedDocument,
    style: &DocumentStyle,
    degradations: &mut Vec<Degradation>,
) -> FootnoteEntries {
    let mut entries = FootnoteEntries {
        by_location: HashMap::new(),
        in_order: Vec::new(),
    };
    for page in paged.pages() {
        collect_entries_from_frame(&page.frame, style, degradations, &mut entries);
    }
    entries
}

/// Recurse into frame groups; a group whose `parent` is set and whose
/// frame directly contains a `Tag::Start("entry")` is a footnote entry
/// group. (Outline entries also carry an `"entry"` tag, but their groups
/// have no parent, so they never match here.)
fn collect_entries_from_frame(
    frame: &Frame,
    style: &DocumentStyle,
    degradations: &mut Vec<Degradation>,
    entries: &mut FootnoteEntries,
) {
    for (_, item) in frame.items() {
        if let FrameItem::Group(group) = item {
            if crate::convert::frames::is_footnote_entry_group(group)
                && let Some(parent) = group.parent
            {
                let mut walk = EntryWalk {
                    style,
                    tag_stack: Vec::new(),
                    paragraph: IrParagraph::default(),
                };
                walk_entry_frame(&group.frame, &mut walk, degradations);
                let mut body = vec![walk.paragraph];
                body.retain(|para| !para.inlines.is_empty());
                if entries
                    .by_location
                    .insert(parent.location, body.clone())
                    .is_none()
                {
                    entries.in_order.push((parent.location, body));
                }
            }
            collect_entries_from_frame(&group.frame, style, degradations, entries);
        }
    }
}

/// Walk state for one footnote entry's frame.
struct EntryWalk<'a> {
    style: &'a DocumentStyle,
    /// Open tags, as (element name, location), innermost last.
    tag_stack: Vec<(&'static str, Location)>,
    /// The entry body, as a single paragraph (the lowering concatenates
    /// footnote paragraphs anyway, so line/paragraph splits inside an
    /// entry carry no information).
    paragraph: IrParagraph,
}

/// Walk an entry group's frame, collecting its text (and raster images)
/// into the walk's paragraph.
fn walk_entry_frame(frame: &Frame, walk: &mut EntryWalk<'_>, degradations: &mut Vec<Degradation>) {
    for (_, item) in frame.items() {
        match item {
            FrameItem::Tag(tag) => match tag {
                Tag::Start(elem, _) => {
                    if let Some(location) = elem.location() {
                        walk.tag_stack.push((elem.elem().name(), location));
                    }
                }
                Tag::End(location, _, _) => {
                    if let Some(index) = walk.tag_stack.iter().rposition(|(_, loc)| loc == location)
                    {
                        walk.tag_stack.remove(index);
                    }
                }
            },
            FrameItem::Text(text) => {
                if !in_entry(walk) {
                    continue;
                }
                // The entry's marker number lives inside `link-marker`
                // tags; the body text follows it.
                if walk
                    .tag_stack
                    .iter()
                    .any(|(name, _)| *name == "link-marker")
                {
                    continue;
                }
                if text.text.is_empty() {
                    continue;
                }
                let run = footnote_run(text, walk);
                push_merged(&mut walk.paragraph, run);
            }
            FrameItem::Group(group) => {
                walk_entry_frame(&group.frame, walk, degradations);
            }
            FrameItem::Image(image, size, _) => {
                if !in_entry(walk) {
                    continue;
                }
                push_frame_image(image, size, walk, degradations);
            }
            FrameItem::Shape(..) | FrameItem::Link(..) => {}
        }
    }
}

/// Whether the walk is currently inside the entry tag.
fn in_entry(walk: &EntryWalk<'_>) -> bool {
    walk.tag_stack.iter().any(|(name, _)| *name == "entry")
}

/// One frame text item as a run: weight/style from the shaped font
/// (variable-font aware, as the legacy path reads them), tag context
/// (`strong`/`emph`/`super`/`sub`) OR-ed in, size and font recorded only
/// when they differ from the document body.
fn footnote_run(text: &TextItem, walk: &EntryWalk<'_>) -> Run {
    let mut run = Run::new(text.text.to_string());
    let info = text.font.info();
    run.bold = effective_weight(&text.font) >= 700
        || walk.tag_stack.iter().any(|(name, _)| *name == "strong");
    run.italic = matches!(
        effective_style(&text.font),
        FontStyle::Italic | FontStyle::Oblique
    ) || walk.tag_stack.iter().any(|(name, _)| *name == "emph");
    run.superscript = walk.tag_stack.iter().any(|(name, _)| *name == "super");
    run.subscript = walk.tag_stack.iter().any(|(name, _)| *name == "sub");
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "font sizes are small non-negative values"
    )]
    let size_half_pt = (text.size.to_pt() * 2.0).round().max(1.0) as u32;
    if size_half_pt != walk.style.body_size_half_pt {
        run.size_half_pt = Some(size_half_pt);
    }
    let family = info.family.as_str();
    if contains_cjk(&run.text) {
        if !family.eq_ignore_ascii_case(&walk.style.body_font_east_asia) {
            run.font_east_asia = Some(family.to_string());
        }
    } else if !family.eq_ignore_ascii_case(&walk.style.body_font_ascii) {
        run.font_ascii = Some(family.to_string());
    }
    if let Some(color) = extract_non_black_color(&text.fill) {
        run.color = Some(color);
    }
    run
}

/// A raster image inside a footnote entry: kept with its frame display
/// size. SVG/PDF images have no raster form here and are skipped with a
/// degradation (the main image path rasterizes SVGs; the footnote path
/// does not yet).
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "positive pt sizes rounded to whole EMU"
)]
fn push_frame_image(
    image: &typst_library::visualize::Image,
    size: &typst_library::layout::Size,
    walk: &mut EntryWalk<'_>,
    degradations: &mut Vec<Degradation>,
) {
    let ImageKind::Raster(raster) = image.kind() else {
        degradations.push(Degradation {
            span: None,
            construct: "footnote-svg-image",
            action: DegradationAction::Skipped,
            message: "an SVG/PDF image inside a footnote has no raster form in the patch; skipped"
                .to_string(),
        });
        return;
    };
    let bytes = raster.data().as_slice().to_vec();
    let mime = if is_png(&bytes) {
        Some("image/png".to_string())
    } else if is_jpeg(&bytes) {
        Some("image/jpeg".to_string())
    } else {
        degradations.push(Degradation {
            span: None,
            construct: "footnote-image-format",
            action: DegradationAction::Skipped,
            message: "a footnote image is neither PNG nor JPEG; skipped".to_string(),
        });
        return;
    };
    walk.paragraph.inlines.push(IrInline::Image(IrImage {
        source_hash: hash_bytes(&bytes),
        bytes,
        width_emu: (size.x.to_pt() * 12700.0) as u64,
        height_emu: (size.y.to_pt() * 12700.0) as u64,
        mime,
        span: None,
        vector: false,
        non_cover_fit: false,
        alt: None,
    }));
}

/// Push a run onto a paragraph, merging it into the previous run when the
/// format matches (frame text is split at shaping boundaries the lowering
/// would otherwise have to re-merge).
fn push_merged(paragraph: &mut IrParagraph, run: Run) {
    if let Some(IrInline::Text(prev)) = paragraph.inlines.last_mut()
        && lower_runs_match(prev, &run)
    {
        prev.text.push_str(&run.text);
        return;
    }
    paragraph.inlines.push(IrInline::Text(run));
}

/// Fill image display sizes throughout the body, by content hash, with an
/// intrinsic fallback (see the module docs).
fn patch_block_images(blocks: &mut [IrBlock], sizes: &HashMap<u64, (u64, u64)>) {
    for block in blocks {
        match block {
            IrBlock::Paragraph(para) => patch_inline_images(&mut para.inlines, sizes),
            IrBlock::Heading { body, .. } => patch_inline_images(body, sizes),
            IrBlock::Table(table) => {
                for row in &mut table.rows {
                    for cell in &mut row.cells {
                        patch_block_images(&mut cell.body, sizes);
                    }
                }
            }
            IrBlock::Grid(grid) => {
                for row in &mut grid.rows {
                    for cell in &mut row.cells {
                        patch_block_images(&mut cell.body, sizes);
                    }
                }
            }
            IrBlock::Figure { body, caption, .. } => {
                patch_block_images(body, sizes);
                patch_inline_images(caption, sizes);
            }
            IrBlock::List(list) => {
                for item in &mut list.items {
                    patch_block_images(&mut item.body, sizes);
                }
            }
            IrBlock::Enum(enumer) => {
                for item in &mut enumer.items {
                    patch_block_images(&mut item.body, sizes);
                }
            }
            IrBlock::Quote { body } | IrBlock::Place { body, .. } => {
                patch_block_images(body, sizes);
            }
            IrBlock::Bibliography { entries } => {
                for entry in entries {
                    patch_inline_images(&mut entry.body, sizes);
                }
            }
            IrBlock::Outline { title, entries, .. } => {
                patch_inline_images(title, sizes);
                for entry in entries {
                    patch_inline_images(&mut entry.title, sizes);
                }
            }
            IrBlock::Shape { body, .. } => patch_block_images(body, sizes),
            IrBlock::Equation(_) | IrBlock::Raw(_) | IrBlock::PageBreak { .. } => {}
        }
    }
}

fn patch_inline_images(inlines: &mut [IrInline], sizes: &HashMap<u64, (u64, u64)>) {
    for inline in inlines {
        match inline {
            IrInline::Image(image) => {
                // Look the display size up by the original-bytes hash:
                // re-encoded images (SVG/GIF/WebP) carry rasterized PNG
                // bytes whose hash never appears in the frames. Fall back
                // to hashing the stored bytes for images whose source
                // hash was recorded before this field existed.
                if let Some((width, height)) = sizes
                    .get(&image.source_hash)
                    .or_else(|| sizes.get(&hash_bytes(&image.bytes)))
                {
                    image.width_emu = *width;
                    image.height_emu = *height;
                } else if let Some((width, height)) = intrinsic_size(&image.bytes) {
                    image.width_emu = width;
                    image.height_emu = height;
                }
                // Otherwise the size stays zero: a re-encoded image whose
                // original bytes never appeared in the frames, or an
                // undecodable image. The writer still embeds it; its
                // display size is a known gap (see fallback-registry.md).
            }
            IrInline::Link { body, .. } | IrInline::Shape { body, .. } => {
                patch_inline_images(body, sizes);
            }
            _ => {}
        }
    }
}

/// Intrinsic pixel size -> EMU at 96 DPI, the legacy fallback when the
/// paged size is unavailable.
fn intrinsic_size(bytes: &[u8]) -> Option<(u64, u64)> {
    let dims = ::image::load_from_memory(bytes).ok()?;
    let px_to_emu = |px: u32| u64::from(px) * 9525;
    Some((px_to_emu(dims.width()), px_to_emu(dims.height())))
}

/// Whether `bytes` are a PNG (magic `89 50 4E 47`).
fn is_png(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes[..4] == [0x89, 0x50, 0x4E, 0x47]
}

/// Whether `bytes` are a JPEG (magic `FF D8 FF`).
fn is_jpeg(bytes: &[u8]) -> bool {
    bytes.len() >= 3 && bytes[..3] == [0xFF, 0xD8, 0xFF]
}
