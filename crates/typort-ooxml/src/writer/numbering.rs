use std::io::{self, Write};

use quick_xml::Writer;

use super::package::write_value_element;
use crate::document::Document;

#[allow(
    clippy::too_many_lines,
    reason = "writes all numbering levels in schema order"
)]
pub(super) fn generate_numbering_xml(
    writer: &mut Writer<&mut Vec<u8>>,
    doc: &Document,
) -> io::Result<()> {
    writer
        .create_element("w:numbering")
        .with_attribute((
            "xmlns:w",
            "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
        ))
        .write_inner_content(|w| {
            // Abstract numbering 1: ordered list (decimal). Levels 0-8: the
            // body walk emits `w:ilvl` up to the nesting depth, and a level
            // with no definition here renders with no number (L1).
            w.create_element("w:abstractNum")
                .with_attribute(("w:abstractNumId", "1"))
                .write_inner_content(|abs| {
                    for ilvl in 0..=8 {
                        write_numbering_level(
                            abs,
                            &ilvl.to_string(),
                            "1",
                            "decimal",
                            &format!("%{}.", ilvl + 1),
                            "left",
                        )?;
                    }
                    Ok(())
                })?;
            // Abstract numbering 2: unordered list (bullet), levels 0-8.
            w.create_element("w:abstractNum")
                .with_attribute(("w:abstractNumId", "2"))
                .write_inner_content(|abs| {
                    for ilvl in 0..=8 {
                        write_numbering_level(
                            abs,
                            &ilvl.to_string(),
                            "1",
                            "bullet",
                            "\u{2022}",
                            "left",
                        )?;
                    }
                    Ok(())
                })?;
            // Numbering instance 1 -> abstractNum 1 (ordered)
            write_numbering_instance(w, "1", "1", None)?;
            // Numbering instance 2 -> abstractNum 2 (unordered)
            write_numbering_instance(w, "2", "2", None)?;
            // No abstractNum 3: the Chinese five-level heading numbering
            // definition was emitted unconditionally but never referenced by
            // any w:numPr — heading numbering is literal text, not linked
            // Word numbering (see #29). It returns if heading numbering is
            // ever linked to a numId.
            // Dynamic numbering instances for each top-level list
            for &(num_id, abstract_num_id, start) in &doc.list_num_instances {
                let num_id_str = num_id.to_string();
                let abs_id_str = abstract_num_id.to_string();
                let start_str = start.to_string();
                write_numbering_instance(
                    w,
                    num_id_str.as_str(),
                    abs_id_str.as_str(),
                    Some(start_str.as_str()),
                )?;
            }
            Ok(())
        })?;
    Ok(())
}

fn write_numbering_instance<W: Write>(
    writer: &mut Writer<W>,
    num_id: &str,
    abstract_num_id: &str,
    start: Option<&str>,
) -> io::Result<()> {
    writer
        .create_element("w:num")
        .with_attribute(("w:numId", num_id))
        .write_inner_content(|num| {
            write_value_element(num, "w:abstractNumId", abstract_num_id)?;
            if let Some(start) = start {
                // Each top-level list is an independent instance: override the
                // level-0 start so Word restarts at the list's own first number.
                num.create_element("w:lvlOverride")
                    .with_attribute(("w:ilvl", "0"))
                    .write_inner_content(|ovr| {
                        write_value_element(ovr, "w:startOverride", start)?;
                        Ok(())
                    })?;
            }
            Ok(())
        })?;
    Ok(())
}

/// Helper to write a single numbering level definition.
fn write_numbering_level(
    writer: &mut Writer<&mut Vec<u8>>,
    ilvl: &str,
    start: &str,
    num_fmt: &str,
    lvl_text: &str,
    jc: &str,
) -> io::Result<()> {
    writer
        .create_element("w:lvl")
        .with_attribute(("w:ilvl", ilvl))
        .write_inner_content(|lvl| {
            write_value_element(lvl, "w:start", start)?;
            write_value_element(lvl, "w:numFmt", num_fmt)?;
            write_value_element(lvl, "w:lvlText", lvl_text)?;
            write_value_element(lvl, "w:lvlJc", jc)?;
            Ok(())
        })?;
    Ok(())
}
