use std::io::{self, Write};

use quick_xml::Writer;
use quick_xml::events::BytesText;

use super::WriteCtx;
use super::run::{write_inline, write_run, write_text_run};
use crate::document::{DocumentStyle, InlineElement, Run};

use crate::document::truncate_bookmark_name;

pub(super) fn write_bookmark_start<W: Write>(
    writer: &mut Writer<W>,
    id: u32,
    name: &str,
) -> io::Result<()> {
    let id_str = id.to_string();
    let name = truncate_bookmark_name(name);
    writer
        .create_element("w:bookmarkStart")
        .with_attribute(("w:id", id_str.as_str()))
        .with_attribute(("w:name", name))
        .write_empty()?;
    Ok(())
}

pub(super) fn write_bookmark_end<W: Write>(writer: &mut Writer<W>, id: u32) -> io::Result<()> {
    let id_str = id.to_string();
    writer
        .create_element("w:bookmarkEnd")
        .with_attribute(("w:id", id_str.as_str()))
        .write_empty()?;
    Ok(())
}

/// Write a `<w:r><w:fldChar w:fldCharType="..."/></w:r>` element.
pub(super) fn write_fld_char<W: Write>(writer: &mut Writer<W>, char_type: &str) -> io::Result<()> {
    writer.create_element("w:r").write_inner_content(|w| {
        w.create_element("w:fldChar")
            .with_attribute(("w:fldCharType", char_type))
            .write_empty()?;
        Ok(())
    })?;
    Ok(())
}

pub(super) fn write_instruction_text_run<W: Write>(
    writer: &mut Writer<W>,
    text: &str,
) -> io::Result<()> {
    writer.create_element("w:r").write_inner_content(|w| {
        w.create_element("w:instrText")
            .with_attribute(("xml:space", "preserve"))
            .write_text_content(BytesText::new(text))?;
        Ok(())
    })?;
    Ok(())
}

pub(super) fn write_complex_field<W: Write>(
    writer: &mut Writer<W>,
    instruction: &str,
    cached_result: &str,
    preserve_cached_space: bool,
) -> io::Result<()> {
    write_fld_char(writer, "begin")?;
    write_instruction_text_run(writer, instruction)?;
    write_fld_char(writer, "separate")?;
    write_text_run(writer, cached_result, preserve_cached_space)?;
    write_fld_char(writer, "end")
}

/// Write a complex field whose field runs carry `bold`/`size_half_pt` styling —
/// used when the cached result was extracted from styled Typst content (a bold
/// or resized page-number token in a header/footer).
pub(super) fn write_complex_field_styled<W: Write>(
    writer: &mut Writer<W>,
    instruction: &str,
    cached_result: &str,
    bold: bool,
    size_half_pt: Option<u32>,
) -> io::Result<()> {
    write_fld_char_styled(writer, "begin", bold, size_half_pt)?;
    write_instruction_text_run_styled(writer, instruction, bold, size_half_pt)?;
    write_fld_char_styled(writer, "separate", bold, size_half_pt)?;
    writer.create_element("w:r").write_inner_content(|w| {
        write_field_rpr(w, bold, size_half_pt)?;
        w.create_element("w:t")
            .with_attribute(("xml:space", "preserve"))
            .write_text_content(BytesText::new(cached_result))?;
        Ok(())
    })?;
    write_fld_char_styled(writer, "end", bold, size_half_pt)
}

fn write_field_rpr<W: Write>(
    w: &mut Writer<W>,
    bold: bool,
    size_half_pt: Option<u32>,
) -> io::Result<()> {
    w.create_element("w:rPr").write_inner_content(|rpr| {
        if bold {
            rpr.create_element("w:b").write_empty()?;
        }
        if let Some(size) = size_half_pt {
            super::package::write_size_pair(rpr, &size.to_string())?;
        }
        Ok(())
    })?;
    Ok(())
}

fn write_fld_char_styled<W: Write>(
    writer: &mut Writer<W>,
    char_type: &str,
    bold: bool,
    size_half_pt: Option<u32>,
) -> io::Result<()> {
    writer.create_element("w:r").write_inner_content(|w| {
        write_field_rpr(w, bold, size_half_pt)?;
        w.create_element("w:fldChar")
            .with_attribute(("w:fldCharType", char_type))
            .write_empty()?;
        Ok(())
    })?;
    Ok(())
}

fn write_instruction_text_run_styled<W: Write>(
    writer: &mut Writer<W>,
    text: &str,
    bold: bool,
    size_half_pt: Option<u32>,
) -> io::Result<()> {
    writer.create_element("w:r").write_inner_content(|w| {
        write_field_rpr(w, bold, size_half_pt)?;
        w.create_element("w:instrText")
            .with_attribute(("xml:space", "preserve"))
            .write_text_content(BytesText::new(text))?;
        Ok(())
    })?;
    Ok(())
}

pub(super) fn write_field_ref<W: Write>(
    writer: &mut Writer<W>,
    bookmark_name: &str,
    display_text: &str,
) -> io::Result<()> {
    let bookmark_name = truncate_bookmark_name(bookmark_name);
    let instr = format!(" REF {bookmark_name} \\h ");
    write_complex_field(writer, &instr, display_text, true)
}

pub(super) fn write_field_pageref<W: Write>(
    writer: &mut Writer<W>,
    bookmark_name: &str,
    display_text: &str,
) -> io::Result<()> {
    let bookmark_name = truncate_bookmark_name(bookmark_name);
    let instr = format!(" PAGEREF {bookmark_name} \\h ");
    write_complex_field(writer, &instr, display_text, true)
}

/// Write a current-page field (` PAGE `) with the cached display text
/// (the page number Typst rendered at this spot).
pub(super) fn write_field_page<W: Write>(
    writer: &mut Writer<W>,
    display_text: &str,
    bold: bool,
    size_half_pt: Option<u32>,
) -> io::Result<()> {
    if !bold && size_half_pt.is_none() {
        return write_complex_field(writer, " PAGE ", display_text, true);
    }
    write_complex_field_styled(writer, " PAGE ", display_text, bold, size_half_pt)
}

pub(super) fn write_hyperlink<W: Write>(
    writer: &mut Writer<W>,
    url: &str,
    body: &[InlineElement],
    ctx: &WriteCtx,
) -> io::Result<()> {
    // Use w:fldSimple with HYPERLINK field code to avoid relationship management
    let instr = format!("HYPERLINK &quot;{url}&quot;");
    writer
        .create_element("w:fldSimple")
        .with_attribute(("w:instr", instr.as_str()))
        .write_inner_content(|w| {
            for inline in body {
                if let InlineElement::Text(run) = inline {
                    write_hyperlink_run(w, run, ctx.doc_style)?;
                } else {
                    // Non-text children (drawing, OMML, tab, field) are valid
                    // run-level content inside the field result; they keep
                    // their own appearance rather than hyperlink styling.
                    write_inline(w, inline, ctx)?;
                }
            }
            Ok(())
        })?;
    Ok(())
}

/// One text run inside a HYPERLINK field result: the run keeps all of its own
/// properties (fonts, size, tracking, …) with hyperlink color + underline on
/// top.
fn write_hyperlink_run<W: Write>(
    writer: &mut Writer<W>,
    run: &Run,
    doc_style: &DocumentStyle,
) -> io::Result<()> {
    let mut styled = run.clone();
    styled.color = Some(doc_style.hyperlink_color.clone());
    styled.underline = true;
    write_run(writer, &styled, doc_style)
}

/// Write a generic complex field: `instruction` into `w:instrText`, `result`
/// runs (with their own styling) as the cached result. Used by the C3 field
/// layer where the cached result is the literal content the field replaced.
pub(super) fn write_field<W: Write>(
    writer: &mut Writer<W>,
    instruction: &str,
    result: &[Run],
    doc_style: &DocumentStyle,
) -> io::Result<()> {
    write_fld_char(writer, "begin")?;
    write_instruction_text_run(writer, instruction)?;
    write_fld_char(writer, "separate")?;
    for run in result {
        write_run(writer, run, doc_style)?;
    }
    write_fld_char(writer, "end")
}

/// The `TOC` instruction: `\o "1-N"` lists headings by outline level; a table
/// of figures uses `\c "Id"` to list captions of one `SEQ` identifier instead.
fn toc_instruction(max_depth: u8, caption_id: Option<&str>) -> String {
    caption_id.map_or_else(
        || format!(r#" TOC \o "1-{max_depth}" \h \z \u "#),
        |id| format!(r#" TOC \h \z \c "{id}" "#),
    )
}

pub(super) fn write_toc_field<W: Write>(
    writer: &mut Writer<W>,
    max_depth: u8,
    caption_id: Option<&str>,
) -> io::Result<()> {
    let instr = toc_instruction(max_depth, caption_id);
    write_complex_field(
        writer,
        &instr,
        "Right-click and update field to see table of contents.",
        false,
    )
}

/// A TOC field whose cached result is one paragraph per collected entry —
/// the same multi-paragraph complex-field shape Word itself writes. `instr`
/// spans the first entry's paragraph; the `end` char gets its own paragraph.
pub(super) fn write_toc_block<W: Write>(
    writer: &mut Writer<W>,
    max_depth: u8,
    caption_id: Option<&str>,
    paragraphs: &[crate::document::Paragraph],
    ctx: &WriteCtx,
) -> io::Result<()> {
    let instr = toc_instruction(max_depth, caption_id);
    if paragraphs.is_empty() {
        return write_toc_field(writer, max_depth, caption_id);
    }
    writer.create_element("w:p").write_inner_content(|pw| {
        write_fld_char(pw, "begin")?;
        write_instruction_text_run(pw, &instr)?;
        write_fld_char(pw, "separate")?;
        for inline in &paragraphs[0].inlines {
            write_inline(pw, inline, ctx)?;
        }
        Ok(())
    })?;
    for para in &paragraphs[1..] {
        super::paragraph::write_paragraph(writer, para, ctx)?;
    }
    writer.create_element("w:p").write_inner_content(|pw| {
        write_fld_char(pw, "end")?;
        Ok(())
    })?;
    Ok(())
}
