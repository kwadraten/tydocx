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

use super::collect::location_hash64;
use super::inline::FootnoteMarker;
use super::style::{contains_cjk, lower_runs_match};
use crate::convert::image::{
    ImagePlacement, collect_image_sizes, extract_figure_rasters, hash_bytes,
};
use crate::convert::page::{effective_style, effective_weight, extract_non_black_color};

pub fn patch_document(
    doc: &mut IrDocument,
    markers: &[FootnoteMarker],
    collected_bodies: &HashMap<Location, Vec<IrParagraph>>,
    paged: &PagedDocument,
    style: &DocumentStyle,
) -> Vec<Degradation> {
    let mut degradations = Vec::new();

    let entries = collect_footnote_entries(paged, style);
    let ordered = &entries.in_order;
    let mut used: Vec<bool> = vec![false; ordered.len()];
    let mut footnotes = Vec::new();
    for (index, marker) in markers.iter().enumerate() {
        let (location, rendered) = (&marker.location, &marker.rendered);
        let ordinal = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let body = match collected_bodies.get(location) {
            Some(body) => {
                if let Some(pos) = ordered.iter().position(|(loc, _)| loc == location) {
                    used[pos] = true;
                }
                Some(body.clone())
            }
            None => match entries.by_location.get(location) {
                Some(entry) => {
                    if let Some(pos) = ordered.iter().position(|(loc, _)| loc == location) {
                        used[pos] = true;
                    }
                    degradations.extend(entry.skipped.iter().cloned());
                    Some(entry.paragraphs.clone())
                }
                None => {
                    if index < ordered.len() && !used[index] {
                        used[index] = true;
                        let entry = &ordered[index].1;
                        degradations.extend(entry.skipped.iter().cloned());
                        Some(entry.paragraphs.clone())
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
                label: marker.label.clone(),
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

    let sizes = collect_image_sizes(paged);
    patch_block_images(&mut doc.body, &sizes);
    for footnote in &mut doc.footnotes {
        for para in &mut footnote.body {
            patch_inline_images(&mut para.inlines, &sizes);
        }
    }

    let mut canvases: HashMap<u64, typort_ooxml::document::ImageData> =
        extract_figure_rasters(paged)
            .into_iter()
            .map(|(loc, data)| (location_hash64(loc), data))
            .collect();
    attach_figure_canvases(&mut doc.body, &mut canvases);

    degradations
}

struct EntryBody {
    paragraphs: Vec<IrParagraph>,
    /// Images the frame walk could not materialize; reported only when this
    /// frame body is actually used (a collected pair body embeds them).
    skipped: Vec<Degradation>,
}

struct FootnoteEntries {
    by_location: HashMap<Location, EntryBody>,
    in_order: Vec<(Location, EntryBody)>,
}

fn collect_footnote_entries(paged: &PagedDocument, style: &DocumentStyle) -> FootnoteEntries {
    let mut entries = FootnoteEntries {
        by_location: HashMap::new(),
        in_order: Vec::new(),
    };
    for page in paged.pages() {
        collect_entries_from_frame(&page.frame, style, &mut entries);
    }
    entries
}

/// Outline entries share the `"entry"` tag but have no parent, so they
/// never match here.
fn collect_entries_from_frame(frame: &Frame, style: &DocumentStyle, entries: &mut FootnoteEntries) {
    for (_, item) in frame.items() {
        if let FrameItem::Group(group) = item {
            if crate::convert::frames::is_footnote_entry_group(group)
                && let Some(parent) = group.parent
            {
                let mut walk = EntryWalk {
                    style,
                    tag_stack: Vec::new(),
                    paragraph: IrParagraph::default(),
                    skipped: Vec::new(),
                };
                walk_entry_frame(&group.frame, &mut walk);
                let mut body = vec![walk.paragraph];
                body.retain(|para| !para.inlines.is_empty());
                let entry = EntryBody {
                    paragraphs: body,
                    skipped: walk.skipped,
                };
                if !entries.by_location.contains_key(&parent.location) {
                    entries.in_order.push((
                        parent.location,
                        EntryBody {
                            paragraphs: entry.paragraphs.clone(),
                            skipped: entry.skipped.clone(),
                        },
                    ));
                }
                entries.by_location.insert(parent.location, entry);
            }
            collect_entries_from_frame(&group.frame, style, entries);
        }
    }
}

struct EntryWalk<'a> {
    style: &'a DocumentStyle,
    tag_stack: Vec<(&'static str, Location)>,
    paragraph: IrParagraph,
    skipped: Vec<Degradation>,
}

fn walk_entry_frame(frame: &Frame, walk: &mut EntryWalk<'_>) {
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
                walk_entry_frame(&group.frame, walk);
            }
            FrameItem::Image(image, size, _) => {
                if !in_entry(walk) {
                    continue;
                }
                push_frame_image(image, size, walk);
            }
            FrameItem::Shape(..) | FrameItem::Link(..) => {}
        }
    }
}

fn in_entry(walk: &EntryWalk<'_>) -> bool {
    walk.tag_stack.iter().any(|(name, _)| *name == "entry")
}

fn footnote_run(text: &TextItem, walk: &EntryWalk<'_>) -> Run {
    let mut run = Run::new(text.text.to_string());
    let info = text.font.info();
    run.bold = effective_weight(&text.font) >= 600
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

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "positive pt sizes rounded to whole EMU"
)]
fn push_frame_image(
    image: &typst_library::visualize::Image,
    size: &typst_library::layout::Size,
    walk: &mut EntryWalk<'_>,
) {
    let ImageKind::Raster(raster) = image.kind() else {
        walk.skipped.push(Degradation {
            span: None,
            construct: "footnote-svg-image",
            action: DegradationAction::Skipped,
            message:
                "an SVG/PDF image inside a footnote has no raster form in the paged frame; skipped"
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
        walk.skipped.push(Degradation {
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
        svg: None,
        src_rect: None,
    }));
}

fn push_merged(paragraph: &mut IrParagraph, run: Run) {
    if let Some(IrInline::Text(prev)) = paragraph.inlines.last_mut()
        && lower_runs_match(prev, &run)
    {
        prev.text.push_str(&run.text);
        return;
    }
    paragraph.inlines.push(IrInline::Text(run));
}

fn patch_block_images(blocks: &mut [IrBlock], sizes: &HashMap<u64, ImagePlacement>) {
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

fn attach_figure_canvases(
    blocks: &mut [IrBlock],
    canvases: &mut HashMap<u64, typort_ooxml::document::ImageData>,
) {
    for block in blocks {
        match block {
            IrBlock::Figure {
                body,
                location_hash,
                ..
            } => {
                if let Some(data) = location_hash.and_then(|hash| canvases.remove(&hash))
                    && !blocks_have_image(body)
                {
                    body.insert(
                        0,
                        IrBlock::Paragraph(IrParagraph {
                            inlines: vec![IrInline::Image(IrImage {
                                source_hash: hash_bytes(&data.bytes),
                                bytes: data.bytes,
                                width_emu: data.width_emu,
                                height_emu: data.height_emu,
                                mime: Some("image/png".to_string()),
                                span: None,
                                vector: true,
                                non_cover_fit: false,
                                alt: None,
                                svg: data.svg,
                                src_rect: None,
                            })],
                            ..IrParagraph::default()
                        }),
                    );
                }
                attach_figure_canvases(body, canvases);
            }
            IrBlock::Table(table) => {
                for row in &mut table.rows {
                    for cell in &mut row.cells {
                        attach_figure_canvases(&mut cell.body, canvases);
                    }
                }
            }
            IrBlock::Grid(grid) => {
                for row in &mut grid.rows {
                    for cell in &mut row.cells {
                        attach_figure_canvases(&mut cell.body, canvases);
                    }
                }
            }
            IrBlock::List(list) => {
                for item in &mut list.items {
                    attach_figure_canvases(&mut item.body, canvases);
                }
            }
            IrBlock::Enum(enumer) => {
                for item in &mut enumer.items {
                    attach_figure_canvases(&mut item.body, canvases);
                }
            }
            IrBlock::Quote { body } | IrBlock::Place { body, .. } | IrBlock::Shape { body, .. } => {
                attach_figure_canvases(body, canvases);
            }
            _ => {}
        }
    }
}

fn blocks_have_image(blocks: &[IrBlock]) -> bool {
    blocks.iter().any(|block| match block {
        IrBlock::Paragraph(para) => para
            .inlines
            .iter()
            .any(|inline| matches!(inline, IrInline::Image(_))),
        _ => false,
    })
}

fn patch_inline_images(inlines: &mut [IrInline], sizes: &HashMap<u64, ImagePlacement>) {
    for inline in inlines {
        match inline {
            IrInline::Image(image) => {
                if let Some(placement) = sizes
                    .get(&image.source_hash)
                    .or_else(|| sizes.get(&hash_bytes(&image.bytes)))
                {
                    image.width_emu = placement.size_emu.0;
                    image.height_emu = placement.size_emu.1;
                    image.src_rect = placement.src_rect;
                } else if let Some((width, height)) = intrinsic_size(&image.bytes) {
                    image.width_emu = width;
                    image.height_emu = height;
                }
            }
            IrInline::Link { body, .. }
            | IrInline::Shape { body, .. }
            | IrInline::Field { body, .. } => {
                patch_inline_images(body, sizes);
            }
            _ => {}
        }
    }
}

fn intrinsic_size(bytes: &[u8]) -> Option<(u64, u64)> {
    let dims = ::image::load_from_memory(bytes).ok()?;
    let px_to_emu = |px: u32| u64::from(px) * 9525;
    Some((px_to_emu(dims.width()), px_to_emu(dims.height())))
}

fn is_png(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes[..4] == [0x89, 0x50, 0x4E, 0x47]
}

fn is_jpeg(bytes: &[u8]) -> bool {
    bytes.len() >= 3 && bytes[..3] == [0xFF, 0xD8, 0xFF]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intrinsic_size_uses_96_dpi() {
        // Minimal 1x1 transparent PNG.
        let png: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        assert_eq!(intrinsic_size(png), Some((9525, 9525)));
        assert_eq!(intrinsic_size(b"not an image"), None);
    }

    #[test]
    fn magic_bytes_detect_png_and_jpeg() {
        assert!(is_png(&[0x89, 0x50, 0x4E, 0x47, 0x00]));
        assert!(!is_png(&[0xFF, 0xD8, 0xFF, 0xE0]));
        assert!(is_jpeg(&[0xFF, 0xD8, 0xFF, 0xE0]));
        assert!(!is_jpeg(&[0x89, 0x50, 0x4E, 0x47]));
    }
}
