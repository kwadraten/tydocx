use std::cell::Cell;
use std::io::{self, Write};

use quick_xml::Writer;

use super::fields::{write_complex_field, write_field, write_field_page};
use super::footnotes::para_has_math;
use super::hyperlinks::{HyperlinkRelationships, write_hyperlink};
use super::image::{para_has_drawing, write_anchored_float};
use super::package::{Indentation, write_indentation};
use super::run::write_run;
use super::shape::write_inline_shape;
use super::{DocParts, RelKind, WriteCtx};
use crate::document::{DocumentStyle, HeaderFooter, InlineElement, Paragraph, SectionFooter};

pub(super) const EMPTY_FOOTER_SLOT: u8 = 4;

pub(super) fn write_footer_refs<W: Write>(
    w: &mut Writer<W>,
    parts: &DocParts,
    include_first: bool,
    footer: SectionFooter,
) -> io::Result<()> {
    if footer == SectionFooter::Omitted {
        return Ok(());
    }
    for (slot, kind) in [(1, "default"), (2, "even"), (3, "first")] {
        if (slot == 3 && !include_first) || !parts.has(RelKind::Footer(slot)) {
            continue;
        }
        let part = if footer == SectionFooter::Empty {
            EMPTY_FOOTER_SLOT
        } else {
            slot
        };
        if let Some(rid) = parts.rel_id(RelKind::Footer(part)) {
            w.create_element("w:footerReference")
                .with_attribute(("w:type", kind))
                .with_attribute(("r:id", rid.as_str()))
                .write_empty()?;
        }
    }
    Ok(())
}

/// A `WriteCtx` scoped to one header/footer part: the image list is the
/// part's own (rIds restart at `rId1`, matching the part's `.rels` file) and
/// `parts` is empty so `image_rel_id` yields the part-local `rId{i}`.
pub(super) struct PartCtx<'a> {
    pub(super) parts: DocParts,
    pub(super) images: Vec<&'a crate::document::ImageData>,
    pub(super) image_counter: Cell<usize>,
    pub(super) hyperlinks: HyperlinkRelationships,
}

impl<'a> PartCtx<'a> {
    pub(super) fn new(images: Vec<&'a crate::document::ImageData>) -> Self {
        Self {
            parts: DocParts::empty(),
            images,
            image_counter: Cell::new(0),
            hyperlinks: HyperlinkRelationships::default(),
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
            hyperlinks: &self.hyperlinks,
            content_width_twips: shared.content_width_twips,
            image_counter: &self.image_counter,
            citation_id_counter: shared.citation_id_counter,
            drawing_id_counter: shared.drawing_id_counter,
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
    let mut root = writer
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
    if content.paragraphs.iter().any(para_has_drawing) {
        root = root.with_attribute((
            "xmlns:wp",
            "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing",
        ));
    }
    if content.paragraphs.iter().any(para_has_math) {
        root = root.with_attribute((
            "xmlns:m",
            "http://schemas.openxmlformats.org/officeDocument/2006/math",
        ));
    }
    root.write_inner_content(|w| {
        for para in &content.paragraphs {
            write_header_footer_paragraph(w, para, doc_style, ctx)?;
        }
        Ok(())
    })?;
    Ok(())
}

fn write_header_footer_paragraph<W: Write>(
    writer: &mut Writer<W>,
    para: &Paragraph,
    doc_style: &DocumentStyle,
    ctx: &WriteCtx,
) -> io::Result<()> {
    writer.create_element("w:p").write_inner_content(|w| {
        let has_alignment = para.alignment.is_some();
        let has_mark_size = para.mark_size_half_pt.is_some();
        let has_spacing = para.spacing_before.is_some() || para.spacing_after.is_some();
        if has_alignment || has_mark_size || has_spacing || para.first_line_indent.is_some() {
            w.create_element("w:pPr").write_inner_content(|ppr| {
                if has_spacing {
                    ppr.create_element("w:spacing")
                        .with_attribute((
                            "w:before",
                            para.spacing_before.unwrap_or(0).to_string().as_str(),
                        ))
                        .with_attribute((
                            "w:after",
                            para.spacing_after.unwrap_or(0).to_string().as_str(),
                        ))
                        .write_empty()?;
                }
                if let Some(indent) = para.first_line_indent {
                    write_indentation(
                        ppr,
                        Indentation {
                            first_line: Some(indent.twips.to_string().as_str()),
                            first_line_chars: indent.chars.map(|c| c.to_string()).as_deref(),
                            ..Indentation::default()
                        },
                    )?;
                }
                if let Some(alignment) = &para.alignment {
                    ppr.create_element("w:jc")
                        .with_attribute(("w:val", alignment.ooxml_value()))
                        .write_empty()?;
                }
                if let Some(size) = para.mark_size_half_pt {
                    ppr.create_element("w:rPr").write_inner_content(|rpr| {
                        rpr.create_element("w:sz")
                            .with_attribute(("w:val", size.to_string().as_str()))
                            .write_empty()?;
                        Ok(())
                    })?;
                }
                Ok(())
            })?;
        }
        for inline in &para.inlines {
            match inline {
                InlineElement::Text(run) => write_run(w, run, doc_style)?,
                InlineElement::Hyperlink {
                    url,
                    body,
                    preserve_style,
                } => {
                    write_hyperlink(w, url, body, *preserve_style, ctx)?;
                }
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
                    write_anchored_float(w, float, ctx.next_drawing_id()?, ctx)?;
                }
                InlineElement::Shape(shape) => {
                    write_inline_shape(w, shape, ctx.next_drawing_id()?, ctx)?;
                }
                _ => {}
            }
        }
        Ok(())
    })?;
    Ok(())
}
