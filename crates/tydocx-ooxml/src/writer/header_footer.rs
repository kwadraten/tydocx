use std::cell::Cell;
use std::io::{self, Write};

use quick_xml::Writer;

use super::fields::{write_complex_field, write_field, write_field_page};
use super::image::write_anchored_float;
use super::run::write_run;
use super::shape::write_inline_shape;
use super::{DocParts, WriteCtx};
use crate::document::{DocumentStyle, HeaderFooter, InlineElement, Paragraph};

/// A `WriteCtx` scoped to one header/footer part: the image list is the
/// part's own (rIds restart at `rId1`, matching the part's `.rels` file) and
/// `parts` is empty so `image_rel_id` yields the part-local `rId{i}`.
/// `shape_id_counter` stays shared so `wp:docPr` ids are unique package-wide.
pub(super) struct PartCtx<'a> {
    pub(super) parts: DocParts,
    pub(super) images: Vec<&'a crate::document::ImageData>,
    pub(super) image_counter: Cell<usize>,
}

impl<'a> PartCtx<'a> {
    pub(super) fn new(images: Vec<&'a crate::document::ImageData>) -> Self {
        Self {
            parts: DocParts::empty(),
            images,
            image_counter: Cell::new(0),
        }
    }

    pub(super) fn ctx<'b>(&'b self, shared: &'b WriteCtx<'a>) -> WriteCtx<'b>
    where
        'a: 'b,
    {
        WriteCtx {
            doc_style: shared.doc_style,
            parts: &self.parts,
            images: &self.images,
            content_width_twips: shared.content_width_twips,
            image_counter: &self.image_counter,
            citation_id_counter: shared.citation_id_counter,
            shape_id_counter: shared.shape_id_counter,
            header_refs: &[],
        }
    }
}

pub(super) fn generate_header_xml(
    writer: &mut Writer<&mut Vec<u8>>,
    content: &HeaderFooter,
    doc_style: &DocumentStyle,
    ctx: &WriteCtx,
) -> io::Result<()> {
    generate_header_footer_part(writer, "w:hdr", content, doc_style, ctx)
}

pub(super) fn generate_footer_xml(
    writer: &mut Writer<&mut Vec<u8>>,
    content: &HeaderFooter,
    doc_style: &DocumentStyle,
    ctx: &WriteCtx,
) -> io::Result<()> {
    generate_header_footer_part(writer, "w:ftr", content, doc_style, ctx)
}

/// Generate a footer XML part containing a PAGE field code for automatic page numbering.
///
/// Produces a centered paragraph with `fldChar begin / instrText PAGE / fldChar separate /
/// fallback text / fldChar end`.
pub(super) fn generate_page_number_footer_xml(writer: &mut Writer<&mut Vec<u8>>) -> io::Result<()> {
    writer
        .create_element("w:ftr")
        .with_attribute((
            "xmlns:w",
            "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
        ))
        .with_attribute((
            "xmlns:r",
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
        ))
        .write_inner_content(|w| {
            w.create_element("w:p").write_inner_content(|pw| {
                // Center-aligned paragraph
                pw.create_element("w:pPr").write_inner_content(|ppr| {
                    ppr.create_element("w:jc")
                        .with_attribute(("w:val", "center"))
                        .write_empty()?;
                    Ok(())
                })?;
                write_complex_field(pw, " PAGE ", "1", false)?;
                Ok(())
            })?;
            Ok(())
        })?;
    Ok(())
}

fn generate_header_footer_part(
    writer: &mut Writer<&mut Vec<u8>>,
    root_tag: &str,
    content: &HeaderFooter,
    doc_style: &DocumentStyle,
    ctx: &WriteCtx,
) -> io::Result<()> {
    let has_anchors = content.paragraphs.iter().any(|para| {
        para.inlines
            .iter()
            .any(|i| matches!(i, InlineElement::AnchoredFloat(_) | InlineElement::Shape(_)))
    });
    let root = writer
        .create_element(root_tag)
        .with_attribute((
            "xmlns:w",
            "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
        ))
        .with_attribute((
            "xmlns:r",
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
        ));
    // Furniture anchors (`wp:anchor`) need the drawing prefix declared;
    // `a:`/`pic:`/`wps:`/`mc:` are declared inline by the drawing writers.
    let root = if has_anchors {
        root.with_attribute((
            "xmlns:wp",
            "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing",
        ))
    } else {
        root
    };
    root.write_inner_content(|w| {
        for para in &content.paragraphs {
            write_header_footer_paragraph(w, para, doc_style, ctx)?;
        }
        Ok(())
    })?;
    Ok(())
}

/// Write a paragraph for headers/footers: text runs, fields, and furniture
/// `wp:anchor` drawings. `FieldPage` (margin-zone page-number tokens) and
/// `Field` (C3 running-heading `STYLEREF`s) reach here; any other non-text
/// inline is still dropped.
fn write_header_footer_paragraph<W: Write>(
    writer: &mut Writer<W>,
    para: &Paragraph,
    doc_style: &DocumentStyle,
    ctx: &WriteCtx,
) -> io::Result<()> {
    writer.create_element("w:p").write_inner_content(|w| {
        let has_alignment = para.alignment.is_some();
        let has_mark_size = para.mark_size_half_pt.is_some();
        if has_alignment || has_mark_size {
            w.create_element("w:pPr").write_inner_content(|ppr| {
                if let Some(size) = para.mark_size_half_pt {
                    ppr.create_element("w:rPr").write_inner_content(|rpr| {
                        rpr.create_element("w:sz")
                            .with_attribute(("w:val", size.to_string().as_str()))
                            .write_empty()?;
                        Ok(())
                    })?;
                }
                if let Some(alignment) = &para.alignment {
                    ppr.create_element("w:jc")
                        .with_attribute(("w:val", alignment.ooxml_value()))
                        .write_empty()?;
                }
                Ok(())
            })?;
        }
        for inline in &para.inlines {
            match inline {
                InlineElement::Text(run) => write_run(w, run, doc_style)?,
                InlineElement::FieldPage {
                    display_text,
                    bold,
                    size_half_pt,
                } => {
                    write_field_page(w, display_text, *bold, *size_half_pt)?;
                }
                InlineElement::Field {
                    instruction,
                    result,
                } => {
                    write_field(w, instruction, result, doc_style)?;
                }
                InlineElement::AnchoredFloat(float) => {
                    let n = ctx.shape_id_counter.get() + 1;
                    ctx.shape_id_counter.set(n);
                    write_anchored_float(w, float, n, ctx)?;
                }
                InlineElement::Shape(shape) => {
                    let n = ctx.shape_id_counter.get() + 1;
                    ctx.shape_id_counter.set(n);
                    write_inline_shape(w, shape, n, ctx)?;
                }
                _ => {}
            }
        }
        Ok(())
    })?;
    Ok(())
}
