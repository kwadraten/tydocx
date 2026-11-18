use std::io::{self, Write};

use quick_xml::Writer;

use super::citation::write_bibliography_sdt;
use super::footnotes::write_footnote_pr;
use super::paragraph::write_paragraph;
use super::table::write_table;
use super::{DocParts, RelKind, WriteCtx};
use crate::document::{
    BlockElement, Document, DocumentStyle, PageNumberFormat, PageSettings, SectionBreak,
};

/// Compute the relationship ID for a header or footer by locating it in the
/// single source of truth (`DocParts::relationships`). Only called when the part
/// is present, as guarded by the call sites.
fn header_footer_rel_id(is_header: bool, parts: &DocParts) -> String {
    let kind = if is_header {
        RelKind::Header
    } else {
        RelKind::Footer
    };
    let pos = parts
        .relationships
        .iter()
        .position(|k| *k == kind)
        .expect("header/footer rel id requested for a part that is not present");
    format!("rId{}", pos + 1)
}

/// Write `word/document.xml`. `ctx` is created by the caller (`write_docx`)
/// and shared with the footnote writer, so the running image/citation ids
/// are document-wide instead of restarting per part.
pub(super) fn generate_document_xml(
    writer: &mut Writer<&mut Vec<u8>>,
    doc: &Document,
    has_drawings: bool,
    ctx: &WriteCtx,
) -> io::Result<()> {
    let mut elem = writer
        .create_element("w:document")
        .with_attribute((
            "xmlns:w",
            "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
        ))
        .with_attribute((
            "xmlns:r",
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
        ))
        .with_attribute((
            "xmlns:m",
            "http://schemas.openxmlformats.org/officeDocument/2006/math",
        ));
    if has_drawings {
        elem = elem
            .with_attribute((
                "xmlns:wp",
                "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing",
            ))
            .with_attribute((
                "xmlns:a",
                "http://schemas.openxmlformats.org/drawingml/2006/main",
            ))
            .with_attribute((
                "xmlns:pic",
                "http://schemas.openxmlformats.org/drawingml/2006/picture",
            ));
    }
    elem.write_inner_content(|w| {
        w.create_element("w:body").write_inner_content(|body_w| {
            for element in &doc.body.elements {
                match element {
                    BlockElement::Paragraph(para) => {
                        write_paragraph(body_w, para, ctx)?;
                    }
                    BlockElement::Table(table) => {
                        write_table(body_w, table, ctx)?;
                    }
                    BlockElement::BibliographyBlock { paragraphs } => {
                        write_bibliography_sdt(body_w, paragraphs, ctx)?;
                    }
                }
            }
            write_section_properties(
                body_w,
                &doc.page_settings,
                &doc.style,
                ctx.parts,
                doc.page_numbering.as_ref(),
            )?;
            Ok(())
        })?;
        Ok(())
    })?;
    Ok(())
}

fn write_section_properties<W: Write>(
    writer: &mut Writer<W>,
    settings: &PageSettings,
    style: &DocumentStyle,
    parts: &DocParts,
    page_numbering: Option<&PageNumberFormat>,
) -> io::Result<()> {
    writer.create_element("w:sectPr").write_inner_content(|w| {
        // Header reference
        if parts.has(RelKind::Header) {
            let rid = header_footer_rel_id(true, parts);
            w.create_element("w:headerReference")
                .with_attribute(("w:type", "default"))
                .with_attribute(("r:id", rid.as_str()))
                .write_empty()?;
        }
        // Footer reference
        if parts.has(RelKind::Footer) {
            let rid = header_footer_rel_id(false, parts);
            w.create_element("w:footerReference")
                .with_attribute(("w:type", "default"))
                .with_attribute(("r:id", rid.as_str()))
                .write_empty()?;
        }
        write_footnote_pr(w, &style.footnote_format, style.footnote_restart_each_page)?;
        // Page number format (w:pgNumType)
        if let Some(fmt) = page_numbering {
            w.create_element("w:pgNumType")
                .with_attribute(("w:fmt", fmt.ooxml_value()))
                .write_empty()?;
        }
        write_section_page_settings(w, settings)?;
        Ok(())
    })?;
    Ok(())
}

/// Write page size, margins, columns, and document grid for a section.
fn write_section_page_settings<W: Write>(
    w: &mut Writer<W>,
    settings: &PageSettings,
) -> io::Result<()> {
    w.create_element("w:pgSz")
        .with_attribute(("w:w", settings.width_twips.to_string().as_str()))
        .with_attribute(("w:h", settings.height_twips.to_string().as_str()))
        .write_empty()?;
    let pg_mar = w
        .create_element("w:pgMar")
        .with_attribute(("w:top", settings.margin_top.to_string().as_str()))
        .with_attribute(("w:right", settings.margin_right.to_string().as_str()))
        .with_attribute(("w:bottom", settings.margin_bottom.to_string().as_str()))
        .with_attribute(("w:left", settings.margin_left.to_string().as_str()));
    let pg_mar = match settings.header_twips {
        Some(v) => pg_mar.with_attribute(("w:header", v.to_string().as_str())),
        None => pg_mar,
    };
    let pg_mar = match settings.footer_twips {
        Some(v) => pg_mar.with_attribute(("w:footer", v.to_string().as_str())),
        None => pg_mar,
    };
    pg_mar.write_empty()?;
    if let Some(distance) = settings.line_number_distance_twips {
        let distance = distance.to_string();
        w.create_element("w:lnNumType")
            .with_attribute(("w:countBy", "1"))
            .with_attribute(("w:start", "0"))
            .with_attribute(("w:distance", distance.as_str()))
            .with_attribute(("w:restart", "continuous"))
            .write_empty()?;
    }
    // Columns
    if let Some(cols) = settings.columns.filter(|&c| c > 1) {
        let num = cols.to_string();
        let space = settings.column_spacing.unwrap_or(720).to_string();
        w.create_element("w:cols")
            .with_attribute(("w:num", num.as_str()))
            .with_attribute(("w:space", space.as_str()))
            .write_empty()?;
    }
    if let Some(v_align) = settings.vertical_align {
        w.create_element("w:vAlign")
            .with_attribute(("w:val", v_align.ooxml_value()))
            .write_empty()?;
    }
    // No w:docGrid: Pandoc-aligned. A docGrid linePitch re-imposes a geometric
    // line grid; omitting it lets Word flow text with default single spacing.
    Ok(())
}
/// Write a `w:sectPr` element inside a paragraph's `w:pPr` for a section break.
pub(super) fn write_section_break<W: Write>(
    writer: &mut Writer<W>,
    section: &SectionBreak,
) -> io::Result<()> {
    writer.create_element("w:sectPr").write_inner_content(|w| {
        let break_val = section.break_type.ooxml_value();
        w.create_element("w:type")
            .with_attribute(("w:val", break_val))
            .write_empty()?;
        if let Some(ps) = &section.page_settings {
            write_section_page_settings(w, ps)?;
        } else {
            // Use default page settings
            write_section_page_settings(w, &PageSettings::default())?;
        }
        Ok(())
    })?;
    Ok(())
}
