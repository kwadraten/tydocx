use std::io::{self, Write};

use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesText, Event};

use super::footnotes::write_footnote_pr;
use super::image::{image_rel_id, svg_rel_id};
use super::{DocParts, RelKind};
use crate::document::{Document, DocumentStyle, ImageData, ImageFormat};

pub(crate) fn xml_part(
    build: impl FnOnce(&mut Writer<&mut Vec<u8>>) -> io::Result<()>,
) -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut writer = Writer::new_with_indent(&mut buf, b' ', 2);
    writer.write_event(Event::Decl(BytesDecl::new(
        "1.0",
        Some("UTF-8"),
        Some("yes"),
    )))?;
    build(&mut writer)?;
    Ok(buf)
}

pub(crate) fn write_value_element<W: Write>(
    writer: &mut Writer<W>,
    name: &str,
    value: &str,
) -> io::Result<()> {
    writer
        .create_element(name)
        .with_attribute(("w:val", value))
        .write_empty()?;
    Ok(())
}

pub(crate) fn write_font_triple<W: Write>(
    writer: &mut Writer<W>,
    ascii: &str,
    east_asia: &str,
    hint_east_asia: bool,
) -> io::Result<()> {
    let mut fonts = writer
        .create_element("w:rFonts")
        .with_attribute(("w:ascii", ascii))
        .with_attribute(("w:hAnsi", ascii))
        .with_attribute(("w:eastAsia", east_asia));
    if hint_east_asia {
        fonts = fonts.with_attribute(("w:hint", "eastAsia"));
    }
    fonts.write_empty()?;
    Ok(())
}

pub(crate) fn write_size_pair<W: Write>(writer: &mut Writer<W>, size: &str) -> io::Result<()> {
    write_value_element(writer, "w:sz", size)?;
    write_value_element(writer, "w:szCs", size)
}

pub(crate) fn write_language_pair<W: Write>(
    writer: &mut Writer<W>,
    name: &str,
    latin: &str,
    east_asia: &str,
) -> io::Result<()> {
    writer
        .create_element(name)
        .with_attribute(("w:val", latin))
        .with_attribute(("w:eastAsia", east_asia))
        .write_empty()?;
    Ok(())
}

/// Indentation attributes for a `w:ind` element. Every field is optional;
/// an absent field is omitted from the output. Call sites set only the
/// attributes they need (`..Default::default()` for the rest) instead of
/// passing four positional `Option<&str>` values.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Indentation<'a> {
    pub left: Option<&'a str>,
    pub right: Option<&'a str>,
    pub hanging: Option<&'a str>,
    pub first_line_chars: Option<&'a str>,
    pub first_line: Option<&'a str>,
}

pub(crate) fn write_indentation<W: Write>(
    writer: &mut Writer<W>,
    indentation: Indentation<'_>,
) -> io::Result<()> {
    let mut element = writer.create_element("w:ind");
    if let Some(value) = indentation.left {
        element = element.with_attribute(("w:left", value));
    }
    if let Some(value) = indentation.right {
        element = element.with_attribute(("w:right", value));
    }
    if let Some(value) = indentation.hanging {
        element = element.with_attribute(("w:hanging", value));
    }
    if let Some(value) = indentation.first_line_chars {
        element = element.with_attribute(("w:firstLineChars", value));
    }
    if let Some(value) = indentation.first_line {
        element = element.with_attribute(("w:firstLine", value));
    }
    element.write_empty()?;
    Ok(())
}

pub(crate) fn two_em_hanging_twips(size_half_pt: u32) -> u32 {
    size_half_pt * 10 * 2
}

pub(super) fn generate_content_types(
    writer: &mut Writer<&mut Vec<u8>>,
    parts: &DocParts,
    images: &[&ImageData],
    has_xsl_item: bool,
) -> io::Result<()> {
    writer
        .create_element("Types")
        .with_attribute(("xmlns", "http://schemas.openxmlformats.org/package/2006/content-types"))
        .write_inner_content(|w| {
            w.create_element("Default")
                .with_attribute(("Extension", "rels"))
                .with_attribute(("ContentType", "application/vnd.openxmlformats-package.relationships+xml"))
                .write_empty()?;
            w.create_element("Default")
                .with_attribute(("Extension", "xml"))
                .with_attribute(("ContentType", "application/xml"))
                .write_empty()?;
            for format in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::Svg] {
                if images.iter().any(|image| {
                    image.format == format || (format == ImageFormat::Svg && image.svg.is_some())
                }) {
                    w.create_element("Default")
                        .with_attribute(("Extension", format.extension()))
                        .with_attribute(("ContentType", format.content_type()))
                        .write_empty()?;
                }
            }
            w.create_element("Override")
                .with_attribute(("PartName", "/word/document.xml"))
                .with_attribute(("ContentType", "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"))
                .write_empty()?;
            w.create_element("Override")
                .with_attribute(("PartName", "/word/styles.xml"))
                .with_attribute(("ContentType", "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"))
                .write_empty()?;
            w.create_element("Override")
                .with_attribute(("PartName", "/word/fontTable.xml"))
                .with_attribute(("ContentType", "application/vnd.openxmlformats-officedocument.wordprocessingml.fontTable+xml"))
                .write_empty()?;
            if parts.has(RelKind::Footnotes) {
                w.create_element("Override")
                    .with_attribute(("PartName", "/word/footnotes.xml"))
                    .with_attribute(("ContentType", "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml"))
                    .write_empty()?;
            }
            if parts.has(RelKind::Numbering) {
                w.create_element("Override")
                    .with_attribute(("PartName", "/word/numbering.xml"))
                    .with_attribute(("ContentType", "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"))
                    .write_empty()?;
            }
            w.create_element("Override")
                .with_attribute(("PartName", "/word/settings.xml"))
                .with_attribute(("ContentType", "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"))
                .write_empty()?;
            for slot in 1_u8..=3 {
                if parts.has(RelKind::Header(slot)) {
                    w.create_element("Override")
                        .with_attribute(("PartName", format!("/word/header{slot}.xml").as_str()))
                        .with_attribute(("ContentType", "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"))
                        .write_empty()?;
                }
            }
            for slot in 1_u8..=3 {
                if parts.has(RelKind::Footer(slot)) {
                    w.create_element("Override")
                        .with_attribute(("PartName", format!("/word/footer{slot}.xml").as_str()))
                        .with_attribute(("ContentType", "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"))
                        .write_empty()?;
                }
            }
            w.create_element("Override")
                .with_attribute(("PartName", "/docProps/core.xml"))
                .with_attribute(("ContentType", "application/vnd.openxmlformats-package.core-properties+xml"))
                .write_empty()?;
            if parts.has(RelKind::Bibliography) {
                w.create_element("Override")
                    .with_attribute(("PartName", "/customXml/itemProps1.xml"))
                    .with_attribute(("ContentType", "application/vnd.openxmlformats-officedocument.customXmlProperties+xml"))
                    .write_empty()?;
                if has_xsl_item {
                    w.create_element("Override")
                        .with_attribute(("PartName", "/customXml/itemProps2.xml"))
                        .with_attribute(("ContentType", "application/vnd.openxmlformats-officedocument.customXmlProperties+xml"))
                        .write_empty()?;
                }
            }
            Ok(())
        })?;
    Ok(())
}

pub(super) fn generate_rels(writer: &mut Writer<&mut Vec<u8>>) -> io::Result<()> {
    writer
        .create_element("Relationships")
        .with_attribute(("xmlns", "http://schemas.openxmlformats.org/package/2006/relationships"))
        .write_inner_content(|w| {
            w.create_element("Relationship")
                .with_attribute(("Id", "rId1"))
                .with_attribute(("Type", "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument"))
                .with_attribute(("Target", "word/document.xml"))
                .write_empty()?;
            w.create_element("Relationship")
                .with_attribute(("Id", "rId2"))
                .with_attribute(("Type", "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties"))
                .with_attribute(("Target", "docProps/core.xml"))
                .write_empty()?;
            Ok(())
        })?;
    Ok(())
}

pub(super) fn generate_document_rels(
    writer: &mut Writer<&mut Vec<u8>>,
    parts: &DocParts,
    images: &[&ImageData],
) -> io::Result<()> {
    writer
        .create_element("Relationships")
        .with_attribute((
            "xmlns",
            "http://schemas.openxmlformats.org/package/2006/relationships",
        ))
        .write_inner_content(|w| {
            // Fixed parts, in the order that defines every rId (see `RelKind`).
            for (idx, kind) in parts.relationships.iter().enumerate() {
                let rid = format!("rId{}", idx + 1);
                let (rel_type, target) = kind.type_and_target();
                w.create_element("Relationship")
                    .with_attribute(("Id", rid.as_str()))
                    .with_attribute(("Type", rel_type.as_str()))
                    .with_attribute(("Target", target.as_str()))
                    .write_empty()?;
            }

            // Image relationships
            for (idx, img) in images.iter().enumerate() {
                let rid = image_rel_id(idx + 1, images, parts);
                let ext = img.format.extension();
                let target = format!("media/image{}.{ext}", idx + 1);
                w.create_element("Relationship")
                    .with_attribute(("Id", rid.as_str()))
                    .with_attribute((
                        "Type",
                        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image",
                    ))
                    .with_attribute(("Target", target.as_str()))
                    .write_empty()?;
                if img.svg.is_some() {
                    let svg_rid = svg_rel_id(idx + 1, images, parts);
                    let svg_target = format!("media/image{}.svg", idx + 1);
                    w.create_element("Relationship")
                        .with_attribute(("Id", svg_rid.as_str()))
                        .with_attribute((
                            "Type",
                            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image",
                        ))
                        .with_attribute(("Target", svg_target.as_str()))
                        .write_empty()?;
                }
            }
            Ok(())
        })?;
    Ok(())
}

pub(super) fn generate_settings(
    writer: &mut Writer<&mut Vec<u8>>,
    style: &DocumentStyle,
    even_and_odd_headers: bool,
    update_fields: bool,
) -> io::Result<()> {
    writer
        .create_element("w:settings")
        .with_attribute((
            "xmlns:w",
            "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
        ))
        .with_attribute((
            "xmlns:m",
            "http://schemas.openxmlformats.org/officeDocument/2006/math",
        ))
        .write_inner_content(|w| {
            if style.suppress_auto_hyphens {
                w.create_element("w:suppressAutoHyphens").write_empty()?;
            }
            if even_and_odd_headers {
                w.create_element("w:evenAndOddHeaders").write_empty()?;
            }
            if update_fields {
                w.create_element("w:updateFields")
                    .with_attribute(("w:val", "true"))
                    .write_empty()?;
            }
            write_footnote_pr(w, &style.footnote_format, style.footnote_restart_each_page)?;
            w.create_element("w:compat").write_inner_content(|c| {
                c.create_element("w:useFELayout").write_empty()?;
                c.create_element("w:compatSetting")
                    .with_attribute(("w:name", "compatibilityMode"))
                    .with_attribute(("w:uri", "http://schemas.microsoft.com/office/word"))
                    .with_attribute(("w:val", "15"))
                    .write_empty()?;
                Ok(())
            })?;
            write_language_pair(
                w,
                "w:themeFontLang",
                style.lang_latin.as_str(),
                style.lang_east_asia.as_str(),
            )?;
            w.create_element("m:mathPr").write_inner_content(|m| {
                m.create_element("m:mathFont")
                    .with_attribute(("m:val", "Cambria Math"))
                    .write_empty()?;
                m.create_element("m:brkBin")
                    .with_attribute(("m:val", "before"))
                    .write_empty()?;
                m.create_element("m:brkBinSub")
                    .with_attribute(("m:val", "--"))
                    .write_empty()?;
                m.create_element("m:smallFrac")
                    .with_attribute(("m:val", "0"))
                    .write_empty()?;
                m.create_element("m:dispDef").write_empty()?;
                m.create_element("m:lMargin")
                    .with_attribute(("m:val", "0"))
                    .write_empty()?;
                m.create_element("m:rMargin")
                    .with_attribute(("m:val", "0"))
                    .write_empty()?;
                m.create_element("m:defJc")
                    .with_attribute(("m:val", "centerGroup"))
                    .write_empty()?;
                m.create_element("m:wrapIndent")
                    .with_attribute(("m:val", "1440"))
                    .write_empty()?;
                m.create_element("m:intLim")
                    .with_attribute(("m:val", "subSup"))
                    .write_empty()?;
                m.create_element("m:naryLim")
                    .with_attribute(("m:val", "undOvr"))
                    .write_empty()?;
                Ok(())
            })?;
            Ok(())
        })?;
    Ok(())
}
pub(super) fn generate_core_properties(
    writer: &mut Writer<&mut Vec<u8>>,
    doc: &Document,
) -> io::Result<()> {
    writer
        .create_element("cp:coreProperties")
        .with_attribute((
            "xmlns:cp",
            "http://schemas.openxmlformats.org/package/2006/metadata/core-properties",
        ))
        .with_attribute(("xmlns:dc", "http://purl.org/dc/elements/1.1/"))
        .with_attribute(("xmlns:dcterms", "http://purl.org/dc/terms/"))
        .with_attribute(("xmlns:xsi", "http://www.w3.org/2001/XMLSchema-instance"))
        .write_inner_content(|w| {
            if let Some(title) = &doc.metadata.title {
                w.create_element("dc:title")
                    .write_text_content(BytesText::new(title))?;
            }
            if let Some(author) = &doc.metadata.author {
                w.create_element("dc:creator")
                    .write_text_content(BytesText::new(author))?;
            }
            if let Some(keywords) = &doc.metadata.keywords {
                w.create_element("cp:keywords")
                    .write_text_content(BytesText::new(keywords))?;
            }
            if let Some(description) = &doc.metadata.description {
                w.create_element("dc:description")
                    .write_text_content(BytesText::new(description))?;
            }
            w.create_element("dcterms:created")
                .with_attribute(("xsi:type", "dcterms:W3CDTF"))
                .write_text_content(BytesText::new(&doc.metadata.created_time()))?;
            Ok(())
        })?;
    Ok(())
}

/// Write a part-local `word/_rels/<part>.xml.rels` holding only image
/// relationships: ids restart at `rId1` (each part has its own rId
/// namespace); the media target keeps the document-wide image number
/// (`image_base + k + 1`) so it matches the file written to `word/media/`.
/// Used for `footnotes.xml.rels` and `headerN.xml.rels`/`footerN.xml.rels`.
pub(super) fn generate_part_image_rels(
    writer: &mut Writer<&mut Vec<u8>>,
    images: &[&ImageData],
    image_base: usize,
) -> io::Result<()> {
    writer
        .create_element("Relationships")
        .with_attribute((
            "xmlns",
            "http://schemas.openxmlformats.org/package/2006/relationships",
        ))
        .write_inner_content(|w| {
            let mut rel = 0_usize;
            for (k, img) in images.iter().enumerate() {
                let n = image_base + k + 1;
                let ext = img.format.extension();
                let target = format!("media/image{n}.{ext}");
                rel += 1;
                w.create_element("Relationship")
                    .with_attribute(("Id", format!("rId{rel}").as_str()))
                    .with_attribute((
                        "Type",
                        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image",
                    ))
                    .with_attribute(("Target", target.as_str()))
                    .write_empty()?;
                if img.svg.is_some() {
                    rel += 1;
                    let svg_target = format!("media/image{n}.svg");
                    w.create_element("Relationship")
                        .with_attribute(("Id", format!("rId{rel}").as_str()))
                        .with_attribute((
                            "Type",
                            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image",
                        ))
                        .with_attribute(("Target", svg_target.as_str()))
                        .write_empty()?;
                }
            }
            Ok(())
        })?;
    Ok(())
}
