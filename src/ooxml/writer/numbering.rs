use std::io::{self, Write};

use quick_xml::Writer;

use super::package::write_value_element;
use crate::ooxml::document::{Document, LevelOverride};

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
            for instance in &doc.heading_num_instances {
                let abs_id_str = instance.abstract_num_id.to_string();
                w.create_element("w:abstractNum")
                    .with_attribute(("w:abstractNumId", abs_id_str.as_str()))
                    .write_inner_content(|abs| {
                        for level in &instance.levels {
                            let ilvl_str = level.ilvl.to_string();
                            write_heading_level(
                                abs,
                                ilvl_str.as_str(),
                                "1",
                                level.num_fmt,
                                level.lvl_text.as_str(),
                            )?;
                        }
                        Ok(())
                    })?;
            }
            // Numbering instance 1 -> abstractNum 1 (ordered)
            write_numbering_instance(w, "1", "1", None, &[])?;
            // Numbering instance 2 -> abstractNum 2 (unordered)
            write_numbering_instance(w, "2", "2", None, &[])?;
            // Dynamic numbering instances for each top-level list
            for instance in &doc.list_num_instances {
                let num_id_str = instance.num_id.to_string();
                let abs_id_str = instance.abstract_num_id.to_string();
                let start_str = instance.start.to_string();
                write_numbering_instance(
                    w,
                    num_id_str.as_str(),
                    abs_id_str.as_str(),
                    Some(start_str.as_str()),
                    &instance.level_overrides,
                )?;
            }
            for instance in &doc.heading_num_instances {
                let num_id_str = instance.num_id.to_string();
                let abs_id_str = instance.abstract_num_id.to_string();
                write_numbering_instance(w, num_id_str.as_str(), abs_id_str.as_str(), None, &[])?;
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
    level_overrides: &[LevelOverride],
) -> io::Result<()> {
    writer
        .create_element("w:num")
        .with_attribute(("w:numId", num_id))
        .write_inner_content(|num| {
            write_value_element(num, "w:abstractNumId", abstract_num_id)?;
            // The level-0 pattern override (if any) shares its w:lvlOverride
            // with the startOverride: ECMA-376 intends one override per
            // level, and a reader taking the first ilvl-0 element would
            // silently drop the pattern if the two were split.
            let level0 = level_overrides.iter().find(|o| o.ilvl == 0);
            if let Some(start) = start {
                // Each top-level list is an independent instance: override the
                // level-0 start so Word restarts at the list's own first number.
                num.create_element("w:lvlOverride")
                    .with_attribute(("w:ilvl", "0"))
                    .write_inner_content(|ovr| {
                        write_value_element(ovr, "w:startOverride", start)?;
                        if let Some(def) = level0 {
                            write_numbering_level(
                                ovr,
                                "0",
                                "1",
                                def.num_fmt,
                                def.lvl_text.as_str(),
                                "left",
                            )?;
                        }
                        Ok(())
                    })?;
            }
            // Per-level overrides (e.g. an enum's non-decimal numbering
            // pattern): each carries a full w:lvl so Word renders that
            // level's numbers in the author's format. The ilvl-0 entry is
            // skipped here when it was merged into the start element above.
            for ovr in level_overrides {
                if ovr.ilvl == 0 && start.is_some() {
                    continue;
                }
                let ilvl_str = ovr.ilvl.to_string();
                num.create_element("w:lvlOverride")
                    .with_attribute(("w:ilvl", ilvl_str.as_str()))
                    .write_inner_content(|lvl_ovr| {
                        write_numbering_level(
                            lvl_ovr,
                            ilvl_str.as_str(),
                            "1",
                            ovr.num_fmt,
                            ovr.lvl_text.as_str(),
                            "left",
                        )
                    })?;
            }
            Ok(())
        })?;
    Ok(())
}

/// A heading numbering level: like [`write_numbering_level`] but with
/// `w:suff="space"` — Typst separates the number from the body with a space
/// (a weak `h(0.3em)`), not Word's default tab.
fn write_heading_level<W: Write>(
    writer: &mut Writer<W>,
    ilvl: &str,
    start: &str,
    num_fmt: &str,
    lvl_text: &str,
) -> io::Result<()> {
    writer
        .create_element("w:lvl")
        .with_attribute(("w:ilvl", ilvl))
        .write_inner_content(|lvl| {
            write_value_element(lvl, "w:start", start)?;
            write_value_element(lvl, "w:numFmt", num_fmt)?;
            write_value_element(lvl, "w:suff", "space")?;
            write_value_element(lvl, "w:lvlText", lvl_text)?;
            write_value_element(lvl, "w:lvlJc", "left")?;
            Ok(())
        })?;
    Ok(())
}

/// Helper to write a single numbering level definition.
fn write_numbering_level<W: Write>(
    writer: &mut Writer<W>,
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
