use std::io::{self, Write};

use quick_xml::Writer;

use super::citation::write_bibliography_sdt;
use super::fields::write_toc_block;
use super::footnotes::write_footnote_pr;
use super::header_footer::write_footer_refs;
use super::paragraph::write_paragraph;
use super::table::write_table;
use super::{DocParts, RelKind, WriteCtx};
use crate::ooxml::document::{
    BlockElement, Document, DocumentStyle, PageNumberFormat, PageSettings, SectionBreak,
    SectionBreakType,
};

const DEFAULT_HEADER_FOOTER_TWIPS: u32 = 720;

/// Write `w:headerReference`/`w:footerReference` for every header/footer part
/// the document carries. `include_first` adds the `w:type="first"` references,
/// which are only meaningful on a `w:titlePg` section.
fn write_header_footer_refs<W: Write>(
    w: &mut Writer<W>,
    parts: &DocParts,
    include_first: bool,
    settings: &PageSettings,
) -> io::Result<()> {
    for (slot, kind, tag) in [
        (1_u8, "default", "w:headerReference"),
        (2, "even", "w:headerReference"),
        (3, "first", "w:headerReference"),
    ] {
        if slot == 3 && !include_first {
            continue;
        }
        if let Some(rid) = parts.rel_id(RelKind::Header(slot)) {
            w.create_element(tag)
                .with_attribute(("w:type", kind))
                .with_attribute(("r:id", rid.as_str()))
                .write_empty()?;
        }
    }
    write_footer_refs(w, parts, include_first, settings.footer_reference)
}

/// Write `<w:pgNumType>` with optional `w:fmt` and `w:start` attributes.
fn write_pg_num_type<W: Write>(
    w: &mut Writer<W>,
    fmt: Option<&PageNumberFormat>,
    start: Option<u32>,
) -> io::Result<()> {
    if fmt.is_none() && start.is_none() {
        return Ok(());
    }
    let mut elem = w.create_element("w:pgNumType");
    if let Some(fmt) = fmt {
        elem = elem.with_attribute(("w:fmt", fmt.ooxml_value()));
    }
    if let Some(start) = start {
        let start = start.to_string();
        elem = elem.with_attribute(("w:start", start.as_str()));
    }
    elem.write_empty()?;
    Ok(())
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
                    BlockElement::TocBlock {
                        max_depth,
                        caption_id,
                        paragraphs,
                    } => {
                        write_toc_block(
                            body_w,
                            *max_depth,
                            caption_id.as_deref(),
                            paragraphs,
                            ctx,
                        )?;
                    }
                }
            }
            // `w:titlePg` on the final sectPr only applies when the document is
            // a single section — with earlier sections the flag belongs to the
            // mid-body sectPr that ends the section containing page 1.
            let is_single_section =
                !doc.body.elements.iter().any(
                    |el| matches!(el, BlockElement::Paragraph(p) if p.section_break.is_some()),
                );
            write_section_properties(
                body_w,
                &doc.page_settings,
                &doc.style,
                ctx.parts,
                &SectionTail {
                    page_numbering: doc.page_numbering.as_ref(),
                    page_number_start: doc.page_number_start,
                    title_page: doc.title_page && is_single_section,
                    section_type: doc.final_section_type,
                },
            )?;
            Ok(())
        })?;
        Ok(())
    })?;
    Ok(())
}

/// The body-final `w:sectPr`'s section descriptor — everything past the
/// page settings.
struct SectionTail<'a> {
    page_numbering: Option<&'a PageNumberFormat>,
    page_number_start: Option<u32>,
    title_page: bool,
    section_type: Option<SectionBreakType>,
}

fn write_section_properties<W: Write>(
    writer: &mut Writer<W>,
    settings: &PageSettings,
    style: &DocumentStyle,
    parts: &DocParts,
    tail: &SectionTail<'_>,
) -> io::Result<()> {
    writer.create_element("w:sectPr").write_inner_content(|w| {
        write_header_footer_refs(w, parts, tail.title_page, settings)?;
        write_footnote_pr(w, &style.footnote_format, style.footnote_restart_each_page)?;
        // CT_SectPr orders w:type between footnotePr and pgSz; omitted
        // means nextPage.
        if let Some(section_type) = tail.section_type {
            w.create_element("w:type")
                .with_attribute(("w:val", section_type.ooxml_value()))
                .write_empty()?;
        }
        write_section_page_settings(w, settings, tail.page_numbering, tail.page_number_start)?;
        if tail.title_page {
            w.create_element("w:titlePg").write_empty()?;
        }
        Ok(())
    })?;
    Ok(())
}

/// Write page size, margins, page-numbering, columns, and alignment for a section.
fn write_section_page_settings<W: Write>(
    w: &mut Writer<W>,
    settings: &PageSettings,
    page_numbering: Option<&PageNumberFormat>,
    page_number_start: Option<u32>,
) -> io::Result<()> {
    let pg_sz = w
        .create_element("w:pgSz")
        .with_attribute(("w:w", settings.width_twips.to_string().as_str()))
        .with_attribute(("w:h", settings.height_twips.to_string().as_str()));
    if settings.width_twips > settings.height_twips {
        pg_sz
            .with_attribute(("w:orient", "landscape"))
            .write_empty()?;
    } else {
        pg_sz.write_empty()?;
    }
    w.create_element("w:pgMar")
        .with_attribute(("w:top", settings.margin_top.to_string().as_str()))
        .with_attribute(("w:right", settings.margin_right.to_string().as_str()))
        .with_attribute(("w:bottom", settings.margin_bottom.to_string().as_str()))
        .with_attribute(("w:left", settings.margin_left.to_string().as_str()))
        .with_attribute((
            "w:header",
            settings
                .header_twips
                .unwrap_or(DEFAULT_HEADER_FOOTER_TWIPS)
                .to_string()
                .as_str(),
        ))
        .with_attribute((
            "w:footer",
            settings
                .footer_twips
                .unwrap_or(DEFAULT_HEADER_FOOTER_TWIPS)
                .to_string()
                .as_str(),
        ))
        .with_attribute(("w:gutter", "0"))
        .write_empty()?;
    if let Some(distance) = settings.line_number_distance_twips {
        let distance = distance.to_string();
        w.create_element("w:lnNumType")
            .with_attribute(("w:countBy", "1"))
            .with_attribute(("w:start", "0"))
            .with_attribute(("w:distance", distance.as_str()))
            .with_attribute(("w:restart", "continuous"))
            .write_empty()?;
    }
    write_pg_num_type(w, page_numbering, page_number_start)?;
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
///
/// The `sectPr` describes the section that *ends* at this paragraph: its page
/// settings, page numbering, and (when the section owns page 1) `w:titlePg`.
pub(super) fn write_section_break<W: Write>(
    writer: &mut Writer<W>,
    section: &SectionBreak,
    parts: &DocParts,
) -> io::Result<()> {
    writer.create_element("w:sectPr").write_inner_content(|w| {
        let default_settings = PageSettings::default();
        let ps = section.page_settings.as_ref().unwrap_or(&default_settings);
        write_header_footer_refs(w, parts, section.title_page, ps)?;
        let break_val = section.break_type.ooxml_value();
        w.create_element("w:type")
            .with_attribute(("w:val", break_val))
            .write_empty()?;
        write_section_page_settings(
            w,
            ps,
            section.page_numbering.as_ref(),
            section.page_number_start,
        )?;
        if section.title_page {
            w.create_element("w:titlePg").write_empty()?;
        }
        Ok(())
    })?;
    Ok(())
}
