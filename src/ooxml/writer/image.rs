use std::io::{self, Write};

use quick_xml::Writer;
use quick_xml::events::BytesText;

use super::citation::write_bibliography_sdt;
use super::fields::write_toc_block;
use super::paragraph::write_paragraph;
use super::table::write_table;
use super::{DocParts, WriteCtx};
use crate::ooxml::document::{
    AnchorWrap, AnchoredContent, AnchoredFloat, AnchoredLine, BlockElement, CellContent, Document,
    ImageData, InlineElement, Paragraph, SrcRect, Table,
};

/// Collect all images from the document model in order of appearance.
/// Returns a `Vec<&ImageData>` where the index corresponds to the 1-based image number.
/// Body images come first, footnote images after them, header/footer-part
/// images last — the footnote and header/footer writers rely on this order
/// for the document-wide media numbering.
pub(super) fn collect_images(doc: &Document) -> Vec<&ImageData> {
    let mut images = Vec::new();
    collect_images_from_blocks(&doc.body.elements, &mut images);
    images.extend(collect_footnote_images(doc));
    for part in collect_header_footer_images(doc) {
        images.extend(part);
    }
    images
}

/// Images inside each header/footer part, in part order
/// (header default/even/first, then footer default/even/first). Each entry is
/// that part's image list in encounter order; the media numbering continues
/// the document-wide sequence while the relationships live in the part's own
/// `word/_rels/headerN.xml.rels` / `word/_rels/footerN.xml.rels`.
pub(super) fn collect_header_footer_images(doc: &Document) -> Vec<Vec<&ImageData>> {
    let mut parts = Vec::new();
    for hf in [
        &doc.header,
        &doc.header_even,
        &doc.header_first,
        &doc.footer,
        &doc.footer_even,
        &doc.footer_first,
    ] {
        let mut images = Vec::new();
        if let Some(hf) = hf {
            for para in &hf.paragraphs {
                collect_images_from_para(para, &mut images);
            }
        }
        parts.push(images);
    }
    parts
}

fn collect_images_from_blocks<'a>(blocks: &'a [BlockElement], images: &mut Vec<&'a ImageData>) {
    for el in blocks {
        match el {
            BlockElement::Paragraph(p) => collect_images_from_para(p, images),
            BlockElement::Table(t) => collect_images_from_table(t, images),
            BlockElement::BibliographyBlock { paragraphs }
            | BlockElement::TocBlock { paragraphs, .. } => {
                for p in paragraphs {
                    collect_images_from_para(p, images);
                }
            }
        }
    }
}

/// Collect the images embedded in footnotes, in document order. They are
/// appended after the body images in [`collect_images`], so their media file
/// numbers continue the document-wide numbering while their relationships
/// live in `word/_rels/footnotes.xml.rels`.
pub(super) fn collect_footnote_images(doc: &Document) -> Vec<&ImageData> {
    let mut images = Vec::new();
    for footnote in &doc.footnotes {
        for para in &footnote.paragraphs {
            collect_images_from_inlines(&para.inlines, &mut images);
        }
    }
    images
}

fn collect_images_from_para<'a>(para: &'a Paragraph, images: &mut Vec<&'a ImageData>) {
    collect_images_from_inlines(&para.inlines, images);
}

/// Images in a run-level inline list — including inside hyperlink bodies,
/// whose media numbering and relationships must resolve like top-level ones.
fn collect_images_from_inlines<'a>(inlines: &'a [InlineElement], images: &mut Vec<&'a ImageData>) {
    for inline in inlines {
        match inline {
            InlineElement::Image(img) => images.push(img),
            InlineElement::AnchoredFloat(float) => match &float.content {
                AnchoredContent::Image(img) => images.push(img),
                AnchoredContent::Text { blocks, .. } => {
                    collect_images_from_blocks(blocks, images);
                }
            },
            InlineElement::Shape(shape) => {
                collect_images_from_blocks(&shape.blocks, images);
            }
            InlineElement::Hyperlink { body, .. } | InlineElement::InternalLink { body, .. } => {
                collect_images_from_inlines(body, images);
            }
            _ => {}
        }
    }
}

fn collect_images_from_table<'a>(table: &'a Table, images: &mut Vec<&'a ImageData>) {
    for row in &table.rows {
        for cell in &row.cells {
            // Check structured content first (includes nested tables)
            if cell.content.is_empty() {
                for para in &cell.paragraphs {
                    collect_images_from_para(para, images);
                }
            } else {
                for item in &cell.content {
                    match item {
                        CellContent::Paragraph(p) => {
                            collect_images_from_para(p, images);
                        }
                        CellContent::Table(t) => {
                            collect_images_from_table(t, images);
                        }
                    }
                }
            }
        }
    }
}

/// Compute the relationship ID for an image's raster (fallback) part given
/// its 1-based index. Image relationships follow all fixed parts in
/// `document.xml.rels`; an image with `svg` set consumes two consecutive
/// relationships — raster first, SVG second.
fn raster_rel_number(image_index: usize, images: &[&ImageData], parts: &DocParts) -> usize {
    let extras = images
        .iter()
        .take(image_index.saturating_sub(1))
        .filter(|img| img.svg.is_some())
        .count();
    parts.relationships.len() + image_index + extras
}

/// Relationship ID for the image's PNG/JPEG part.
pub(super) fn image_rel_id(image_index: usize, images: &[&ImageData], parts: &DocParts) -> String {
    format!("rId{}", raster_rel_number(image_index, images, parts))
}

/// Relationship ID for the image's SVG part, when it has one.
pub(super) fn svg_rel_id(image_index: usize, images: &[&ImageData], parts: &DocParts) -> String {
    format!("rId{}", raster_rel_number(image_index, images, parts) + 1)
}

fn inline_is_drawing(inline: &InlineElement) -> bool {
    match inline {
        InlineElement::Image(_)
        | InlineElement::AnchoredLine(_)
        | InlineElement::AnchoredFloat(_)
        | InlineElement::Shape(_) => true,
        InlineElement::Hyperlink { body, .. } | InlineElement::InternalLink { body, .. } => {
            body.iter().any(inline_is_drawing)
        }
        _ => false,
    }
}

pub(super) fn para_has_drawing(para: &Paragraph) -> bool {
    para.inlines.iter().any(inline_is_drawing)
}

fn table_has_drawing(table: &Table) -> bool {
    table.rows.iter().any(|row| {
        row.cells.iter().any(|cell| {
            if cell.content.is_empty() {
                cell.paragraphs.iter().any(para_has_drawing)
            } else {
                cell.content.iter().any(|item| match item {
                    CellContent::Paragraph(p) => para_has_drawing(p),
                    CellContent::Table(t) => table_has_drawing(t),
                })
            }
        })
    })
}

/// Whether the document body holds any drawing (image or anchored line).
/// Gates the wp/a/pic namespace declarations on `w:document`.
pub(super) fn body_has_drawing(doc: &Document) -> bool {
    doc.body.elements.iter().any(|el| match el {
        BlockElement::Paragraph(p) => para_has_drawing(p),
        BlockElement::Table(t) => table_has_drawing(t),
        BlockElement::BibliographyBlock { paragraphs }
        | BlockElement::TocBlock { paragraphs, .. } => paragraphs.iter().any(para_has_drawing),
    })
}

/// Whether any footnote holds a drawing; gates the same namespaces on
/// `w:footnotes`.
pub(super) fn footnotes_have_drawing(doc: &Document) -> bool {
    doc.footnotes.iter().any(|f| {
        f.paragraphs
            .iter()
            .any(|p| p.inlines.iter().any(inline_is_drawing))
    })
}

/// Write a floating horizontal line as a `wp:anchor` + `wps:wsp` shape: an
/// absolute page-x, line-relative y position, and a `line` preset geometry.
/// This is the overline trick — the shape is anchored to a run on the text
/// line it decorates, so it follows that line's vertical position.
pub(super) fn write_anchored_line<W: Write>(
    writer: &mut Writer<W>,
    line: &AnchoredLine,
    n: usize,
) -> io::Result<()> {
    writer.create_element("w:r").write_inner_content(|w| {
        w.create_element("w:drawing").write_inner_content(|dw| {
            dw.create_element("wp:anchor")
                .with_attribute(("distT", "0"))
                .with_attribute(("distB", "0"))
                .with_attribute(("distL", "0"))
                .with_attribute(("distR", "0"))
                .with_attribute(("simplePos", "0"))
                .with_attribute(("relativeHeight", "0"))
                .with_attribute(("behindDoc", "0"))
                .with_attribute(("locked", "0"))
                .with_attribute(("layoutInCell", "1"))
                .with_attribute(("allowOverlap", "1"))
                .write_inner_content(|an| write_anchor_geometry(an, line, n))?;
            Ok(())
        })?;
        Ok(())
    })?;
    Ok(())
}

fn write_anchor_geometry<W: Write>(
    an: &mut quick_xml::Writer<W>,
    line: &AnchoredLine,
    n: usize,
) -> io::Result<()> {
    let x = line.x_emu.to_string();
    let y = line.y_emu.to_string();
    let cx = line.length_emu.to_string();
    let id_str = n.to_string();
    let name = format!("Line{n}");
    an.create_element("wp:simplePos")
        .with_attribute(("x", "0"))
        .with_attribute(("y", "0"))
        .write_empty()?;
    an.create_element("wp:positionH")
        .with_attribute(("relativeFrom", "page"))
        .write_inner_content(|ph| {
            ph.create_element("wp:posOffset")
                .write_text_content(BytesText::new(&x))?;
            Ok(())
        })?;
    an.create_element("wp:positionV")
        .with_attribute(("relativeFrom", "line"))
        .write_inner_content(|pv| {
            pv.create_element("wp:posOffset")
                .write_text_content(BytesText::new(&y))?;
            Ok(())
        })?;
    an.create_element("wp:extent")
        .with_attribute(("cx", cx.as_str()))
        .with_attribute(("cy", "0"))
        .write_empty()?;
    an.create_element("wp:effectExtent")
        .with_attribute(("l", "0"))
        .with_attribute(("t", "0"))
        .with_attribute(("r", "0"))
        .with_attribute(("b", "0"))
        .write_empty()?;
    an.create_element("wp:wrapNone").write_empty()?;
    an.create_element("wp:docPr")
        .with_attribute(("id", id_str.as_str()))
        .with_attribute(("name", name.as_str()))
        .write_empty()?;
    an.create_element("wp:cNvGraphicFramePr").write_empty()?;
    an.create_element("a:graphic")
        .with_attribute((
            "xmlns:a",
            "http://schemas.openxmlformats.org/drawingml/2006/main",
        ))
        .write_inner_content(|gr| {
            gr.create_element("a:graphicData")
                .with_attribute((
                    "uri",
                    "http://schemas.microsoft.com/office/word/2010/wordprocessingShape",
                ))
                .write_inner_content(|gd| write_line_shape(gd, line))?;
            Ok(())
        })?;
    Ok(())
}

fn write_line_shape<W: Write>(
    gd: &mut quick_xml::Writer<W>,
    line: &AnchoredLine,
) -> io::Result<()> {
    let cx = line.length_emu.to_string();
    let thick = line.thickness_emu.to_string();
    let color = line.color.as_deref().unwrap_or("000000");
    gd.create_element("wps:wsp")
        .with_attribute((
            "xmlns:wps",
            "http://schemas.microsoft.com/office/word/2010/wordprocessingShape",
        ))
        .write_inner_content(|wsp| {
            wsp.create_element("wps:cNvSpPr").write_empty()?;
            wsp.create_element("wps:spPr").write_inner_content(|sp| {
                sp.create_element("a:xfrm").write_inner_content(|xf| {
                    xf.create_element("a:off")
                        .with_attribute(("x", "0"))
                        .with_attribute(("y", "0"))
                        .write_empty()?;
                    xf.create_element("a:ext")
                        .with_attribute(("cx", cx.as_str()))
                        .with_attribute(("cy", "0"))
                        .write_empty()?;
                    Ok(())
                })?;
                sp.create_element("a:prstGeom")
                    .with_attribute(("prst", "line"))
                    .write_inner_content(|pg| {
                        pg.create_element("a:avLst").write_empty()?;
                        Ok(())
                    })?;
                sp.create_element("a:ln")
                    .with_attribute(("w", thick.as_str()))
                    .write_inner_content(|ln| {
                        ln.create_element("a:solidFill").write_inner_content(|sf| {
                            sf.create_element("a:srgbClr")
                                .with_attribute(("val", color))
                                .write_empty()?;
                            Ok(())
                        })?;
                        Ok(())
                    })?;
                Ok(())
            })?;
            wsp.create_element("wps:bodyPr").write_empty()?;
            Ok(())
        })?;
    Ok(())
}

/// Relationship ids and the media filename for a floated image, precomputed
/// from the shared image counter so `pic:pic` inside a `wp:anchor` resolves
/// the same `document.xml.rels` entries an inline image would.
struct AnchorImage<'a> {
    rid: &'a str,
    svg_rid: Option<&'a str>,
    filename: String,
}

/// Write a `wp:anchor` drawing for a `#place(.., float: true)`: a
/// page-absolute position, `distT`/`distB` from `place(clearance:)`, and
/// either a `pic:pic` or a `wps:txbx` text box as the floating content.
pub(super) fn write_anchored_float<W: Write>(
    writer: &mut Writer<W>,
    float: &AnchoredFloat,
    n: usize,
    ctx: &WriteCtx,
) -> io::Result<()> {
    match &float.content {
        AnchoredContent::Image(img) => {
            let i = ctx.image_counter.get() + 1;
            ctx.image_counter.set(i);
            let rid = image_rel_id(i, ctx.images, ctx.parts);
            let svg_rid = img
                .svg
                .is_some()
                .then(|| svg_rel_id(i, ctx.images, ctx.parts));
            let filename = format!("image{i}.{}", img.format.extension());
            let image = AnchorImage {
                rid: &rid,
                svg_rid: svg_rid.as_deref(),
                filename,
            };
            write_anchor_run(writer, float, n, Some(&image), ctx)
        }
        AnchoredContent::Text { .. } => write_anchor_run(writer, float, n, None, ctx),
    }
}

fn write_anchor_run<W: Write>(
    writer: &mut Writer<W>,
    float: &AnchoredFloat,
    n: usize,
    image: Option<&AnchorImage<'_>>,
    ctx: &WriteCtx,
) -> io::Result<()> {
    let dist = float.clearance_emu.to_string();
    let behind = if float.behind_doc { "1" } else { "0" };
    writer.create_element("w:r").write_inner_content(|w| {
        w.create_element("w:drawing").write_inner_content(|dw| {
            dw.create_element("wp:anchor")
                .with_attribute(("distT", dist.as_str()))
                .with_attribute(("distB", dist.as_str()))
                .with_attribute(("distL", "0"))
                .with_attribute(("distR", "0"))
                .with_attribute(("simplePos", "0"))
                .with_attribute(("relativeHeight", float.z_order.to_string().as_str()))
                .with_attribute(("behindDoc", behind))
                .with_attribute(("locked", "0"))
                .with_attribute(("layoutInCell", "1"))
                .with_attribute(("allowOverlap", "1"))
                .write_inner_content(|an| write_float_geometry(an, float, n, image, ctx))?;
            Ok(())
        })?;
        Ok(())
    })?;
    Ok(())
}

/// The `wp:anchor` children: page-absolute position, extent, wrap mode, and
/// the `a:graphic` payload (picture or text box).
fn write_float_geometry<W: Write>(
    an: &mut Writer<W>,
    float: &AnchoredFloat,
    n: usize,
    image: Option<&AnchorImage<'_>>,
    ctx: &WriteCtx,
) -> io::Result<()> {
    let x = float.x_emu.to_string();
    let y = float.y_emu.to_string();
    let cx = float.width_emu.to_string();
    let cy = float.height_emu.to_string();
    let id_str = n.to_string();
    let name = format!("Float{n}");
    an.create_element("wp:simplePos")
        .with_attribute(("x", "0"))
        .with_attribute(("y", "0"))
        .write_empty()?;
    an.create_element("wp:positionH")
        .with_attribute(("relativeFrom", float.horz_rel.attr()))
        .write_inner_content(|ph| {
            ph.create_element("wp:posOffset")
                .write_text_content(BytesText::new(&x))?;
            Ok(())
        })?;
    an.create_element("wp:positionV")
        .with_attribute(("relativeFrom", "page"))
        .write_inner_content(|pv| {
            pv.create_element("wp:posOffset")
                .write_text_content(BytesText::new(&y))?;
            Ok(())
        })?;
    an.create_element("wp:extent")
        .with_attribute(("cx", cx.as_str()))
        .with_attribute(("cy", cy.as_str()))
        .write_empty()?;
    an.create_element("wp:effectExtent")
        .with_attribute(("l", "0"))
        .with_attribute(("t", "0"))
        .with_attribute(("r", "0"))
        .with_attribute(("b", "0"))
        .write_empty()?;
    match float.wrap {
        AnchorWrap::TopAndBottom => {
            an.create_element("wp:wrapTopAndBottom").write_empty()?;
        }
        AnchorWrap::Square => {
            an.create_element("wp:wrapSquare")
                .with_attribute(("wrapText", "bothSides"))
                .write_empty()?;
        }
        AnchorWrap::None => {
            an.create_element("wp:wrapNone").write_empty()?;
        }
    }
    an.create_element("wp:docPr")
        .with_attribute(("id", id_str.as_str()))
        .with_attribute(("name", name.as_str()))
        .write_empty()?;
    an.create_element("wp:cNvGraphicFramePr").write_empty()?;
    an.create_element("a:graphic")
        .with_attribute((
            "xmlns:a",
            "http://schemas.openxmlformats.org/drawingml/2006/main",
        ))
        .write_inner_content(|gr| {
            gr.create_element("a:graphicData")
                .with_attribute(("uri", graphic_data_uri(&float.content)))
                .write_inner_content(|gd| match &float.content {
                    AnchoredContent::Image(img) => {
                        let Some(image) = image else { return Ok(()) };
                        gd.create_element("pic:pic")
                            .with_attribute((
                                "xmlns:pic",
                                "http://schemas.openxmlformats.org/drawingml/2006/picture",
                            ))
                            .write_inner_content(|pic| {
                                write_pic_body(
                                    pic,
                                    &id_str,
                                    &image.filename,
                                    &cx,
                                    &cy,
                                    image.rid,
                                    image.svg_rid,
                                    img.src_rect,
                                )
                            })?;
                        Ok(())
                    }
                    AnchoredContent::Text { blocks, fill_hex } => write_float_textbox(
                        gd,
                        blocks,
                        fill_hex.as_deref(),
                        &cx,
                        &cy,
                        float.wrap == AnchorWrap::None,
                        ctx,
                    ),
                })?;
            Ok(())
        })?;
    Ok(())
}

fn graphic_data_uri(content: &AnchoredContent) -> &'static str {
    match content {
        AnchoredContent::Image(_) => "http://schemas.openxmlformats.org/drawingml/2006/picture",
        AnchoredContent::Text { .. } => {
            "http://schemas.microsoft.com/office/word/2010/wordprocessingShape"
        }
    }
}

/// A `wps:wsp` text box (`txBox="1"` + `wps:txbx` + `w:txbxContent`) holding
/// real block content, so a text float stays editable rather than raster. A
/// `fill_hex` fills the whole shape — a placed `rect(fill:)` draws as a solid
/// box, not as shading on individual text lines.
fn write_float_textbox<W: Write>(
    gd: &mut Writer<W>,
    blocks: &[BlockElement],
    fill_hex: Option<&str>,
    cx: &str,
    cy: &str,
    no_wrap: bool,
    ctx: &WriteCtx,
) -> io::Result<()> {
    gd.create_element("wps:wsp")
        .with_attribute((
            "xmlns:wps",
            "http://schemas.microsoft.com/office/word/2010/wordprocessingShape",
        ))
        .write_inner_content(|wsp| {
            wsp.create_element("wps:cNvSpPr")
                .with_attribute(("txBox", "1"))
                .write_empty()?;
            wsp.create_element("wps:spPr").write_inner_content(|sp| {
                sp.create_element("a:xfrm").write_inner_content(|xf| {
                    xf.create_element("a:off")
                        .with_attribute(("x", "0"))
                        .with_attribute(("y", "0"))
                        .write_empty()?;
                    xf.create_element("a:ext")
                        .with_attribute(("cx", cx))
                        .with_attribute(("cy", cy))
                        .write_empty()?;
                    Ok(())
                })?;
                sp.create_element("a:prstGeom")
                    .with_attribute(("prst", "rect"))
                    .write_inner_content(|pg| {
                        pg.create_element("a:avLst").write_empty()?;
                        Ok(())
                    })?;
                if let Some(hex) = fill_hex {
                    sp.create_element("a:solidFill").write_inner_content(|sf| {
                        sf.create_element("a:srgbClr")
                            .with_attribute(("val", hex))
                            .write_empty()?;
                        Ok(())
                    })?;
                }
                Ok(())
            })?;
            wsp.create_element("wps:txbx").write_inner_content(|tx| {
                tx.create_element("w:txbxContent")
                    .write_inner_content(|c| {
                        for element in blocks {
                            match element {
                                BlockElement::Paragraph(para) => write_paragraph(c, para, ctx)?,
                                BlockElement::Table(table) => write_table(c, table, ctx)?,
                                BlockElement::BibliographyBlock { paragraphs } => {
                                    write_bibliography_sdt(c, paragraphs, ctx)?;
                                }
                                BlockElement::TocBlock {
                                    max_depth,
                                    caption_id,
                                    paragraphs,
                                } => {
                                    write_toc_block(
                                        c,
                                        *max_depth,
                                        caption_id.as_deref(),
                                        paragraphs,
                                        ctx,
                                    )?;
                                }
                            }
                        }
                        Ok(())
                    })?;
                Ok(())
            })?;
            // Floats get `a:spAutoFit` over a fixed height: Word line heights
            // can exceed the measured frame, and clipping is worse than a
            // taller box. A non-floating `#place` is measured at ink bounds —
            // `wrap="none"` stops Word re-breaking lines the author never
            // wrapped, and `a:noAutofit` keeps the extent a placed `rect`
            // declares (autofit would shrink a fill-only box to nothing).
            let el = wsp
                .create_element("wps:bodyPr")
                .with_attribute(("wrap", if no_wrap { "none" } else { "square" }))
                .with_attribute(("lIns", "0"))
                .with_attribute(("tIns", "0"))
                .with_attribute(("rIns", "0"))
                .with_attribute(("bIns", "0"))
                .with_attribute(("anchor", "t"));
            if no_wrap {
                el.with_attribute(("vertOverflow", "overflow"))
                    .with_attribute(("horzOverflow", "overflow"))
                    .write_inner_content(|bp| {
                        bp.create_element("a:noAutofit").write_empty()?;
                        Ok(())
                    })?;
            } else {
                el.write_inner_content(|bp| {
                    bp.create_element("a:spAutoFit").write_empty()?;
                    Ok(())
                })?;
            }
            Ok(())
        })?;
    Ok(())
}

/// Write a `wp:inline` drawing element for an embedded image.
pub(super) fn write_image_inline<W: Write>(
    writer: &mut Writer<W>,
    img: &ImageData,
    image_index: usize,
    drawing_id: usize,
    rid: &str,
    svg_rid: Option<&str>,
) -> io::Result<()> {
    let cx = img.width_emu.to_string();
    let cy = img.height_emu.to_string();
    let id_str = drawing_id.to_string();
    let ext = img.format.extension();
    let name = format!("Image{image_index}");
    let filename = format!("image{image_index}.{ext}");

    writer.create_element("w:r").write_inner_content(|w| {
        w.create_element("w:drawing").write_inner_content(|dw| {
            dw.create_element("wp:inline")
                .with_attribute(("distT", "0"))
                .with_attribute(("distB", "0"))
                .with_attribute(("distL", "0"))
                .with_attribute(("distR", "0"))
                .write_inner_content(|inl| {
                    inl.create_element("wp:extent")
                        .with_attribute(("cx", cx.as_str()))
                        .with_attribute(("cy", cy.as_str()))
                        .write_empty()?;
                    inl.create_element("wp:docPr")
                        .with_attribute(("id", id_str.as_str()))
                        .with_attribute(("name", name.as_str()))
                        .with_attribute(("descr", img.alt.as_deref().unwrap_or("")))
                        .write_empty()?;
                    inl.create_element("a:graphic")
                        .with_attribute(("xmlns:a", "http://schemas.openxmlformats.org/drawingml/2006/main"))
                        .write_inner_content(|gr| {
                            gr.create_element("a:graphicData")
                                .with_attribute(("uri", "http://schemas.openxmlformats.org/drawingml/2006/picture"))
                                .write_inner_content(|gd| {
                                    gd.create_element("pic:pic")
                                        .with_attribute(("xmlns:pic", "http://schemas.openxmlformats.org/drawingml/2006/picture"))
                                        .write_inner_content(|pic| {
                                            write_pic_body(
                                                pic,
                                                &id_str,
                                                &filename,
                                                &cx,
                                                &cy,
                                                rid,
                                                svg_rid,
                                                img.src_rect,
                                            )
                                        })?;
                                    Ok(())
                                })?;
                            Ok(())
                        })?;
                    Ok(())
                })?;
            Ok(())
        })?;
        Ok(())
    })?;
    Ok(())
}

#[allow(
    clippy::too_many_arguments,
    reason = "the pic:pic parts are all fields of the same drawing"
)]
fn write_pic_body<W: Write>(
    pic: &mut Writer<W>,
    id_str: &str,
    filename: &str,
    cx: &str,
    cy: &str,
    rid: &str,
    svg_rid: Option<&str>,
    src_rect: Option<SrcRect>,
) -> io::Result<()> {
    pic.create_element("pic:nvPicPr")
        .write_inner_content(|nv| {
            nv.create_element("pic:cNvPr")
                .with_attribute(("id", id_str))
                .with_attribute(("name", filename))
                .write_empty()?;
            nv.create_element("pic:cNvPicPr").write_empty()?;
            Ok(())
        })?;
    pic.create_element("pic:blipFill")
        .write_inner_content(|bf| {
            let blip = bf.create_element("a:blip").with_attribute(("r:embed", rid));
            if let Some(svg_rid) = svg_rid {
                blip.write_inner_content(|blip| {
                    blip.create_element("a:extLst").write_inner_content(|lst| {
                        lst.create_element("a:ext")
                            .with_attribute(("uri", "{96DAC541-7B7A-43D3-8B79-37D633B846F1}"))
                            .write_inner_content(|ext_el| {
                                ext_el
                                    .create_element("asvg:svgBlip")
                                    .with_attribute((
                                        "xmlns:asvg",
                                        "http://schemas.microsoft.com/office/drawing/2016/SVG/main",
                                    ))
                                    .with_attribute(("r:embed", svg_rid))
                                    .write_empty()?;
                                Ok(())
                            })?;
                        Ok(())
                    })?;
                    Ok(())
                })?;
            } else {
                blip.write_empty()?;
            }
            if let Some(rect) = src_rect {
                bf.create_element("a:srcRect")
                    .with_attribute(("l", rect.l.to_string().as_str()))
                    .with_attribute(("t", rect.t.to_string().as_str()))
                    .with_attribute(("r", rect.r.to_string().as_str()))
                    .with_attribute(("b", rect.b.to_string().as_str()))
                    .write_empty()?;
            }
            bf.create_element("a:stretch").write_inner_content(|st| {
                st.create_element("a:fillRect").write_empty()?;
                Ok(())
            })?;
            Ok(())
        })?;
    pic.create_element("pic:spPr").write_inner_content(|sp| {
        sp.create_element("a:xfrm").write_inner_content(|xf| {
            xf.create_element("a:off")
                .with_attribute(("x", "0"))
                .with_attribute(("y", "0"))
                .write_empty()?;
            xf.create_element("a:ext")
                .with_attribute(("cx", cx))
                .with_attribute(("cy", cy))
                .write_empty()?;
            Ok(())
        })?;
        sp.create_element("a:prstGeom")
            .with_attribute(("prst", "rect"))
            .write_inner_content(|pg| {
                pg.create_element("a:avLst").write_empty()?;
                Ok(())
            })?;
        Ok(())
    })?;
    Ok(())
}
