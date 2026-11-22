//! Timetable-style `grid` elements lower to genuine Word tables with their
//! geometry pinned: fixed layout + `w:tblGrid` lane widths, exact row
//! heights, header repetition, merged spans (`w:gridSpan` / `w:vMerge`),
//! per-cell borders and fills, and 1pt paragraph marks inside empty slots
//! so compressed rows keep their declared height.

use crate::common::{fixture_doc_xml, fixture_document, fixture_package};
use tydocx_ooxml::document::{BlockElement, CellContent};

fn count(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

#[test]
fn timetable_grid_emits_fixed_layout_table() {
    let xml = fixture_doc_xml("grid_timetable");
    assert!(xml.contains(r#"<w:tblLayout w:type="fixed"/>"#));
    assert_eq!(
        count(&xml, "<w:gridCol "),
        6,
        "one lane per timetable column"
    );
    assert!(
        xml.contains(r#"<w:tblW w:w="7935" w:type="dxa"/>"#),
        "table width fills the content area"
    );
    assert_eq!(
        count(&xml, r#"<w:trHeight w:val="397" w:hRule="exact"/>"#),
        1,
        "header row keeps its declared 0.7cm"
    );
    assert_eq!(
        count(&xml, "<w:tr>"),
        11,
        "phantom slot rows merge into bands between cell edges and hlines"
    );
    for (height, times) in [(170, 6), (340, 2), (510, 2)] {
        assert_eq!(
            count(
                &xml,
                &format!(r#"<w:trHeight w:val="{height}" w:hRule="exact"/>"#)
            ),
            times,
            "merged bands sum their source rows' 0.3cm heights"
        );
    }
    assert_eq!(
        count(&xml, "<w:tc>"),
        61,
        "gridSpan cells and their vMerge continuations each emit one <w:tc>"
    );
}

#[test]
fn timetable_grid_spans_fills_and_lines() {
    let xml = fixture_doc_xml("grid_timetable");
    assert!(
        xml.contains("<w:tblHeader/>"),
        "grid.header repeats on new pages"
    );
    assert_eq!(
        count(&xml, r#"<w:gridSpan w:val="2"/>"#),
        5,
        "the two colspan=2 events and their three span continuations keep the span"
    );
    assert_eq!(
        count(&xml, r#"<w:vMerge w:val="restart"/>"#),
        6,
        "labels and events that span merged bands restart a vertical merge"
    );
    assert_eq!(
        count(&xml, "<w:vMerge/>"),
        10,
        "span continuations cover the merged bands below each restart"
    );
    for fill in ["B8D4F0", "C8E6C9", "FFE0B2", "F8BBD0"] {
        assert!(
            xml.contains(&format!(r#"w:fill="{fill}""#)),
            "event fill {fill} is preserved"
        );
    }
    assert!(
        xml.contains(r#"<w:top w:val="single" w:sz="6""#),
        "the hour-separator hlines surface as cell borders"
    );
}

#[test]
fn timetable_empty_slots_use_tiny_paragraphs() {
    let xml = fixture_doc_xml("grid_timetable");
    assert!(
        xml.contains(r#"w:line="20" w:lineRule="exact""#),
        "empty slots get a 1pt exact line height so rows don't inflate"
    );
    assert!(
        xml.contains(r#"<w:sz w:val="2"/>"#),
        "empty slots get a 1pt paragraph mark"
    );
    assert!(
        xml.contains(r#"<w:tcMar>"#),
        "empty slots zero their cell margins"
    );
}

#[test]
fn context_wrapped_grid_still_emits_table() {
    let xml = fixture_doc_xml("grid_context_rect");
    assert!(
        xml.contains("<w:tbl>"),
        "a grid inside rect inside context must survive collection"
    );
    assert_eq!(
        count(&xml, r#"<w:gridSpan w:val="2"/>"#),
        1,
        "the context-produced colspan keeps its span"
    );
    assert!(
        xml.contains(r#"<w:vMerge"#),
        "the context-produced rowspan keeps its merge"
    );
    for text in ["Slot", "A1", "A2", "B2", "B3", "C2", "D3"] {
        assert!(
            xml.contains(&format!(">{text}</w:t>")),
            "context cell text {text:?} survives"
        );
    }
}

#[test]
fn timetable_cell_content_stays_editable() {
    let doc = fixture_document("grid_timetable");
    let mut texts = Vec::new();
    for el in &doc.body.elements {
        if let BlockElement::Table(table) = el {
            for row in &table.rows {
                for cell in &row.cells {
                    for item in &cell.content {
                        if let CellContent::Paragraph(p) = item {
                            for inline in &p.inlines {
                                if let tydocx_ooxml::document::InlineElement::Text(run) = inline {
                                    texts.push(run.text.clone());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    for needle in [
        "Time",
        "Mon",
        "Fri",
        "08:00",
        "11:00",
        "Algebra",
        "Physics",
        "Lab session",
        "PE",
    ] {
        assert!(
            texts.iter().any(|t| t == needle),
            "cell text {needle:?} survives as editable text, got: {texts:?}"
        );
    }
    let package = fixture_package("grid_timetable");
    assert!(
        package
            .part_names()
            .all(|name| !name.starts_with("word/media/")),
        "a timetable grid embeds no raster images"
    );
}

#[test]
fn timetable_auto_row_keeps_measured_height() {
    let xml = fixture_doc_xml("grid_timetable_auto");
    assert_eq!(
        count(&xml, r#"<w:trHeight w:val="290" w:hRule="exact"/>"#),
        1,
        "the auto title row keeps its own measured height"
    );
    for height in [672, 1344, 2016] {
        assert!(
            xml.contains(&format!(
                r#"<w:trHeight w:val="{height}" w:hRule="exact"/>"#
            )),
            "fr slot bands share the remaining height ({height} twips)"
        );
    }
}

#[test]
fn timetable_dashed_cell_stroke_stays_dashed() {
    let xml = fixture_doc_xml("grid_timetable_auto");
    assert_eq!(
        count(
            &xml,
            r#"<w:top w:val="dashed" w:sz="4" w:space="0" w:color="AAAAAA"/>"#
        ),
        1,
        "the hour cell's declared dashed top stroke survives as a dashed border"
    );
}

#[test]
fn timetable_borders_emit_on_owning_side_only() {
    let xml = fixture_doc_xml("grid_timetable_auto");
    assert_eq!(
        count(&xml, r#"<w:left w:val="single" w:sz="4""#),
        3,
        "the lane divider emits only on Beta's left edge (restart + continuations)"
    );
    assert_eq!(
        count(&xml, r#"<w:right w:val="single" w:sz="4""#),
        1,
        "empty and continue cells never acquire the neighbour's border"
    );
    assert_eq!(
        count(&xml, r#"<w:right w:val="nil"/>"#),
        16,
        "undeclared slot cells emit nil borders, not phantom strokes"
    );
}

#[test]
fn timetable_gridspan_vmerge_continue_stays_one_cell() {
    let xml = fixture_doc_xml("grid_timetable");
    let spanning_continues = xml
        .split("<w:tc>")
        .skip(1)
        .filter(|tc| {
            let end = tc.find("</w:tc>").unwrap_or(tc.len());
            let body = &tc[..end];
            body.contains(r#"<w:gridSpan w:val="2"/>"#) && body.contains("<w:vMerge/>")
        })
        .count();
    assert_eq!(
        spanning_continues, 3,
        "a vMerge continuation of a gridSpan restart keeps the span as one cell"
    );
}

#[test]
fn timetable_cell_runs_keep_declared_text_size() {
    let xml = fixture_doc_xml("grid_timetable_auto");
    assert_eq!(
        count(&xml, r#"<w:sz w:val="12"/>"#),
        4,
        "`show grid.cell: set text(size: 6pt)` reaches the cell-body runs"
    );
    assert!(
        xml.contains(r#"<w:sz w:val="32"/>"#),
        "an inline size override inside a cell body is preserved too"
    );
}
