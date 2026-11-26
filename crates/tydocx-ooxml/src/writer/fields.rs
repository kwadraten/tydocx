use std::io::{self, Write};

use quick_xml::Writer;
use quick_xml::events::BytesText;

use super::WriteCtx;
use super::paragraph::write_toc_paragraph;
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
    write_pageref(writer, bookmark_name, display_text, true)
}

fn write_pageref<W: Write>(
    writer: &mut Writer<W>,
    bookmark_name: &str,
    display_text: &str,
    hyperlink: bool,
) -> io::Result<()> {
    let bookmark_name = truncate_bookmark_name(bookmark_name);
    let switch = if hyperlink { " \\h" } else { "" };
    let instr = format!(" PAGEREF {bookmark_name}{switch} ");
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

pub(super) fn write_toc_inline<W: Write>(
    writer: &mut Writer<W>,
    inline: &InlineElement,
    ctx: &WriteCtx,
) -> io::Result<()> {
    let InlineElement::InternalLink { anchor, body } = inline else {
        return write_inline(writer, inline, ctx);
    };
    writer
        .create_element("w:hyperlink")
        .with_attribute(("w:anchor", anchor.as_str()))
        .with_attribute(("w:history", "1"))
        .write_inner_content(|w| {
            for inline in body {
                if let InlineElement::FieldPageRef {
                    bookmark_name,
                    display_text,
                } = inline
                {
                    write_pageref(w, bookmark_name, display_text, false)?;
                } else {
                    write_inline(w, inline, ctx)?;
                }
            }
            Ok(())
        })?;
    Ok(())
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
    let Some((first, rest)) = paragraphs.split_first() else {
        return write_toc_field(writer, max_depth, caption_id);
    };
    write_toc_paragraph(writer, first, ctx, Some(&instr))?;
    for para in rest {
        write_toc_paragraph(writer, para, ctx, None)?;
    }
    writer.create_element("w:p").write_inner_content(|pw| {
        write_fld_char(pw, "end")?;
        Ok(())
    })?;
    Ok(())
}
