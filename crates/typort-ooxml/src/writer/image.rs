use std::io::{self, Write};

use quick_xml::Writer;
use quick_xml::events::BytesText;

use super::DocParts;
use crate::document::{
    AnchoredLine, BlockElement, CellContent, Document, ImageData, InlineElement, Paragraph, Table,
};

/// Collect all images from the document model in order of appearance.
/// Returns a `Vec<&ImageData>` where the index corresponds to the 1-based image number.
/// Body images come first, footnote images after them — the footnote writer
/// relies on this order for the document-wide media numbering.
pub(super) fn collect_images(doc: &Document) -> Vec<&ImageData> {
    let mut images = Vec::new();
    for el in &doc.body.elements {
        match el {
            BlockElement::Paragraph(p) => collect_images_from_para(p, &mut images),
            BlockElement::Table(t) => {
                collect_images_from_table(t, &mut images);
            }
            BlockElement::BibliographyBlock { paragraphs } => {
                for p in paragraphs {
                    collect_images_from_para(p, &mut images);
                }
            }
        }
    }
    images.extend(collect_footnote_images(doc));
    images
}

/// Collect the images embedded in footnotes, in document order. They are
/// appended after the body images in [`collect_images`], so their media file
/// numbers continue the document-wide numbering while their relationships
/// live in `word/_rels/footnotes.xml.rels`.
pub(super) fn collect_footnote_images(doc: &Document) -> Vec<&ImageData> {
    let mut images = Vec::new();
    for footnote in &doc.footnotes {
        for inline in &footnote.content {
            if let InlineElement::Image(img) = inline {
                images.push(img);
            }
        }
    }
    images
}

fn collect_images_from_para<'a>(para: &'a Paragraph, images: &mut Vec<&'a ImageData>) {
    for inline in &para.inlines {
        if let InlineElement::Image(img) = inline {
            images.push(img);
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

/// Compute the relationship ID for an image given its 1-based index.
/// Image relationships follow all fixed parts in `document.xml.rels`.
pub(super) fn image_rel_id(image_index: usize, parts: &DocParts) -> String {
    format!("rId{}", parts.relationships.len() + image_index)
}

fn inline_is_drawing(inline: &InlineElement) -> bool {
    matches!(
        inline,
        InlineElement::Image(_) | InlineElement::AnchoredLine(_)
    )
}

fn para_has_drawing(para: &Paragraph) -> bool {
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
        BlockElement::BibliographyBlock { paragraphs } => paragraphs.iter().any(para_has_drawing),
    })
}

/// Whether any footnote holds a drawing; gates the same namespaces on
/// `w:footnotes`.
pub(super) fn footnotes_have_drawing(doc: &Document) -> bool {
    doc.footnotes
        .iter()
        .any(|f| f.content.iter().any(inline_is_drawing))
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

/// Write a `wp:inline` drawing element for an embedded image.
pub(super) fn write_image_inline<W: Write>(
    writer: &mut Writer<W>,
    img: &ImageData,
    n: usize,
    rid: &str,
) -> io::Result<()> {
    let cx = img.width_emu.to_string();
    let cy = img.height_emu.to_string();
    let id_str = n.to_string();
    let ext = img.format.extension();
    let name = format!("Image{n}");
    let filename = format!("image{n}.{ext}");

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
                                            pic.create_element("pic:nvPicPr").write_inner_content(|nv| {
                                                nv.create_element("pic:cNvPr")
                                                    .with_attribute(("id", id_str.as_str()))
                                                    .with_attribute(("name", filename.as_str()))
                                                    .write_empty()?;
                                                nv.create_element("pic:cNvPicPr").write_empty()?;
                                                Ok(())
                                            })?;
                                            pic.create_element("pic:blipFill").write_inner_content(|bf| {
                                                bf.create_element("a:blip")
                                                    .with_attribute(("r:embed", rid))
                                                    .write_empty()?;
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
                                                        .with_attribute(("cx", cx.as_str()))
                                                        .with_attribute(("cy", cy.as_str()))
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
