use std::io::{self, Write};

use quick_xml::Writer;

use super::WriteCtx;
use super::fields::{write_bookmark_end, write_bookmark_start};
use super::header_footer::PartCtx;
use super::package::write_value_element;
use super::run::{write_inline, write_text_run};
use crate::document::{
    Alignment, AnchoredContent, BlockElement, CellContent, Document, Footnote, FootnoteEntryStyle,
    FootnoteFormat, ImageData, InlineElement, Paragraph, Table,
};

/// Write `w:footnotePr` element with optional circled-number format and
/// per-page restart. `restart_each_page` mirrors Typst: the footnote counter
/// restarts on every page only when the source resets it in a page header
/// (`counter(footnote).update(0)` there); Typst's default is continuous
/// numbering, so `w:numRestart` is omitted then (Word's own default is
/// continuous too).
pub(super) fn write_footnote_pr<W: Write>(
    writer: &mut Writer<W>,
    format: &FootnoteFormat,
    restart_each_page: bool,
) -> io::Result<()> {
    writer
        .create_element("w:footnotePr")
        .write_inner_content(|fp| {
            if *format == FootnoteFormat::CircledNumber {
                fp.create_element("w:numFmt")
                    .with_attribute(("w:val", "decimalEnclosedCircle"))
                    .write_empty()?;
            }
            if restart_each_page {
                fp.create_element("w:numRestart")
                    .with_attribute(("w:val", "eachPage"))
                    .write_empty()?;
            }
            Ok(())
        })?;
    Ok(())
}
fn write_footnote_mark_run<W: Write>(writer: &mut Writer<W>) -> io::Result<()> {
    // Word generates the marker from the footnote format, so the run holds
    // only `w:footnoteRef` — never a literal marker character, which would
    // freeze the global sequence and contradict a per-page restart.
    writer.create_element("w:r").write_inner_content(|w| {
        w.create_element("w:rPr").write_inner_content(|rpr| {
            write_value_element(rpr, "w:rStyle", "FootnoteReference")?;
            Ok(())
        })?;
        w.create_element("w:footnoteRef").write_empty()?;
        Ok(())
    })?;
    Ok(())
}

pub(super) fn write_footnote_ref<W: Write>(writer: &mut Writer<W>, id: u32) -> io::Result<()> {
    let id_str = id.to_string();
    writer.create_element("w:r").write_inner_content(|w| {
        w.create_element("w:rPr").write_inner_content(|rpr| {
            rpr.create_element("w:rStyle")
                .with_attribute(("w:val", "FootnoteReference"))
                .write_empty()?;
            Ok(())
        })?;
        w.create_element("w:footnoteReference")
            .with_attribute(("w:id", id_str.as_str()))
            .write_empty()?;
        Ok(())
    })?;
    Ok(())
}

/// Write `word/footnotes.xml`.
///
/// `footnote_images` is the document-ordered list of every image inside a
/// footnote (top-level or nested in a link/float) — the same list
/// [`generate_part_image_rels`] enumerates for `word/_rels/footnotes.xml.rels`.
/// The part writes through a part-local [`PartCtx`] so every image reference,
/// wherever it nests, gets a footnote-local `rId`; the media files keep the
/// document-wide image numbering.
pub(super) fn generate_footnotes_xml(
    writer: &mut Writer<&mut Vec<u8>>,
    doc: &Document,
    ctx: &WriteCtx,
    footnote_images: &[&ImageData],
    has_drawings: bool,
    has_math: bool,
) -> io::Result<()> {
    let mut elem = writer
        .create_element("w:footnotes")
        .with_attribute((
            "xmlns:w",
            "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
        ))
        .with_attribute((
            "xmlns:r",
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
        ));
    if has_math {
        elem = elem.with_attribute((
            "xmlns:m",
            "http://schemas.openxmlformats.org/officeDocument/2006/math",
        ));
    }
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
        // Separator footnotes (id 0 and 1 are reserved by OOXML spec)
        write_separator_footnote(w, "0", "separator", &ctx.doc_style.footnote_entry)?;
        write_separator_footnote(
            w,
            "1",
            "continuationSeparator",
            &FootnoteEntryStyle::default(),
        )?;

        // Actual footnotes. The part-local context enumerates footnote images
        // globally across footnotes (rId{k+1}), so a later footnote's drawing
        // cannot embed the first image again.
        let part = PartCtx::new(footnote_images.to_vec());
        let part_ctx = part.ctx(ctx);
        for footnote in &doc.footnotes {
            write_footnote_entry(w, footnote, &part_ctx)?;
        }
        Ok(())
    })?;
    Ok(())
}

/// One `w:footnote` element: the style properties, the Word-generated mark
/// (wrapped in the NOTEREF bookmark when the footnote was labelled), and the
/// content inlines written through the footnote part-local context — image
/// relationships resolve against `word/_rels/footnotes.xml.rels`, not
/// `document.xml.rels`, including images nested inside links.
fn write_footnote_entry(
    w: &mut Writer<&mut Vec<u8>>,
    footnote: &Footnote,
    ctx: &WriteCtx,
) -> io::Result<()> {
    let id_str = footnote.id.to_string();
    w.create_element("w:footnote")
        .with_attribute(("w:id", id_str.as_str()))
        .write_inner_content(|fn_w| {
            let mut first_para = true;
            for para in &footnote.paragraphs {
                fn_w.create_element("w:p").write_inner_content(|p_w| {
                    // Paragraph properties with footnote text style
                    let entry = &ctx.doc_style.footnote_entry;
                    p_w.create_element("w:pPr").write_inner_content(|ppr| {
                        ppr.create_element("w:pStyle")
                            .with_attribute(("w:val", "FootnoteText"))
                            .write_empty()?;
                        if let Some(gap) = entry.gap_twips {
                            let gap = gap.to_string();
                            ppr.create_element("w:spacing")
                                .with_attribute(("w:after", gap.as_str()))
                                .write_empty()?;
                        }
                        if let Some(indent) = entry.indent_twips {
                            let indent = indent.to_string();
                            ppr.create_element("w:ind")
                                .with_attribute(("w:firstLine", indent.as_str()))
                                .write_empty()?;
                        }
                        let jc = para
                            .alignment
                            .map(Alignment::ooxml_value)
                            .or_else(|| para.justify.map(|j| if j { "both" } else { "left" }))
                            .or_else(|| entry.justify.map(|j| if j { "both" } else { "left" }));
                        if let Some(jc) = jc {
                            ppr.create_element("w:jc")
                                .with_attribute(("w:val", jc))
                                .write_empty()?;
                        }
                        Ok(())
                    })?;
                    // Word generates the marker from the footnote format;
                    // never emit a literal marker character here. A
                    // bookmark around the mark is the NOTEREF target.
                    if first_para {
                        if let Some(bookmark) = &footnote.bookmark {
                            write_bookmark_start(p_w, bookmark.id, &bookmark.name)?;
                        }
                        write_footnote_mark_run(p_w)?;
                        if let Some(bookmark) = &footnote.bookmark {
                            write_bookmark_end(p_w, bookmark.id)?;
                        }
                        // Space after reference mark
                        write_text_run(p_w, " ", true)?;
                        first_para = false;
                    }
                    for inline in &para.inlines {
                        write_inline(p_w, inline, ctx)?;
                    }
                    Ok(())
                })?;
            }
            Ok(())
        })?;
    Ok(())
}

/// Whether any footnote holds OMML math; gates the `xmlns:m` declaration on
/// `w:footnotes` — an undeclared `m:` prefix makes the part malformed XML.
pub(super) fn footnotes_have_math(doc: &Document) -> bool {
    doc.footnotes.iter().any(|f| {
        f.paragraphs
            .iter()
            .any(|p| p.inlines.iter().any(inline_has_math))
    })
}

fn inline_has_math(inline: &InlineElement) -> bool {
    match inline {
        InlineElement::Math { .. } => true,
        InlineElement::AnchoredFloat(float) => match &float.content {
            AnchoredContent::Text { blocks, .. } => blocks.iter().any(block_has_math),
            AnchoredContent::Image(_) => false,
        },
        InlineElement::Hyperlink { body, .. } | InlineElement::InternalLink { body, .. } => {
            body.iter().any(inline_has_math)
        }
        _ => false,
    }
}

fn para_has_math(para: &Paragraph) -> bool {
    para.inlines.iter().any(inline_has_math)
}

fn table_has_math(table: &Table) -> bool {
    table.rows.iter().any(|row| {
        row.cells.iter().any(|cell| {
            if cell.content.is_empty() {
                cell.paragraphs.iter().any(para_has_math)
            } else {
                cell.content.iter().any(|item| match item {
                    CellContent::Paragraph(p) => para_has_math(p),
                    CellContent::Table(t) => table_has_math(t),
                })
            }
        })
    })
}

fn block_has_math(block: &BlockElement) -> bool {
    match block {
        BlockElement::Paragraph(p) => para_has_math(p),
        BlockElement::Table(t) => table_has_math(t),
        BlockElement::BibliographyBlock { paragraphs }
        | BlockElement::TocBlock { paragraphs, .. } => paragraphs.iter().any(para_has_math),
    }
}

fn write_separator_footnote<W: Write>(
    writer: &mut Writer<W>,
    id: &str,
    sep_type: &str,
    entry: &FootnoteEntryStyle,
) -> io::Result<()> {
    writer
        .create_element("w:footnote")
        .with_attribute(("w:type", sep_type))
        .with_attribute(("w:id", id))
        .write_inner_content(|w| {
            w.create_element("w:p").write_inner_content(|p_w| {
                let custom_stroke = entry.separator_stroke_eighths.is_some();
                if custom_stroke || entry.clearance_twips.is_some() {
                    p_w.create_element("w:pPr").write_inner_content(|ppr| {
                        if let Some(clearance) = entry.clearance_twips {
                            let clearance = clearance.to_string();
                            ppr.create_element("w:spacing")
                                .with_attribute(("w:before", clearance.as_str()))
                                .write_empty()?;
                        }
                        // A declared `line(...)` separator replaces Word's default
                        // rule with a paragraph border carrying the stroke width.
                        if let Some(eighths) = entry.separator_stroke_eighths {
                            let sz = eighths.to_string();
                            ppr.create_element("w:pBdr").write_inner_content(|bdr| {
                                bdr.create_element("w:bottom")
                                    .with_attribute(("w:val", "single"))
                                    .with_attribute(("w:sz", sz.as_str()))
                                    .with_attribute(("w:space", "1"))
                                    .with_attribute(("w:color", "auto"))
                                    .write_empty()?;
                                Ok(())
                            })?;
                        }
                        Ok(())
                    })?;
                }
                if !custom_stroke {
                    p_w.create_element("w:r").write_inner_content(|r_w| {
                        r_w.create_element("w:separator").write_empty()?;
                        Ok(())
                    })?;
                }
                Ok(())
            })?;
            Ok(())
        })?;
    Ok(())
}
