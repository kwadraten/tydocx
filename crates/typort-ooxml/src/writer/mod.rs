use std::cell::Cell;
use std::error::Error;
use std::io::{Seek, Write};

use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::document::{BlockElement, CellContent, Document, DocumentStyle, Table};
use crate::styles;

mod citation;
mod document;
mod fields;
mod footnotes;
mod header_footer;
mod image;
mod math;
mod numbering;
pub(crate) mod package;
mod paragraph;
mod run;
mod table;

use citation::{
    generate_custom_xml_item_props, generate_custom_xml_rels, generate_custom_xml_sources,
};
use document::generate_document_xml;
use footnotes::{generate_footnote_rels, generate_footnotes_xml};
use header_footer::{generate_footer_xml, generate_header_xml, generate_page_number_footer_xml};
use image::{body_has_drawing, collect_footnote_images, collect_images, footnotes_have_drawing};
use numbering::generate_numbering_xml;
use package::{
    generate_content_types, generate_core_properties, generate_document_rels, generate_rels,
    generate_settings, xml_part,
};

/// Tracks which optional document parts are present, used for rId assignment
/// and conditional XML generation.
struct DocParts {
    relationships: Vec<RelKind>,
}

impl DocParts {
    fn new(doc: &Document) -> Self {
        let mut relationships = vec![RelKind::Styles, RelKind::FontTable];
        if !doc.footnotes.is_empty() {
            relationships.push(RelKind::Footnotes);
        }
        if doc_has_lists(doc) {
            relationships.push(RelKind::Numbering);
        }
        relationships.push(RelKind::Settings);
        if doc.header.is_some() {
            relationships.push(RelKind::Header(1));
        }
        if doc.header_even.is_some() {
            relationships.push(RelKind::Header(2));
        }
        if doc.header_first.is_some() {
            relationships.push(RelKind::Header(3));
        }
        if doc.footer.is_some() || doc.page_number_footers.default {
            relationships.push(RelKind::Footer(1));
        }
        if doc.footer_even.is_some() || doc.page_number_footers.even {
            relationships.push(RelKind::Footer(2));
        }
        if doc.footer_first.is_some() || doc.page_number_footers.first {
            relationships.push(RelKind::Footer(3));
        }
        if !doc.citation_sources.is_empty() {
            relationships.push(RelKind::Bibliography);
        }
        Self { relationships }
    }

    fn has(&self, kind: RelKind) -> bool {
        self.relationships.contains(&kind)
    }

    /// The `rId` for this part (1-based position in `document.xml.rels`).
    fn rel_id(&self, kind: RelKind) -> Option<String> {
        self.relationships
            .iter()
            .position(|k| *k == kind)
            .map(|i| format!("rId{}", i + 1))
    }
}

/// Read-only context threaded through the body writers (`write_paragraph`,
/// `write_table`, `write_bibliography_sdt`), replacing the
/// `(doc_style, parts, content_width, image_counter, citation_id_counter)` tuple
/// that previously appeared on each. All fields are shared, so this is passed
/// by `&WriteCtx`; the `Cell`s carry the running image/citation ids.
struct WriteCtx<'a> {
    doc_style: &'a DocumentStyle,
    parts: &'a DocParts,
    content_width_twips: u32,
    image_counter: &'a Cell<usize>,
    citation_id_counter: &'a Cell<u32>,
    /// `wp:docPr` ids for anchored shape drawings. Starts high so it cannot
    /// collide with the 1-based image `docPr` ids in the same part.
    shape_id_counter: &'a Cell<usize>,
}

/// Write a Document to a .docx file (ZIP archive) into the given writer.
///
/// # Errors
/// Returns an error if XML serialization or ZIP writing fails.
pub fn write_docx<W: Write + Seek>(doc: &Document, writer: W) -> Result<(), Box<dyn Error>> {
    let parts = DocParts::new(doc);
    let images = collect_images(doc);
    let footnote_images = collect_footnote_images(doc);
    // Footnote images are numbered after the body images in the
    // document-wide media numbering.
    let footnote_image_base = images.len() - footnote_images.len();

    // Running image/citation ids are shared across document.xml and
    // footnotes.xml, so they never restart per part.
    let image_counter = Cell::new(0_usize);
    let citation_id_counter = Cell::new(1_000_000_u32);
    let shape_id_counter = Cell::new(1_000_000_usize);
    let ps = &doc.page_settings;
    let ctx = WriteCtx {
        doc_style: &doc.style,
        parts: &parts,
        content_width_twips: ps
            .width_twips
            .saturating_sub(ps.margin_left + ps.margin_right),
        image_counter: &image_counter,
        citation_id_counter: &citation_id_counter,
        shape_id_counter: &shape_id_counter,
    };

    let mut zip = ZipWriter::new(writer);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("[Content_Types].xml", options)?;
    zip.write_all(&xml_part(|w| generate_content_types(w, &parts, &images))?)?;

    zip.start_file("_rels/.rels", options)?;
    zip.write_all(&xml_part(generate_rels)?)?;

    zip.start_file("word/_rels/document.xml.rels", options)?;
    zip.write_all(&xml_part(|w| generate_document_rels(w, &parts, &images))?)?;

    zip.start_file("word/styles.xml", options)?;
    zip.write_all(&xml_part(|w| {
        styles::generate_styles(w, parts.has(RelKind::Footnotes), &doc.style)
    })?)?;

    zip.start_file("word/fontTable.xml", options)?;
    zip.write_all(&xml_part(|w| styles::generate_font_table(w, &doc.style))?)?;

    zip.start_file("word/settings.xml", options)?;
    zip.write_all(&xml_part(|w| {
        generate_settings(w, &doc.style, doc.even_and_odd_headers)
    })?)?;

    zip.start_file("word/document.xml", options)?;
    zip.write_all(&xml_part(|w| {
        generate_document_xml(w, doc, body_has_drawing(doc), &ctx)
    })?)?;

    if parts.has(RelKind::Footnotes) {
        zip.start_file("word/footnotes.xml", options)?;
        zip.write_all(&xml_part(|w| {
            generate_footnotes_xml(
                w,
                doc,
                &ctx,
                footnote_image_base,
                footnotes_have_drawing(doc),
            )
        })?)?;
        if !footnote_images.is_empty() {
            zip.start_file("word/_rels/footnotes.xml.rels", options)?;
            zip.write_all(&xml_part(|w| {
                generate_footnote_rels(w, &footnote_images, footnote_image_base)
            })?)?;
        }
    }

    if parts.has(RelKind::Numbering) {
        zip.start_file("word/numbering.xml", options)?;
        zip.write_all(&xml_part(|w| generate_numbering_xml(w, doc))?)?;
    }

    write_header_footer_parts(&mut zip, doc, options)?;

    // Write image files to word/media/
    for (idx, img) in images.iter().enumerate() {
        let n = idx + 1;
        let ext = img.format.extension();
        let path = format!("word/media/image{n}.{ext}");
        zip.start_file(path, options)?;
        zip.write_all(&img.bytes)?;
    }

    // Always write docProps/core.xml with metadata
    zip.start_file("docProps/core.xml", options)?;
    zip.write_all(&xml_part(|w| generate_core_properties(w, doc))?)?;

    // Custom XML bibliography data source
    if parts.has(RelKind::Bibliography) {
        zip.start_file("customXml/item1.xml", options)?;
        zip.write_all(&xml_part(|w| {
            generate_custom_xml_sources(w, &doc.citation_sources)
        })?)?;

        zip.start_file("customXml/itemProps1.xml", options)?;
        zip.write_all(&xml_part(|w| {
            generate_custom_xml_item_props(w, &doc.citation_sources)
        })?)?;

        zip.start_file("customXml/_rels/item1.xml.rels", options)?;
        zip.write_all(&xml_part(generate_custom_xml_rels)?)?;
    }

    zip.finish()?;
    Ok(())
}

/// Write `word/headerN.xml`/`word/footerN.xml` — slot 1 = `w:type="default"`,
/// 2 = `w:type="even"`, 3 = `w:type="first"`. A generated PAGE-field footer
/// wins over authored content in its slot.
fn write_header_footer_parts<W: Write + Seek>(
    zip: &mut ZipWriter<W>,
    doc: &Document,
    options: SimpleFileOptions,
) -> Result<(), Box<dyn Error>> {
    for (slot, content) in [
        (1_u8, &doc.header),
        (2, &doc.header_even),
        (3, &doc.header_first),
    ] {
        if let Some(header) = content {
            zip.start_file(format!("word/header{slot}.xml"), options)?;
            zip.write_all(&xml_part(|w| generate_header_xml(w, header, &doc.style))?)?;
        }
    }

    for (slot, content, generated) in [
        (1_u8, &doc.footer, doc.page_number_footers.default),
        (2, &doc.footer_even, doc.page_number_footers.even),
        (3, &doc.footer_first, doc.page_number_footers.first),
    ] {
        if generated {
            zip.start_file(format!("word/footer{slot}.xml"), options)?;
            zip.write_all(&xml_part(generate_page_number_footer_xml)?)?;
        } else if let Some(footer) = content {
            zip.start_file(format!("word/footer{slot}.xml"), options)?;
            zip.write_all(&xml_part(|w| generate_footer_xml(w, footer, &doc.style))?)?;
        }
    }
    Ok(())
}

/// Check if the document needs the numbering part: list items (paragraphs
/// with `list_info`) or numbered headings (allocated heading instances).
fn doc_has_lists(doc: &Document) -> bool {
    !doc.heading_num_instances.is_empty()
        || doc.body.elements.iter().any(|el| match el {
            BlockElement::Paragraph(p) => p.list_info.is_some(),
            BlockElement::Table(table) => table_has_lists(table),
            BlockElement::BibliographyBlock { paragraphs } => {
                paragraphs.iter().any(|p| p.list_info.is_some())
            }
        })
}

/// A list anywhere inside a table — a cell paragraph, a structured cell
/// paragraph, or a nested table — needs the numbering part too.
fn table_has_lists(table: &Table) -> bool {
    table.rows.iter().any(|row| {
        row.cells.iter().any(|cell| {
            cell.paragraphs.iter().any(|p| p.list_info.is_some())
                || cell.content.iter().any(|item| match item {
                    CellContent::Paragraph(p) => p.list_info.is_some(),
                    CellContent::Table(nested) => table_has_lists(nested),
                })
        })
    })
}
/// A fixed (non-image) relationship part of `document.xml.rels`.
///
/// The order stored in [`DocParts::relationships`] is the **single source of truth**
/// for rId assignment. Images follow the fixed relationships.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RelKind {
    Styles,
    FontTable,
    Footnotes,
    Numbering,
    Settings,
    /// Header part slot: 1 = `w:type="default"`, 2 = `w:type="even"`,
    /// 3 = `w:type="first"`.
    Header(u8),
    /// Footer part slot (same numbering as `Header`).
    Footer(u8),
    Bibliography,
}

impl RelKind {
    /// The relationship `Type` URL and `Target` filename for this part.
    fn type_and_target(self) -> (String, String) {
        const PREFIX: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";
        let (suffix, target) = match self {
            RelKind::Styles => ("styles", "styles.xml".to_string()),
            RelKind::FontTable => ("fontTable", "fontTable.xml".to_string()),
            RelKind::Footnotes => ("footnotes", "footnotes.xml".to_string()),
            RelKind::Numbering => ("numbering", "numbering.xml".to_string()),
            RelKind::Settings => ("settings", "settings.xml".to_string()),
            RelKind::Header(n) => ("header", format!("header{n}.xml")),
            RelKind::Footer(n) => ("footer", format!("footer{n}.xml")),
            RelKind::Bibliography => ("customXml", "../customXml/item1.xml".to_string()),
        };
        (format!("{PREFIX}{suffix}"), target)
    }
}
