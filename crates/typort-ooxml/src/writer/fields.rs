use std::io::{self, Write};

use quick_xml::Writer;
use quick_xml::events::BytesText;

use super::run::write_text_run;
use crate::document::{DocumentStyle, Run};

fn truncate_bookmark_name(name: &str) -> &str {
    if name.len() <= 40 {
        return name;
    }
    let mut end = 40;
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    &name[..end]
}

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
    runs: &[Run],
    doc_style: &DocumentStyle,
) -> io::Result<()> {
    // Use w:fldSimple with HYPERLINK field code to avoid relationship management
    let instr = format!("HYPERLINK &quot;{url}&quot;");
    writer
        .create_element("w:fldSimple")
        .with_attribute(("w:instr", instr.as_str()))
        .write_inner_content(|w| {
            for run in runs {
                w.create_element("w:r").write_inner_content(|rw| {
                    // Apply hyperlink styling: colored + underlined
                    rw.create_element("w:rPr").write_inner_content(|rpr| {
                        rpr.create_element("w:color")
                            .with_attribute(("w:val", doc_style.hyperlink_color.as_str()))
                            .write_empty()?;
                        rpr.create_element("w:u")
                            .with_attribute(("w:val", "single"))
                            .write_empty()?;
                        if run.bold {
                            rpr.create_element("w:b").write_empty()?;
                        }
                        if run.italic {
                            rpr.create_element("w:i").write_empty()?;
                        }
                        Ok(())
                    })?;
                    rw.create_element("w:t")
                        .with_attribute(("xml:space", "preserve"))
                        .write_text_content(BytesText::new(&run.text))?;
                    Ok(())
                })?;
            }
            Ok(())
        })?;
    Ok(())
}

pub(super) fn write_toc_field<W: Write>(writer: &mut Writer<W>, max_depth: u8) -> io::Result<()> {
    let instr = format!(r#" TOC \o "1-{max_depth}" \h \z \u "#);
    write_complex_field(
        writer,
        &instr,
        "Right-click and update field to see table of contents.",
        false,
    )
}
