use std::io::{self, Write};

use quick_xml::Writer;

use super::fields::{write_bookmark_end, write_bookmark_start, write_complex_field};
use super::run::write_text_run;
use crate::document::EquationNumber;

pub(super) fn write_math_inline<W: Write>(
    writer: &mut Writer<W>,
    omml_xml: &str,
) -> io::Result<()> {
    // Write the pre-serialized OMML XML directly into the stream
    writer.get_mut().write_all(omml_xml.as_bytes())?;
    Ok(())
}

/// Extract the inner `<m:oMath>…</m:oMath>` from a block `<m:oMathPara>` wrapper so
/// a numbered equation can be written as inline math sitting between tab stops —
/// the structure Word itself uses for numbered equations. A block `<m:oMathPara>`
/// is a standalone centered paragraph and does not coexist with the trailing tab +
/// number on the same line. Returns the input unchanged if it is already inline.
pub(super) fn strip_math_para(omml: &str) -> &str {
    match (omml.find("<m:oMath>"), omml.rfind("</m:oMath>")) {
        (Some(start), Some(end)) => &omml[start..end + "</m:oMath>".len()],
        _ => omml,
    }
}

/// Write a right-aligned equation number after an OMML block equation.
///
/// This uses a right-aligned tab stop to position the number at the right margin,
/// mimicking the standard Chinese journal equation numbering style.
pub(super) fn write_equation_number<W: Write>(
    writer: &mut Writer<W>,
    number: &EquationNumber,
) -> io::Result<()> {
    // Emit a run with a tab character followed by the equation number
    writer.create_element("w:r").write_inner_content(|w| {
        w.create_element("w:tab").write_empty()?;
        Ok(())
    })?;
    match number {
        EquationNumber::Text(text) => write_text_run(writer, text, true),
        EquationNumber::Seq {
            prefix,
            instruction,
            display,
            suffix,
        } => {
            if !prefix.is_empty() {
                write_text_run(writer, prefix, true)?;
            }
            write_complex_field(writer, instruction, display, true)?;
            if !suffix.is_empty() {
                write_text_run(writer, suffix, true)?;
            }
            Ok(())
        }
        EquationNumber::Bookmarked {
            prefix,
            display,
            instruction,
            suffix,
            bookmarks,
        } => {
            if !prefix.is_empty() {
                write_text_run(writer, prefix, true)?;
            }
            for (id, name) in bookmarks {
                write_bookmark_start(writer, *id, name)?;
            }
            if let Some(instruction) = instruction {
                write_complex_field(writer, instruction, display, true)?;
            } else {
                write_text_run(writer, display, true)?;
            }
            for (id, _) in bookmarks.iter().rev() {
                write_bookmark_end(writer, *id)?;
            }
            if !suffix.is_empty() {
                write_text_run(writer, suffix, true)?;
            }
            Ok(())
        }
    }
}
