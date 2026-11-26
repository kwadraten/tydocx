//! Heading semantics and formatting tests.

use crate::common::{
    fixture_doc_xml, fixture_outcome, fixture_part, fixture_styles_xml, paragraph_containing,
};
use tydocx_core::{CompatMode, ConvertOptions};

#[test]
fn issue_show_rule_heading_styles() {
    let xml = fixture_doc_xml("issue_show_rule_heading");
    assert!(
        xml.contains("Main Title"),
        "heading 1 text should be present"
    );
    assert!(
        xml.contains("Blue Subtitle"),
        "heading 2 text should be present"
    );
    // Heading size now lives in the Heading1 style, not on the run.
    let styles = fixture_styles_xml("issue_show_rule_heading");
    assert!(
        styles.contains("w:val=\"36\"") || styles.contains("w:val=\"35\""),
        "Heading1 style should define ~18pt size (36 half-pts). Got:\n{styles}"
    );
    assert!(
        xml.contains("Heading1"),
        "heading 1 should use Heading1 style"
    );
}

#[test]
fn issue_linebreak_in_heading_preserved() {
    let xml = fixture_doc_xml("issue_linebreak_heading");
    assert!(
        xml.contains("Heading with"),
        "heading text before line break should be present"
    );
    assert!(
        xml.contains("line break"),
        "heading text after line break should be present"
    );
    assert!(xml.contains("Heading1"), "should have Heading1 style");
}

#[test]
fn issue_heading_numbering_correct_order() {
    let xml = fixture_doc_xml("issue_heading_numbering_show");
    let intro_pos = xml.find("Introduction").expect("Introduction should exist");
    let bg_pos = xml.find("Background").expect("Background should exist");
    let methods_pos = xml.find("Methods").expect("Methods should exist");
    assert!(
        intro_pos < bg_pos && bg_pos < methods_pos,
        "headings should appear in order: Introduction < Background < Methods"
    );
}

#[test]
fn edge_show_rule_heading_counter_text() {
    let xml = fixture_doc_xml("edge_show_rule_heading_counter");
    for text in [
        "Introduction",
        "Background",
        "Motivation",
        "Methods",
        "Data Collection",
        "Results",
    ] {
        assert!(xml.contains(text), "heading '{text}' should be present");
    }
}

#[test]
fn issue_deep_headings_all_levels() {
    let xml = fixture_doc_xml("issue_deep_headings");
    for text in ["Level 1", "Level 2", "Level 3", "Level 4", "Level 5"] {
        assert!(xml.contains(text), "heading '{text}' should be present");
    }
    assert!(xml.contains("Content under"), "body text should be present");
    for style in ["Heading1", "Heading2", "Heading3", "Heading4", "Heading5"] {
        assert!(xml.contains(style), "style '{style}' should be present");
    }
}

#[test]
fn issue_function_generated_heading() {
    let xml = fixture_doc_xml("issue_function_generated_content");
    assert!(
        xml.contains("Introduction"),
        "first heading should be present"
    );
    assert!(
        xml.contains("Important Result"),
        "function-generated heading should be present"
    );
    assert!(
        xml.contains("Heading1"),
        "level-1 heading style should be present"
    );
    assert!(
        xml.contains("Heading2"),
        "function-generated heading should have Heading2 style"
    );
}

#[test]
fn issue_cjk_heading_numbering_content() {
    let xml = fixture_doc_xml("issue_cjk_heading_numbering");
    assert!(xml.contains("绪论"), "CJK heading text should be present");
    assert!(xml.contains("研究背景"), "CJK subheading should be present");
    assert!(xml.contains("Heading1"), "Heading1 style should be present");
    // The synthesized heading number ("一、") must be emitted in the heading
    // paragraph itself (not merely recovered elsewhere in the document).
    let heading_para = paragraph_containing(&xml, "绪论");
    assert!(
        heading_para.contains("一、"),
        "heading numbering '一、' should be part of the heading paragraph"
    );
}

#[test]
fn heading_smart_quotes_are_resolved() {
    // Regression: smart quotes in a heading were dropped (only body text kept them).
    // See the `edge_heading_smartquotes` fixture.
    let doc_xml = fixture_doc_xml("edge_heading_smartquotes");
    let h1 = paragraph_containing(&doc_xml, "投资于人");
    // Curly opening/closing double quotes (U+201C / U+201D) around the phrase.
    assert!(
        h1.contains('\u{201C}') && h1.contains('\u{201D}'),
        "heading should keep its curly quotes around 投资于人"
    );
    let h2 = paragraph_containing(&doc_xml, "quoted");
    assert!(
        h2.contains('\u{201C}') && h2.contains('\u{201D}'),
        "English quoted heading should keep its curly quotes"
    );
}

#[test]
fn issue_heading_numbering_native_uses_word_numbering() {
    // L2: a static "1.1" pattern is representable — heading paragraphs carry
    // w:numPr pointing at a real abstract numbering definition, and the
    // numbers are not baked into the text.
    let outcome = fixture_outcome("issue_heading_numbering_native");
    assert!(
        outcome.degradations.is_empty(),
        "mappable numbering should report nothing: {:?}",
        outcome.degradations
    );
    let xml = fixture_doc_xml("issue_heading_numbering_native");
    let first = paragraph_containing(&xml, "First One");
    assert!(
        first.contains("<w:numPr>"),
        "numbered heading should carry w:numPr: {first}"
    );
    assert!(
        first.contains(r#"<w:ilvl w:val="1"/>"#),
        "level-2 heading uses ilvl 1: {first}"
    );
    assert!(
        !first.contains("1.1"),
        "the number is Word's, not literal text: {first}"
    );
    let second = paragraph_containing(&xml, "Second");
    assert!(
        second.contains(r#"<w:ilvl w:val="0"/>"#),
        "level-1 heading uses ilvl 0: {second}"
    );

    let numbering = fixture_part("issue_heading_numbering_native", "word/numbering.xml");
    assert!(
        numbering.contains(r#"w:numFmt w:val="decimal""#),
        "the abstract definition uses decimal: {numbering}"
    );
    assert!(
        numbering.contains(r#"w:lvlText w:val="%1.%2""#),
        "level 2 inherits the level-1 counter like Typst's 1.1: {numbering}"
    );
    assert!(
        numbering.contains(r#"w:suff w:val="space""#),
        "the number/body separator is a space, not Word's default tab: {numbering}"
    );
    assert!(
        !numbering.contains("chineseCounting"),
        "no unmappable formats are emitted: {numbering}"
    );

    // C1 keeps the rendered number as literal text instead.
    let c1 = fixture_package_xml_with("issue_heading_numbering_native", CompatMode::C1);
    assert!(
        !c1.contains("<w:numPr>"),
        "C1 does not link Word numbering: {c1}"
    );
    assert!(
        c1.contains(">1.1 <") || c1.contains("1.1 "),
        "C1 keeps the literal number: {c1}"
    );
}

fn fixture_package_xml_with(fixture: &str, mode: CompatMode) -> String {
    crate::common::fixture_package_with_options(fixture, &ConvertOptions::new(mode))
        .part_text("word/document.xml")
        .to_owned()
}

#[test]
fn issue_heading_numbering_func_keeps_literal() {
    // L2: function numbering has no w:numFmt mapping — the number Typst
    // rendered stays literal and the fallback is reported, never dropped.
    let outcome = fixture_outcome("issue_heading_numbering_func");
    let constructs: Vec<&str> = outcome.degradations.iter().map(|d| d.construct).collect();
    assert!(
        constructs.contains(&"heading-numbering"),
        "function numbering should report heading-numbering: {constructs:?}"
    );
    let xml = fixture_doc_xml("issue_heading_numbering_func");
    let alpha = paragraph_containing(&xml, "Alpha");
    assert!(
        alpha.contains("Sec.1"),
        "the rendered number stays in the heading text: {alpha}"
    );
    assert!(
        !alpha.contains("<w:numPr>"),
        "no Word numbering for a function: {alpha}"
    );
    let beta = paragraph_containing(&xml, "Beta");
    assert!(
        beta.contains("Sec.2"),
        "second number is literal too: {beta}"
    );
}

#[test]
fn issue_cjk_heading_numbering_reports_and_stays_literal() {
    // L2: "一、" has no exact w:numFmt — its numbers stay literal and degrade.
    let outcome = fixture_outcome("issue_cjk_heading_numbering");
    let constructs: Vec<&str> = outcome.degradations.iter().map(|d| d.construct).collect();
    assert!(
        constructs.contains(&"heading-numbering"),
        "Chinese numbering should report heading-numbering: {constructs:?}"
    );
    let xml = fixture_doc_xml("issue_cjk_heading_numbering");
    let para = paragraph_containing(&xml, "绪论");
    assert!(
        para.contains("一、"),
        "the CJK number stays literal: {para}"
    );
    assert!(
        !para.contains("<w:numPr>"),
        "no Word numbering for an unrepresentable system: {para}"
    );
}

#[test]
fn issue_mixed_heading_numbering_maps_per_heading() {
    // L2: numbering is decided per heading — switching from a mappable
    // system to an unmappable one mid-document numbers the first group and
    // literalizes + reports the last.
    let outcome = fixture_outcome("issue_heading_numbering_mixed");
    let reports = outcome
        .degradations
        .iter()
        .filter(|d| d.construct == "heading-numbering")
        .count();
    assert_eq!(
        reports, 1,
        "exactly one literalized report for the unmappable heading: {:?}",
        outcome.degradations
    );
    let xml = fixture_doc_xml("issue_heading_numbering_mixed");
    let second = paragraph_containing(&xml, "Second");
    assert!(
        second.contains(r#"<w:ilvl w:val="0"/>"#),
        "the mappable heading uses Word numbering: {second}"
    );
    let second_one = paragraph_containing(&xml, "Second One");
    assert!(
        second_one.contains(r#"<w:ilvl w:val="1"/>"#),
        "level 2 also maps: {second_one}"
    );
    assert!(
        !second_one.contains("1.1"),
        "no literal number baked in: {second_one}"
    );
    let appendix = paragraph_containing(&xml, "附录");
    assert!(
        appendix.contains("二、"),
        "the unmappable number stays literal: {appendix}"
    );
    assert!(
        !appendix.contains("<w:numPr>"),
        "no Word numbering on the unmappable heading: {appendix}"
    );
}

#[test]
fn issue_cjk_heading_number_spacing_headings() {
    let xml = fixture_doc_xml("issue_cjk_heading_number_spacing");
    assert!(
        xml.matches("Heading").count() >= 3,
        "should have multiple heading styles"
    );
}

#[test]
fn issue_show_rule_removes_heading_semantics_text() {
    let xml = fixture_doc_xml("issue_show_rule_removes_heading_semantics");
    assert!(
        xml.matches("Heading").count() >= 3,
        "headings should be present despite show rules"
    );
}

#[test]
fn edge_super_sub_in_heading_preserved() {
    let doc_xml = fixture_doc_xml("edge_super_sub_in_heading");

    assert!(
        doc_xml.contains("w:vertAlign w:val=\"subscript\""),
        "heading with H₂O should have subscript vertAlign"
    );
    assert!(
        doc_xml.contains("w:vertAlign w:val=\"superscript\""),
        "heading with x² should have superscript vertAlign"
    );
}

#[test]
fn issue_chinese_heading_numbering_not_emitted_when_unreferenced() {
    // #29: the Chinese five-level abstract numbering (abstractNum 3) was
    // emitted unconditionally whenever numbering.xml was written, but no
    // w:numPr ever references numId 3 — heading numbering is literal text,
    // not linked Word numbering. Both a Chinese paper and a plain nested
    // list must come out without it, while the real list definitions stay.
    for fixture in ["complex_paper", "edge_deep_nested_list"] {
        let num_xml = fixture_part(fixture, "word/numbering.xml");
        assert!(
            !num_xml.contains("chineseCountingThousand"),
            "{fixture}: numbering.xml must not contain chineseCountingThousand"
        );
        assert!(
            !num_xml.contains("decimalEnclosedCircleChinese"),
            "{fixture}: numbering.xml must not contain decimalEnclosedCircleChinese"
        );
        assert!(
            !num_xml.contains("w:abstractNumId=\"3\""),
            "{fixture}: numbering.xml must not have abstractNumId 3"
        );
        assert!(
            !num_xml.contains("w:numId=\"3\""),
            "{fixture}: numbering.xml must not have numId 3 instance"
        );
        assert!(
            num_xml.contains("w:abstractNumId=\"1\"") && num_xml.contains("w:abstractNumId=\"2\""),
            "{fixture}: ordered/bullet abstract numbering must stay"
        );
    }
}

#[test]
fn show_rule_heading_centered() {
    let doc_xml = fixture_doc_xml("centered_heading");

    // The heading should be detected as centered from the PagedDocument
    // Look for a Heading1 paragraph with center alignment
    assert!(
        doc_xml.contains(r#"<w:jc w:val="center"/>"#),
        "centered heading should have w:jc center. Got:\n{doc_xml}"
    );
}

#[test]
fn large_title_not_split_by_tabs() {
    let doc_xml = fixture_doc_xml("large_title_test");

    // The large centered title "大标题测试文档" should NOT be split by tab characters.
    // Previously, large CJK characters at 22pt exceeded the x-cluster gap threshold,
    // causing each character to be treated as a separate column.
    assert!(
        !doc_xml.contains("<w:tab/>"),
        "large title should not contain tab separators. Got:\n{doc_xml}"
    );

    // The title characters should all be present
    assert!(
        doc_xml.contains('大') && doc_xml.contains('标') && doc_xml.contains('文'),
        "title characters should be in the output"
    );
}

#[test]
fn show_rule_heading_font_and_size() {
    let doc_xml = fixture_doc_xml("show_rule_styles");

    // Heading should be centered (from show rule: align(center))
    assert!(
        doc_xml.contains(r#"<w:jc w:val="center"/>"#),
        "heading should be centered via show rule. Got:\n{doc_xml}"
    );

    // Heading font should be overridden to DejaVu Sans (from show rule)
    assert!(
        doc_xml.contains("DejaVu Sans"),
        "heading should use DejaVu Sans font from show rule. Got:\n{doc_xml}"
    );

    // Heading size 18pt = 36 half-points now lives in the Heading1 STYLE (the
    // run inherits it), not as a redundant per-run <w:sz>.
    let styles_xml = fixture_styles_xml("show_rule_styles");
    assert!(
        styles_xml.contains(r#"<w:sz w:val="36"/>"#),
        "Heading1 style should define size 36 half-points (18pt). Got:\n{styles_xml}"
    );
}

#[test]
fn heading_run_props_not_redundant_with_style() {
    // A plain heading run must NOT repeat the Heading style's bold/size: the
    // pStyle already carries them, and duplicating them fights a Word template.
    // A genuinely-distinct inline span (italic) inside a heading must survive.
    // See the `heading_redundant_run_props` fixture.
    let doc_xml = fixture_doc_xml("heading_redundant_run_props");

    // Isolate the plain heading paragraph; its run should carry no redundant
    // <w:b/>/<w:sz> (those live in the Heading1 style).
    let plain = doc_xml
        .split("<w:p>")
        .find(|p| p.contains("Plain Heading One"))
        .expect("plain heading paragraph present");
    assert!(
        plain.contains(r#"<w:pStyle w:val="Heading1"/>"#),
        "plain heading should carry Heading1 pStyle. Got:\n{plain}"
    );
    assert!(
        !plain.contains("<w:b/>"),
        "plain heading run must not repeat the style's <w:b/>. Got:\n{plain}"
    );
    assert!(
        !plain.contains("<w:sz "),
        "plain heading run must not repeat the style's <w:sz>. Got:\n{plain}"
    );

    // The italic span inside the second heading must keep its distinct override.
    let styled = doc_xml
        .split("<w:p>")
        .find(|p| p.contains("Italic"))
        .expect("styled heading paragraph present");
    assert!(
        styled.contains("<w:i/>"),
        "italic span inside a heading must keep <w:i/>. Got:\n{styled}"
    );
}

/// The `<w:style ...>...</w:style>` block for `Heading{level}` in styles.xml.
fn heading_style<'a>(styles_xml: &'a str, level: u8) -> &'a str {
    let open = format!(r#"<w:style w:type="paragraph" w:styleId="Heading{level}">"#);
    let after_open = styles_xml
        .split(&open)
        .nth(1)
        .unwrap_or_else(|| panic!("Heading{level} style missing"));
    after_open
        .split("</w:style>")
        .next()
        .expect("Heading style closes")
}

#[test]
fn run_size_is_omitted_only_when_the_paragraph_inherits_it() {
    let fixture = "run_size_inheritance";
    let styles = fixture_styles_xml(fixture);
    assert!(heading_style(&styles, 1).contains(r#"<w:sz w:val="36"/>"#));
    let normal = styles
        .split_once(r#"w:styleId="Normal""#)
        .unwrap()
        .1
        .split("</w:style>")
        .next()
        .unwrap();
    assert!(normal.contains(r#"<w:sz w:val="22"/>"#));

    let xml = fixture_doc_xml(fixture);
    let run_of = |text: &str| {
        xml.split("<w:r>")
            .filter_map(|tail| tail.split_once("</w:r>").map(|(run, _)| run))
            .find(|run| run.contains(text))
            .unwrap_or_else(|| panic!("missing run for {text}"))
    };
    assert!(!run_of("Ordinary document text").contains("<w:sz "));
    assert!(run_of("Declaration size override").contains(r#"<w:sz w:val="30"/>"#));
    assert!(!run_of("linked title").contains("<w:sz "));
    // Eleven points matches Normal, not Heading1. Removing this override
    // based on the document default would silently enlarge the span.
    assert!(run_of("smaller title").contains(r#"<w:sz w:val="22"/>"#));
}

#[test]
fn issue_heading_size_equals_body() {
    // An H2 explicitly set to 10pt, below the 11pt body size (Typst 0.15.1
    // default): the Heading2 style must carry 20 half-points — the size
    // Typst renders — not the body+7 fallback the size histogram used when
    // no size larger than the body existed for that level. A heading smaller
    // than the body is the case the histogram could never detect at all.
    let styles_xml = fixture_styles_xml("issue_heading_size_equals_body");
    let heading2 = heading_style(&styles_xml, 2);
    assert!(
        heading2.contains(r#"<w:sz w:val="20"/>"#),
        "Heading2 style should define size 20 half-points (10pt). Got:\n{heading2}"
    );
}

#[test]
fn issue_headings_only_sizes() {
    // Headings only, no body text, at Typst defaults (11pt body): H1 15.5pt,
    // H2 13pt, and H3–H5 all 11pt — the body size itself. The Heading styles
    // must carry the rendered sizes (31 / 26 / 22 / 22 / 22 half-points), not
    // the body+offset fallbacks the size histogram used for the levels whose
    // size does not exceed the body (27 / 25 / 23).
    let styles_xml = fixture_styles_xml("issue_headings_only_sizes");
    for (level, expected) in [(1, 31), (2, 26), (3, 22), (4, 22), (5, 22)] {
        let style = heading_style(&styles_xml, level);
        assert!(
            style.contains(&format!(r#"<w:sz w:val="{expected}"/>"#)),
            "Heading{level} style should define size {expected} half-points. Got:\n{style}"
        );
    }
}

#[test]
fn issue_headings_only_h2_style_matches_rendered_size() {
    // aligned_equations has headings and equations but no body text: the H2
    // renders at 13pt, which is also the detected body size. The Heading2
    // style must carry 26 half-points, not the body+7 fallback (33).
    let styles_xml = fixture_styles_xml("aligned_equations");
    let heading2 = heading_style(&styles_xml, 2);
    assert!(
        heading2.contains(r#"<w:sz w:val="26"/>"#),
        "Heading2 style should define size 26 half-points (13pt). Got:\n{heading2}"
    );
}

#[test]
fn issue_pageref_field_for_page_counter_reference() {
    // A `counter(page).at(<label>)` reference (M8) must become a PAGEREF
    // field to the labelled heading's bookmark, with the rendered page number
    // as the cached display text — the same shape as the existing REF field.
    let xml = fixture_doc_xml("issue_pageref");
    assert!(
        xml.contains(" PAGEREF intro \\h "),
        "page reference should emit a PAGEREF field:\n{xml}"
    );
    assert!(
        xml.contains("w:name=\"intro\""),
        "the target heading should carry the bookmark the field points at:\n{xml}"
    );
    assert!(
        xml.contains("<w:t xml:space=\"preserve\">1</w:t>"),
        "the field's cached display text should be the page number:\n{xml}"
    );
}

#[test]
fn issue_bare_page_counter_display_becomes_page_field() {
    // A bare `counter(page).display()` (and `.at(here())`) must become a PAGE
    // field with the rendered page number as the cached display text — not
    // literal text. The arithmetic reference is the one that stays literal.
    let xml = fixture_doc_xml("issue_pageref_display");
    assert_eq!(
        xml.matches(" PAGE ").count(),
        2,
        "both bare current-page references should emit a PAGE field:\n{xml}"
    );
    assert!(
        xml.contains("One more than the introduction page:"),
        "the arithmetic reference's text should survive:\n{xml}"
    );
}

#[test]
fn issue_uncomputable_page_counters_report_degradations() {
    let outcome = fixture_outcome("issue_pageref_display");
    let constructs: Vec<&str> = outcome.degradations.iter().map(|d| d.construct).collect();
    assert!(
        constructs.contains(&"page-counter-value"),
        "arithmetic around counter(page) should be reported, got: {constructs:?}"
    );
    assert!(
        constructs.contains(&"footnote-counter-reference"),
        "counter(footnote) should be reported, got: {constructs:?}"
    );
}
