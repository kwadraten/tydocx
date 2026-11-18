//! Equivalence between the realize path (`convert`) and the legacy
//! tag-walker path (`convert_legacy`): where the legacy path is correct,
//! the new path's `word/document.xml` must be byte-identical. A fixture
//! moves from `IDENTICAL` to `DOCUMENTED` only with a ruling asserted in
//! `ir_path_documented_diffs`.

use crate::common;

const IDENTICAL: &[&str] = &[
    "hello",
    "style_default",
    "style_custom_page",
    "style_custom_spacing",
    "style_indent",
    "style_justify",
    "style_heading_custom",
    "issue_deep_headings",
    "nested_list",
    "enum_custom_start",
    "style_footnotes",
    "formatted_footnote",
    "image_test",
    "svg_test",
    "pagebreak_test",
    "style_columns",
    "header_footer_test",
    "page_numbering",
    "issue_margin_zero",
    "issue_odd_even_headers",
    "issue_first_page_header",
    "issue_c1_header_style",
];

const DOCUMENTED: &[&str] = &[
    "general_elements",
    "tech_doc",
    "business_report",
    "memo",
    "complex_paper",
    "nested_table_test",
    "mixed_table_borders",
    "two_ordered_lists",
    "bibliography_basic",
    "bibliography_multiple",
    "math_test",
    "aligned_equations",
    "numbered_eq",
    "edge_figure_rasterized",
    "toc_test",
    "columns_test",
    "grid_test",
    "edge_blockquote",
    "style_code_blocks",
    "terms_wide",
    "rowspan_test",
    "edge_complex_table",
    "footnote_in_table",
    "figure_caption",
    "issue_header_run_styles",
    "issue_margin_change_section",
    "issue_page_numbering_change",
    "issue_table_first_section",
    "issue_table_cascade_sections",
];

fn convert_both(fixture: &str) -> (String, String) {
    let world = typort_core::TyportWorld::new(&common::fixture_path(fixture)).unwrap();
    let options = typort_core::ConvertOptions::default();
    let new_doc = typort_core::convert(&world, &options).unwrap().document;
    let legacy_doc = typort_core::convert_legacy(&world, &options)
        .unwrap()
        .document;
    let new_xml = common::fixture_package_from_document(&new_doc)
        .part_text("word/document.xml")
        .to_string();
    let legacy_xml = common::fixture_package_from_document(&legacy_doc)
        .part_text("word/document.xml")
        .to_string();
    (new_xml, legacy_xml)
}

fn first_diff(new_xml: &str, legacy_xml: &str) -> String {
    let new_lines: Vec<&str> = new_xml.lines().collect();
    let legacy_lines: Vec<&str> = legacy_xml.lines().collect();
    for (index, (new_line, legacy_line)) in new_lines.iter().zip(legacy_lines.iter()).enumerate() {
        if new_line != legacy_line {
            return format!("line {index}:\n  new:    {new_line}\n  legacy: {legacy_line}");
        }
    }
    format!(
        "line counts differ: new {} lines, legacy {} lines",
        new_lines.len(),
        legacy_lines.len()
    )
}

#[test]
fn ir_path_matches_legacy_on_core_fixtures() {
    let mut mismatches = Vec::new();
    for fixture in IDENTICAL {
        let (new_xml, legacy_xml) = convert_both(fixture);
        if new_xml != legacy_xml {
            mismatches.push(format!("{fixture}: {}", first_diff(&new_xml, &legacy_xml)));
        }
    }
    assert!(
        mismatches.is_empty(),
        "realize path differs from legacy path on:\n{}",
        mismatches.join("\n")
    );
}

fn count(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

fn without_lines<'a>(xml: &'a str, needles: &[&str]) -> Vec<&'a str> {
    xml.lines()
        .filter(|line| !needles.iter().any(|needle| line.contains(needle)))
        .collect()
}

#[test]
fn ir_path_documented_diffs() {
    for fixture in ["aligned_equations", "numbered_eq"] {
        let (new_xml, legacy_xml) = convert_both(fixture);
        assert_eq!(
            count(&new_xml, "w:jc w:val=\"center\""),
            if fixture == "aligned_equations" { 3 } else { 2 },
            "{fixture}: new path should center every display equation"
        );
        assert_eq!(
            count(&legacy_xml, "w:jc w:val=\"center\""),
            0,
            "{fixture}: legacy path centers no display equation"
        );
        assert_eq!(
            without_lines(
                &new_xml,
                &["w:jc w:val=\"center\"", "w:ind w:firstLine=\"0\""]
            ),
            without_lines(&legacy_xml, &["w:ind w:firstLine=\"0\""]),
            "{fixture}: nothing else should differ"
        );
    }

    {
        let (new_xml, legacy_xml) = convert_both("math_test");
        assert_eq!(count(&new_xml, "w:jc w:val=\"center\""), 1);
        assert_eq!(count(&legacy_xml, "w:jc w:val=\"center\""), 0);
        assert!(
            new_xml.contains("m:chr m:val=\"∑\""),
            "sum equation present"
        );
        assert!(
            legacy_xml.contains("m:chr m:val=\"∑\""),
            "sum equation present"
        );
        assert!(
            new_xml.contains("Inline: "),
            "inline math paragraph present"
        );
    }

    {
        let (new_xml, legacy_xml) = convert_both("columns_test");
        assert_eq!(count(&new_xml, "w:val=\"18\""), 2);
        assert_eq!(count(&legacy_xml, "w:val=\"18\""), 0);
        assert_eq!(
            without_lines(&new_xml, &["w:val=\"18\""]),
            legacy_xml.lines().collect::<Vec<_>>(),
            "columns_test: nothing else should differ"
        );
    }

    {
        let (new_xml, legacy_xml) = convert_both("general_elements");
        assert!(legacy_xml.contains("w:bottom w:val=\"single\" w:sz=\"6\""));
        assert!(!new_xml.contains("w:pBdr"), "new path drops #line (M5 gap)");
        assert_eq!(count(&new_xml, "w:val=\"18\""), 6);
        assert_eq!(count(&legacy_xml, "w:val=\"18\""), 0);
        assert!(
            new_xml.contains("w:color w:val=\"D73948\""),
            "highlight color kept"
        );
        assert!(
            !legacy_xml.contains("w:color w:val=\"D73948\""),
            "legacy drops colors"
        );
        assert!(new_xml.contains(">Rust<"), "term present");
        assert!(
            new_xml.contains(">A systems programming language<"),
            "term description kept"
        );
        assert!(
            new_xml.contains("<w:spacing w:val="),
            "term gap emitted as tracked spacing"
        );
        assert!(legacy_xml.contains(">A systems programming language<"));
    }

    {
        let (new_xml, legacy_xml) = convert_both("tech_doc");
        assert_eq!(count(&legacy_xml, "w:val=\"20\""), 82);
        assert_eq!(count(&new_xml, "w:val=\"20\""), 0);
        assert_eq!(count(&legacy_xml, "DejaVu Sans Mono"), 6);
        assert_eq!(count(&new_xml, "DejaVu Sans Mono"), 0);
        assert!(
            new_xml.contains("Authorization: Bearer"),
            "code text present"
        );
        assert!(
            legacy_xml.contains("Authorization: Bearer"),
            "code text present"
        );
    }

    {
        let (new_xml, legacy_xml) = convert_both("business_report");
        assert!(new_xml.contains("“数字化转型不是选择题，而是生存题。” —— CEO 张明"));
        assert!(!legacy_xml.contains("“数字化转型不是选择题，而是生存题。” —— CEO 张明"));
        assert!(legacy_xml.contains(">数字化转型不是选择题，而是生存题。<"));
        assert!(legacy_xml.contains(">—— CEO 张明<"));
        assert!(
            new_xml.contains(">预计贡献营收 2000 万元<"),
            "term description kept"
        );
        assert!(legacy_xml.contains(">预计贡献营收 2000 万元<"));
    }

    {
        let (new_xml, legacy_xml) = convert_both("memo");
        assert!(legacy_xml.contains("w:bottom w:val=\"single\" w:sz=\"6\""));
        assert!(!new_xml.contains("w:pBdr"), "new path drops #line (M5 gap)");
        assert_eq!(count(&new_xml, "<w:tbl>"), 1);
        assert_eq!(count(&legacy_xml, "<w:tbl>"), 0);
        assert!(
            new_xml.contains("w:val=\"nil\""),
            "strokless grid maps to nil borders"
        );
    }

    {
        let (new_xml, legacy_xml) = convert_both("complex_paper");
        assert_eq!(count(&new_xml, "w:jc w:val=\"center\""), 5);
        assert_eq!(count(&legacy_xml, "w:jc w:val=\"center\""), 2);
        assert_eq!(count(&new_xml, "w:jc w:val=\"left\""), 20);
        assert_eq!(count(&legacy_xml, "w:jc w:val=\"left\""), 24);
        assert_eq!(count(&new_xml, "<w:b/>"), 15);
        assert_eq!(count(&legacy_xml, "<w:b/>"), 20);
        assert!(
            new_xml.contains(" 本文利用2012"),
            "space after 摘要 label kept"
        );
        assert!(legacy_xml.contains("本文利用2012"));
        assert!(
            new_xml.contains(" 数字经济；区域创新效率"),
            "space after 关键词 label kept"
        );
        assert!(
            new_xml.contains("[1] 赵涛"),
            "hand-written reference kept whole"
        );
        assert!(
            !legacy_xml.contains("[1] 赵涛"),
            "legacy splits the [1] marker"
        );
    }

    {
        let (new_xml, legacy_xml) = convert_both("mixed_table_borders");
        assert_eq!(count(&legacy_xml, "w:jc w:val=\"center\""), 1);
        assert_eq!(count(&new_xml, "w:jc w:val=\"center\""), 0);
        assert!(legacy_xml.contains("A borderless table\u{a0}1:"));
        assert!(!new_xml.contains("A borderless table\u{a0}1:"));
    }
    {
        let (new_xml, legacy_xml) = convert_both("nested_table_test");
        assert_eq!(count(&legacy_xml, "w:jc w:val=\"center\""), 1);
        assert_eq!(count(&new_xml, "w:jc w:val=\"center\""), 0);
        assert!(legacy_xml.contains("Inner AInner B"));
        assert!(!new_xml.contains("Inner AInner B"));
    }
    {
        let (new_xml, legacy_xml) = convert_both("two_ordered_lists");
        assert_eq!(count(&legacy_xml, "w:jc w:val=\"center\""), 1);
        assert_eq!(count(&new_xml, "w:jc w:val=\"center\""), 0);
        assert!(legacy_xml.contains("1.one2.two"));
        assert!(!new_xml.contains("1.one2.two"));
    }
    {
        let (new_xml, legacy_xml) = convert_both("toc_test");
        assert_eq!(count(&legacy_xml, "w:jc w:val=\"center\""), 5);
        assert_eq!(count(&new_xml, "w:jc w:val=\"center\""), 0);
        assert!(legacy_xml.contains(">Contents<"));
        assert!(!new_xml.contains(">Contents<"));
        assert!(new_xml.contains(" TOC "), "new path emits the TOC field");
    }

    for (fixture, anchor, author) in [
        ("bibliography_basic", "loc_4", "Smith"),
        ("bibliography_multiple", "loc_2", "Alpha"),
    ] {
        let (new_xml, legacy_xml) = convert_both(fixture);
        assert!(
            new_xml.contains("<w:bibliography/>"),
            "entries missing (known gap)"
        );
        assert!(legacy_xml.contains(&format!("w:anchor=\"{anchor}\"")));
        assert!(!new_xml.contains("w:hyperlink"), "no citation links yet");
        assert!(!new_xml.contains(author), "no entry text in the new path");
        assert!(legacy_xml.contains(author), "legacy renders entries");
    }

    {
        let (new_xml, legacy_xml) = convert_both("edge_figure_rasterized");
        assert_eq!(count(&legacy_xml, "wp:extent"), 1);
        assert_eq!(count(&new_xml, "wp:extent"), 0);
        assert!(legacy_xml.contains("1800000"), "rasterized extent present");
        assert!(!new_xml.contains("1800000"));
        assert!(
            !new_xml.contains("xmlns:wp="),
            "no drawing namespaces emitted"
        );
    }

    {
        let (new_xml, legacy_xml) = convert_both("grid_test");
        assert_eq!(count(&new_xml, "<w:tbl>"), 1);
        assert_eq!(count(&legacy_xml, "<w:tbl>"), 0);
        assert!(
            new_xml.contains("w:val=\"nil\""),
            "strokless grid maps to nil borders"
        );
    }

    {
        let (new_xml, legacy_xml) = convert_both("edge_blockquote");
        assert!(new_xml.contains("A second block quote with "));
        assert!(legacy_xml.contains("A second block quote with<"));
        assert!(!new_xml.contains("A second block quote with<"));
    }

    {
        let (new_xml, legacy_xml) = convert_both("terms_wide");
        assert!(new_xml.contains(">Alpha<"), "term present");
        assert!(new_xml.contains(">Beta<"), "term present");
        assert!(legacy_xml.contains(">Alpha<"));
        assert!(legacy_xml.contains(">Beta<"));
        assert!(new_xml.contains("First wide term description here."));
        assert!(legacy_xml.contains("First wide term description here."));
        assert_eq!(count(&legacy_xml, "w:left=\"0\""), 2);
        assert_eq!(count(&new_xml, "w:left=\"0\""), 0);
    }

    for fixture in [
        "rowspan_test",
        "edge_complex_table",
        "footnote_in_table",
        "figure_caption",
    ] {
        let (new_xml, legacy_xml) = convert_both(fixture);
        assert!(
            new_xml.contains("<w:tblCellMar>"),
            "{fixture}: realize path emits cell margins; legacy does not"
        );
        assert!(!legacy_xml.contains("<w:tblCellMar>"));
        assert_eq!(
            without_lines(
                &new_xml,
                &[
                    "w:tblCellMar",
                    "w:top w:w=",
                    "w:left w:w=",
                    "w:bottom w:w=",
                    "w:right w:w="
                ]
            ),
            without_lines(&legacy_xml, &[]),
            "{fixture}: cell margins are the only diff"
        );
    }

    {
        // M6: the IR path emits a mid-body `w:sectPr` per section with real
        // `w:pgMar`; the legacy path emits a bare page break and folds the
        // last section's margins into the single trailing sectPr.
        let (new_xml, legacy_xml) = convert_both("issue_margin_change_section");
        assert_eq!(count(&new_xml, "<w:sectPr"), 2);
        assert_eq!(count(&legacy_xml, "<w:sectPr"), 1);
        assert!(new_xml.contains(r#"w:left="2268""#));
        assert!(!legacy_xml.contains(r#"w:type="nextPage""#));
    }

    {
        // U30: the IR path emits per-section `w:pgNumType` with `w:start`
        // on a restart; the legacy path has neither the section boundary
        // nor the restart.
        let (new_xml, legacy_xml) = convert_both("issue_page_numbering_change");
        assert_eq!(count(&new_xml, "<w:sectPr"), 2);
        assert_eq!(count(&legacy_xml, "<w:sectPr"), 1);
        assert!(new_xml.contains(r#"<w:pgNumType w:fmt="decimal"/>"#));
        assert!(new_xml.contains(r#"w:start="1""#));
        assert!(!legacy_xml.contains("w:start"));
    }

    for fixture in ["issue_table_first_section", "issue_table_cascade_sections"] {
        // P1: a table-only first section still ends with a `w:sectPr` in the
        // IR path (empty marker paragraph); the legacy path drops the
        // boundary entirely. `w:tblCellMar` is the other known table diff.
        let (new_xml, legacy_xml) = convert_both(fixture);
        assert_eq!(count(&legacy_xml, "<w:sectPr"), 1);
        assert!(
            count(&new_xml, "<w:sectPr") > 1,
            "{fixture}: IR path keeps every section boundary"
        );
    }

    {
        // Legacy emits a stray rPr-only run in the body that the IR path
        // does not produce.
        let (new_xml, legacy_xml) = convert_both("issue_header_run_styles");
        assert!(legacy_xml.contains(r#"<w:sz w:val="22"/>"#));
        assert!(!new_xml.contains(r#"<w:sz w:val="22"/>"#));
    }

    {
        let (new_xml, legacy_xml) = convert_both("style_code_blocks");
        assert_eq!(count(&new_xml, "w:val=\"18\""), 2);
        assert_eq!(count(&legacy_xml, "w:val=\"18\""), 0);
        assert_eq!(count(&legacy_xml, "w:val=\"22\""), 8);
        assert_eq!(count(&new_xml, "w:val=\"22\""), 0);
        assert_eq!(count(&legacy_xml, "DejaVu Sans Mono"), 4);
        assert_eq!(count(&new_xml, "DejaVu Sans Mono"), 0);
        assert!(
            new_xml.contains("w:color w:val=\"D73948\""),
            "highlight color kept"
        );
        assert!(
            !legacy_xml.contains("w:color w:val=\"D73948\""),
            "legacy drops colors"
        );
        assert!(legacy_xml.contains("Courier New"));
    }
}

#[test]
fn ir_path_records_overline_degradation() {
    let outcome = {
        let world =
            typort_core::TyportWorld::new(&common::fixture_path("issue_ir_overline_degradation"))
                .unwrap();
        typort_core::convert(
            &world,
            &typort_core::ConvertOptions::new(typort_core::CompatMode::C1),
        )
        .unwrap()
    };
    let overline = outcome
        .degradations
        .iter()
        .find(|d| d.construct == "overline");
    assert!(
        overline.is_some(),
        "IR path should record an overline degradation, got: {:?}",
        outcome.degradations
    );
    let overline = overline.unwrap();
    assert!(
        overline.span.is_some(),
        "overline degradation must carry a span, got: {:?}",
        overline
    );
    assert!(
        matches!(overline.action, typort_core::DegradationAction::Skipped),
        "overline degradation must be Skipped, got: {:?}",
        overline
    );
    let xml = common::fixture_package_from_document(&outcome.document)
        .part_text("word/document.xml")
        .to_string();
    assert!(
        xml.contains("Overlined text"),
        "overline text must survive without the decoration"
    );
    assert!(!xml.contains("<w:u "), "no underline emitted under C1");
}
