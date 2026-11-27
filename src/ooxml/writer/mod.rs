use std::cell::Cell;
use std::error::Error;
use std::io::{self, Seek, Write};

use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::ooxml::document::{
    BlockElement, CellContent, Document, DocumentStyle, HeaderFooter, ImageData, Paragraph,
    SectionFooter, Table,
};
use crate::ooxml::styles;

mod citation;
mod document;
mod fields;
mod footnotes;
mod header_footer;
mod hyperlinks;
mod image;
mod math;
mod numbering;
pub(crate) mod package;
mod paragraph;
mod run;
mod shape;
mod table;

use citation::{
    generate_custom_xml_item_props, generate_custom_xml_rels, generate_custom_xml_sources,
    generate_item2_rels, generate_xsl_item_props,
};
use document::generate_document_xml;
use footnotes::{footnotes_have_math, generate_footnotes_xml};
use header_footer::{
    EMPTY_FOOTER_SLOT, PartCtx, generate_footer_xml, generate_header_xml,
    generate_page_number_footer_xml,
};
use hyperlinks::HyperlinkRelationships;
use image::{
    body_has_drawing, collect_footnote_images, collect_header_footer_images, collect_images,
    footnotes_have_drawing,
};
use numbering::generate_numbering_xml;
use package::{
    generate_content_types, generate_core_properties, generate_document_rels, generate_part_rels,
    generate_rels, generate_settings, xml_part,
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
        if doc.page_settings.footer_reference == SectionFooter::Empty
            || doc.body.elements.iter().any(|block| {
                matches!(block, BlockElement::Paragraph(para)
                    if para.section_break.as_ref()
                        .and_then(|b| b.page_settings.as_ref())
                        .is_some_and(|s| s.footer_reference == SectionFooter::Empty))
            })
        {
            relationships.push(RelKind::Footer(EMPTY_FOOTER_SLOT));
        }
        if !doc.citation_sources.is_empty() {
            relationships.push(RelKind::Bibliography);
        }
        Self { relationships }
    }

    /// An empty parts table for a header/footer-part-local `WriteCtx` — its
    /// image relationships restart at `rId1` against the part's own `.rels`.
    fn empty() -> Self {
        Self {
            relationships: Vec::new(),
        }
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
    images: &'a [&'a ImageData],
    hyperlinks: &'a HyperlinkRelationships,
    content_width_twips: u32,
    image_counter: &'a Cell<usize>,
    citation_id_counter: &'a Cell<u32>,
    drawing_id_counter: &'a Cell<usize>,
}

impl WriteCtx<'_> {
    fn next_drawing_id(&self) -> io::Result<usize> {
        let next = self.drawing_id_counter.get() + 1;
        if next > 2_147_483_646 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "drawing ID exceeds Word's supported range",
            ));
        }
        self.drawing_id_counter.set(next);
        Ok(next)
    }
}

/// Write a Document to a .docx file (ZIP archive) into the given writer.
///
/// # Errors
/// Returns an error if XML serialization or ZIP writing fails.
pub fn write_docx<W: Write + Seek>(doc: &Document, writer: W) -> Result<(), Box<dyn Error>> {
    let parts = DocParts::new(doc);
    let images = collect_images(doc);
    let footnote_images = collect_footnote_images(doc);
    let hf_images = collect_header_footer_images(doc);
    let hf_image_count: usize = hf_images.iter().map(Vec::len).sum();
    // Footnote images are numbered after the body images in the
    // document-wide media numbering; header/footer-part images come last.
    let footnote_image_base = images.len() - footnote_images.len() - hf_image_count;
    let hf_image_base = images.len() - hf_image_count;

    let image_counter = Cell::new(0_usize);
    let citation_id_counter = Cell::new(1_000_000_u32);
    let drawing_id_counter = Cell::new(0_usize);
    let ps = &doc.page_settings;
    let hyperlinks = HyperlinkRelationships::default();
    let ctx = WriteCtx {
        doc_style: &doc.style,
        parts: &parts,
        images: &images,
        hyperlinks: &hyperlinks,
        content_width_twips: ps
            .width_twips
            .saturating_sub(ps.margin_left + ps.margin_right),
        image_counter: &image_counter,
        citation_id_counter: &citation_id_counter,
        drawing_id_counter: &drawing_id_counter,
    };

    let mut zip = ZipWriter::new(writer);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("[Content_Types].xml", options)?;
    zip.write_all(&xml_part(|w| {
        generate_content_types(w, &parts, &images, doc.bibliography_style_xsl.is_some())
    })?)?;

    zip.start_file("_rels/.rels", options)?;
    zip.write_all(&xml_part(generate_rels)?)?;

    zip.start_file("word/styles.xml", options)?;
    zip.write_all(&xml_part(|w| {
        styles::generate_styles(w, parts.has(RelKind::Footnotes), &doc.style)
    })?)?;

    zip.start_file("word/fontTable.xml", options)?;
    zip.write_all(&xml_part(|w| styles::generate_font_table(w, &doc.style))?)?;

    zip.start_file("word/settings.xml", options)?;
    zip.write_all(&xml_part(|w| {
        generate_settings(
            w,
            &doc.style,
            doc.even_and_odd_headers,
            doc.mirror_margins,
            doc.update_fields,
        )
    })?)?;

    zip.start_file("word/document.xml", options)?;
    zip.write_all(&xml_part(|w| {
        generate_document_xml(w, doc, body_has_drawing(doc), &ctx)
    })?)?;

    zip.start_file("word/_rels/document.xml.rels", options)?;
    zip.write_all(&xml_part(|w| {
        generate_document_rels(w, &parts, &images, &hyperlinks)
    })?)?;

    if parts.has(RelKind::Footnotes) {
        let part = PartCtx::new(footnote_images);
        write_footnotes_part(&mut zip, doc, options, &ctx, &part, footnote_image_base)?;
    }

    if parts.has(RelKind::Numbering) {
        zip.start_file("word/numbering.xml", options)?;
        zip.write_all(&xml_part(|w| generate_numbering_xml(w, doc))?)?;
    }

    write_header_footer_parts(&mut zip, doc, options, &ctx, &hf_images, hf_image_base)?;

    // Write image files to word/media/
    for (idx, img) in images.iter().enumerate() {
        let n = idx + 1;
        let ext = img.format.extension();
        let path = format!("word/media/image{n}.{ext}");
        zip.start_file(path, options)?;
        zip.write_all(&img.bytes)?;
        if let Some(svg) = &img.svg {
            let svg_path = format!("word/media/image{n}.svg");
            zip.start_file(svg_path, options)?;
            zip.write_all(svg)?;
        }
    }

    // Always write docProps/core.xml with metadata
    zip.start_file("docProps/core.xml", options)?;
    zip.write_all(&xml_part(|w| generate_core_properties(w, doc))?)?;

    // Custom XML bibliography data source
    if parts.has(RelKind::Bibliography) {
        write_bibliography_parts(&mut zip, doc, options)?;
    }

    zip.finish()?;
    Ok(())
}

fn write_footnotes_part<W: Write + Seek>(
    zip: &mut ZipWriter<W>,
    doc: &Document,
    options: SimpleFileOptions,
    ctx: &WriteCtx,
    part: &PartCtx,
    image_base: usize,
) -> Result<(), Box<dyn Error>> {
    zip.start_file("word/footnotes.xml", options)?;
    zip.write_all(&xml_part(|w| {
        generate_footnotes_xml(
            w,
            doc,
            &part.ctx(ctx),
            footnotes_have_drawing(doc),
            footnotes_have_math(doc),
        )
    })?)?;
    if !part.images.is_empty() || !part.hyperlinks.is_empty() {
        zip.start_file("word/_rels/footnotes.xml.rels", options)?;
        zip.write_all(&xml_part(|w| {
            generate_part_rels(w, &part.images, image_base, &part.hyperlinks)
        })?)?;
    }
    Ok(())
}

/// Write the `customXml/` bibliography item set: `item1.xml` carries the
/// Sources.xml data source plus its property/rels companions. A
/// `--bibliography-style` XSL travels as a second custom-XML item: Word
/// resolves `SelectedStyle` from its installed style directories, so the
/// embedded copy is payload, not activation — tools that can read the
/// package can extract and install it.
fn write_bibliography_parts<W: Write + Seek>(
    zip: &mut ZipWriter<W>,
    doc: &Document,
    options: SimpleFileOptions,
) -> Result<(), Box<dyn Error>> {
    zip.start_file("customXml/item1.xml", options)?;
    zip.write_all(&xml_part(|w| {
        generate_custom_xml_sources(w, &doc.citation_sources, doc.bibliography_style.as_ref())
    })?)?;

    zip.start_file("customXml/itemProps1.xml", options)?;
    zip.write_all(&xml_part(|w| {
        generate_custom_xml_item_props(w, &doc.citation_sources)
    })?)?;

    zip.start_file("customXml/_rels/item1.xml.rels", options)?;
    zip.write_all(&xml_part(generate_custom_xml_rels)?)?;

    if let Some(xsl) = &doc.bibliography_style_xsl {
        zip.start_file("customXml/item2.xml", options)?;
        zip.write_all(xsl)?;

        zip.start_file("customXml/itemProps2.xml", options)?;
        zip.write_all(&xml_part(generate_xsl_item_props)?)?;

        zip.start_file("customXml/_rels/item2.xml.rels", options)?;
        zip.write_all(&xml_part(generate_item2_rels)?)?;
    }
    Ok(())
}

/// Write `word/headerN.xml`/`word/footerN.xml` — slot 1 = `w:type="default"`,
/// 2 = `w:type="even"`, 3 = `w:type="first"`. A generated PAGE-field footer
/// wins over authored content in its slot. `hf_images` holds each part's
/// images in the same order; a part with images also gets its own
/// `word/_rels/<part>.xml.rels` mapping its local `rId`s onto the
/// document-wide media numbering (`hf_image_base`).
fn write_header_footer_parts<W: Write + Seek>(
    zip: &mut ZipWriter<W>,
    doc: &Document,
    options: SimpleFileOptions,
    ctx: &WriteCtx,
    hf_images: &[Vec<&ImageData>],
    hf_image_base: usize,
) -> Result<(), Box<dyn Error>> {
    let mut media_offset = hf_image_base;
    for (i, (slot, content)) in [
        (1_u8, &doc.header),
        (2, &doc.header_even),
        (3, &doc.header_first),
    ]
    .into_iter()
    .enumerate()
    {
        let part_images = hf_images.get(i).map_or(&[][..], Vec::as_slice);
        if let Some(header) = content {
            let part = PartCtx::new(part_images.to_vec());
            zip.start_file(format!("word/header{slot}.xml"), options)?;
            zip.write_all(&xml_part(|w| {
                generate_header_xml(w, header, &doc.style, &part.ctx(ctx))
            })?)?;
            if !part_images.is_empty() || !part.hyperlinks.is_empty() {
                zip.start_file(format!("word/_rels/header{slot}.xml.rels"), options)?;
                zip.write_all(&xml_part(|w| {
                    generate_part_rels(w, part_images, media_offset, &part.hyperlinks)
                })?)?;
            }
        }
        media_offset += part_images.len();
    }

    for (i, (slot, content, generated)) in [
        (1_u8, &doc.footer, doc.page_number_footers.default),
        (2, &doc.footer_even, doc.page_number_footers.even),
        (3, &doc.footer_first, doc.page_number_footers.first),
    ]
    .into_iter()
    .enumerate()
    {
        let part_images = hf_images.get(i + 3).map_or(&[][..], Vec::as_slice);
        if generated {
            zip.start_file(format!("word/footer{slot}.xml"), options)?;
            zip.write_all(&xml_part(generate_page_number_footer_xml)?)?;
        } else if let Some(footer) = content {
            let part = PartCtx::new(part_images.to_vec());
            zip.start_file(format!("word/footer{slot}.xml"), options)?;
            zip.write_all(&xml_part(|w| {
                generate_footer_xml(w, footer, &doc.style, &part.ctx(ctx))
            })?)?;
            if !part_images.is_empty() || !part.hyperlinks.is_empty() {
                zip.start_file(format!("word/_rels/footer{slot}.xml.rels"), options)?;
                zip.write_all(&xml_part(|w| {
                    generate_part_rels(w, part_images, media_offset, &part.hyperlinks)
                })?)?;
            }
        }
        media_offset += part_images.len();
    }
    if ctx.parts.has(RelKind::Footer(EMPTY_FOOTER_SLOT)) {
        let footer = HeaderFooter {
            paragraphs: vec![Paragraph::new()],
        };
        zip.start_file(format!("word/footer{EMPTY_FOOTER_SLOT}.xml"), options)?;
        zip.write_all(&xml_part(|w| {
            generate_footer_xml(w, &footer, &doc.style, ctx)
        })?)?;
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
            BlockElement::BibliographyBlock { paragraphs }
            | BlockElement::TocBlock { paragraphs, .. } => {
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
