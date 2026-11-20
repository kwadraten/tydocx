//! Section, header/footer, and page-furniture tests (manuscript cases
//! T-01/M7, T-02/U13, T-06/M6, T-07/U31, T-08/U30, T-22/L3).

use crate::common::{fixture_outcome_with_options, fixture_package, fixture_package_with_options};
use typort_core::{CompatMode, ConvertOptions};

fn c1(fixture: &str) -> typort_core::ConvertOutcome {
    fixture_outcome_with_options(fixture, &ConvertOptions::new(CompatMode::C1))
}

fn constructs(outcome: &typort_core::ConvertOutcome) -> Vec<String> {
    outcome
        .degradations
        .iter()
        .map(|d| d.construct.to_string())
        .collect()
}

fn assert_has(outcome: &typort_core::ConvertOutcome, construct: &str) {
    let constructs = constructs(outcome);
    assert!(
        constructs.iter().any(|c| c == construct),
        "expected a {construct:?} degradation, got {constructs:?}"
    );
}

#[test]
fn issue_odd_even_headers_c1_reports_variant() {
    let outcome = c1("issue_odd_even_headers");
    assert_has(&outcome, "header-footer-variant");
    let package = fixture_package_with_options(
        "issue_odd_even_headers",
        &ConvertOptions::new(CompatMode::C1),
    );
    let header = package.part_text("word/header1.xml");
    assert!(
        header.contains("Odd Title"),
        "the first page's header text is kept, got {header:?}"
    );
}

#[test]
fn issue_odd_even_headers_c2_emits_even_and_default_refs() {
    let package = fixture_package_with_options(
        "issue_odd_even_headers",
        &ConvertOptions::new(CompatMode::C2),
    );
    let settings = package.part_text("word/settings.xml");
    assert!(
        settings.contains("<w:evenAndOddHeaders/>"),
        "w:evenAndOddHeaders is emitted, got {settings:?}"
    );
    let document = package.part_text("word/document.xml");
    let header_refs: Vec<&str> = document
        .split('<')
        .filter(|s| s.starts_with("w:headerReference"))
        .collect();
    assert!(
        header_refs
            .iter()
            .any(|r| r.contains(r#"w:type="default""#)),
        "a default headerReference is emitted, got {document:?}"
    );
    assert!(
        header_refs.iter().any(|r| r.contains(r#"w:type="even""#)),
        "an even headerReference is emitted, got {document:?}"
    );
    let even_header = package.part_text("word/header2.xml");
    assert!(
        even_header.contains("Even Title"),
        "the even-page header part carries the even content, got {even_header:?}"
    );
    let outcome = fixture_outcome_with_options(
        "issue_odd_even_headers",
        &ConvertOptions::new(CompatMode::C2),
    );
    assert!(
        !constructs(&outcome)
            .iter()
            .any(|c| c == "header-footer-variant"),
        "odd/even headers are representable; no variant degradation"
    );
}

#[test]
fn issue_first_page_header_c2_emits_title_pg() {
    let package = fixture_package_with_options(
        "issue_first_page_header",
        &ConvertOptions::new(CompatMode::C2),
    );
    let document = package.part_text("word/document.xml");
    assert!(
        document.contains("<w:titlePg/>"),
        "title-page header suppression maps to w:titlePg, got {document:?}"
    );
    assert!(
        document.contains(r#"<w:headerReference w:type="default""#),
        "the default header part is still referenced, got {document:?}"
    );
    assert!(
        !document.contains(r#"<w:headerReference w:type="first""#),
        "no first-page header part is fabricated for a suppressed header, got {document:?}"
    );
    let header = package.part_text("word/header1.xml");
    assert!(
        header.contains("Doc Title"),
        "the header part is kept (not dropped with page 1), got {header:?}"
    );
}

#[test]
fn issue_page_doi_foreground_c1_reports_and_skips_body() {
    let outcome = c1("issue_page_doi_foreground");
    assert_has(&outcome, "page-foreground");
    let package = fixture_package_with_options(
        "issue_page_doi_foreground",
        &ConvertOptions::new(CompatMode::C1),
    );
    let body = package.part_text("word/document.xml");
    assert!(
        !body.contains("DOI-PLACEHOLDER"),
        "foreground decoration must not enter the reading order"
    );
    assert!(
        package
            .part_names()
            .any(|name| package.part_text(name).contains("DOI-PLACEHOLDER")),
        "the foreground text survives in a page-furniture part"
    );
}

#[test]
fn issue_page_foreground_place_c1_reports() {
    let outcome = c1("issue_page_foreground_place");
    assert_has(&outcome, "page-foreground");
    let package = fixture_package_with_options(
        "issue_page_foreground_place",
        &ConvertOptions::new(CompatMode::C1),
    );
    let body = package.part_text("word/document.xml");
    assert!(
        !body.contains("SPECTROSCOPY MARK"),
        "foreground decoration must not enter the reading order"
    );
}

#[test]
fn issue_margin_change_section_c1_keeps_one_section() {
    let outcome = c1("issue_margin_change_section");
    assert_has(&outcome, "section-break");
    let package = fixture_package_with_options(
        "issue_margin_change_section",
        &ConvertOptions::new(CompatMode::C1),
    );
    let xml = package.part_text("word/document.xml");
    assert_eq!(
        xml.matches("<w:sectPr").count(),
        1,
        "C1 keeps a single section"
    );
}

#[test]
fn issue_margin_change_section_c2_emits_per_section_margins() {
    let package = fixture_package_with_options(
        "issue_margin_change_section",
        &ConvertOptions::new(CompatMode::C2),
    );
    let xml = package.part_text("word/document.xml");
    assert_eq!(
        xml.matches("<w:sectPr").count(),
        2,
        "one w:sectPr per Typst section, got {xml:?}"
    );
    // Section 1 (40mm on every side) ends at the mid-body sectPr; the final
    // sectPr carries section 2's left/right = 20mm (top/bottom stay at the
    // Typst default, 2.5/21 * 595.28pt ≈ 1417 twips).
    let pg_mars: Vec<&str> = xml
        .split('<')
        .filter(|s| s.starts_with("w:pgMar"))
        .collect();
    assert_eq!(pg_mars.len(), 2, "each sectPr carries its own w:pgMar");
    assert!(
        pg_mars[0].contains(r#"w:top="2268""#)
            && pg_mars[0].contains(r#"w:left="2268""#)
            && pg_mars[0].contains(r#"w:right="2268""#)
            && pg_mars[0].contains(r#"w:bottom="2268""#),
        "section 1 keeps the declared 40mm margin, got {:?}",
        pg_mars[0]
    );
    assert!(
        pg_mars[1].contains(r#"w:left="1134""#) && pg_mars[1].contains(r#"w:right="1134""#),
        "section 2 keeps the declared 20mm side margins, got {:?}",
        pg_mars[1]
    );
    let outcome = fixture_outcome_with_options(
        "issue_margin_change_section",
        &ConvertOptions::new(CompatMode::C2),
    );
    assert!(
        !constructs(&outcome).iter().any(|c| c == "section-break"),
        "C2 realizes the section break; no section-break degradation"
    );
}

#[test]
fn issue_table_first_section_c2_keeps_boundary() {
    let package = fixture_package_with_options(
        "issue_table_first_section",
        &ConvertOptions::new(CompatMode::C2),
    );
    let xml = package.part_text("word/document.xml");
    assert_eq!(
        xml.matches("<w:sectPr").count(),
        2,
        "a table-only first section still ends with its own w:sectPr, got {xml:?}"
    );
    let pg_mars: Vec<&str> = xml
        .split('<')
        .filter(|s| s.starts_with("w:pgMar"))
        .collect();
    assert!(
        pg_mars[0].contains(r#"w:left="2268""#) && pg_mars[0].contains(r#"w:right="2268""#),
        "section 1 keeps its declared 40mm margin, got {:?}",
        pg_mars[0]
    );
    assert!(
        pg_mars[1].contains(r#"w:left="1134""#) && pg_mars[1].contains(r#"w:right="1134""#),
        "section 2 keeps its declared 20mm side margins, got {:?}",
        pg_mars[1]
    );
}

#[test]
fn issue_table_cascade_sections_c2_keeps_all_boundaries() {
    let package = fixture_package_with_options(
        "issue_table_cascade_sections",
        &ConvertOptions::new(CompatMode::C2),
    );
    let xml = package.part_text("word/document.xml");
    assert_eq!(
        xml.matches("<w:sectPr").count(),
        3,
        "a table-first document keeps every later section too, got {xml:?}"
    );
    let pg_mars: Vec<&str> = xml
        .split('<')
        .filter(|s| s.starts_with("w:pgMar"))
        .collect();
    for (i, left) in ["2268", "1134", "567"].iter().enumerate() {
        assert!(
            pg_mars[i].contains(&format!(r#"w:left="{left}""#)),
            "section {} keeps its declared left margin, got {:?}",
            i + 1,
            pg_mars[i]
        );
    }
}

#[test]
fn issue_table_marker_height_c2_marker_is_one_point() {
    let package = fixture_package_with_options(
        "issue_table_marker_height",
        &ConvertOptions::new(CompatMode::C2),
    );
    let xml = package.part_text("word/document.xml");
    assert_eq!(
        xml.matches("<w:sectPr").count(),
        2,
        "the boundary keeps its own w:sectPr, got {xml:?}"
    );
    let marker = xml
        .split("</w:tbl>")
        .nth(1)
        .and_then(|rest| rest.split("</w:p>").next())
        .unwrap_or_default();
    assert!(
        marker.contains(r#"<w:sz w:val="2"/>"#) && marker.contains("<w:sectPr>"),
        "the marker paragraph carries a 1pt mark so it almost never spills to a blank page, got {marker:?}"
    );
}

#[test]
fn issue_margin_zero_c2_emits_explicit_zero() {
    let package =
        fixture_package_with_options("issue_margin_zero", &ConvertOptions::new(CompatMode::C2));
    let xml = package.part_text("word/document.xml");
    assert!(
        xml.contains(r#"<w:pgMar w:top="0" w:right="0" w:bottom="0" w:left="0""#),
        "an explicit margin: 0pt is written as declared zeros, got {xml:?}"
    );
}

#[test]
fn issue_margin_variable_c2_emits_explicit_zero() {
    let package = fixture_package_with_options(
        "issue_margin_variable",
        &ConvertOptions::new(CompatMode::C2),
    );
    let xml = package.part_text("word/document.xml");
    assert!(
        xml.contains(r#"<w:pgMar w:top="0" w:right="0" w:bottom="0" w:left="0""#),
        "a margin declared through a variable resolves to declared zeros, got {xml:?}"
    );
}

#[test]
fn issue_inside_outside_margins_c1_reports() {
    let outcome = c1("issue_inside_outside_margins");
    assert_has(&outcome, "mirrored-margins");
    let package = fixture_package_with_options(
        "issue_inside_outside_margins",
        &ConvertOptions::new(CompatMode::C1),
    );
    assert!(
        !package
            .part_text("word/settings.xml")
            .contains("mirrorMargins"),
        "C1 does not emit w:mirrorMargins"
    );
}

#[test]
fn issue_page_numbering_change_c1_reports() {
    let outcome = c1("issue_page_numbering_change");
    assert_has(&outcome, "page-numbering-change");
    let package = fixture_package_with_options(
        "issue_page_numbering_change",
        &ConvertOptions::new(CompatMode::C1),
    );
    let xml = package.part_text("word/document.xml");
    assert_eq!(
        xml.matches("<w:sectPr").count(),
        1,
        "C1 keeps a single section; the numbering change is not realized"
    );
    assert!(!xml.contains("w:start"), "C1 emits no page-number restart");
}

#[test]
fn issue_page_numbering_mid_c1_no_change_degradation() {
    let outcome = c1("issue_page_numbering_mid");
    let constructs = constructs(&outcome);
    assert!(
        !constructs.iter().any(|c| c == "page-numbering-change"),
        "a single mid-document numbering declaration is not a change, got {constructs:?}"
    );
    assert!(
        constructs.iter().any(|c| c == "section-break"),
        "the mid-document page-setting change is still detected, got {constructs:?}"
    );
}

#[test]
fn issue_page_numbering_change_c2_emits_fmt_and_restart() {
    let package = fixture_package_with_options(
        "issue_page_numbering_change",
        &ConvertOptions::new(CompatMode::C2),
    );
    let xml = package.part_text("word/document.xml");
    assert_eq!(
        xml.matches("<w:sectPr").count(),
        2,
        "one w:sectPr per numbering section, got {xml:?}"
    );
    assert!(
        xml.contains(r#"<w:pgNumType w:fmt="decimal"/>"#),
        "section 1 keeps decimal numbering, got {xml:?}"
    );
    assert!(
        xml.contains(r#"<w:pgNumType w:fmt="lowerRoman" w:start="1"/>"#),
        "section 2 restarts at 1 in lowerRoman, got {xml:?}"
    );
    let footer = package.part_text("word/footer1.xml");
    assert!(
        footer.contains(" PAGE "),
        "the footer keeps a PAGE field across the numbering change, got {footer:?}"
    );
}

/// The run that contains `text` carries `<w:b/>` iff `bold` holds.
fn run_boldness(xml: &str, text: &str) -> Option<bool> {
    let text_at = xml.find(text)?;
    let run_at = xml[..text_at].rfind("<w:r>")?;
    Some(xml[run_at..text_at].contains("<w:b/>"))
}

#[test]
fn issue_header_run_styles_keep_size() {
    let package = fixture_package("issue_header_run_styles");
    let header = package.part_text("word/header1.xml");
    assert!(
        header.contains("<w:sz w:val=\"18\"/>"),
        "header run keeps its 9pt size, got {header:?}"
    );
    assert!(
        header.contains("Bold Title"),
        "header text is present, got {header:?}"
    );
    assert_eq!(
        run_boldness(&header, "Bold Title"),
        Some(true),
        "the strong header run emits w:b, got {header:?}"
    );
    assert_eq!(
        run_boldness(&header, "Running "),
        Some(false),
        "the plain header run emits no w:b, got {header:?}"
    );
}

#[test]
fn issue_header_run_styles_c2_also_keeps_size() {
    let package = fixture_package_with_options(
        "issue_header_run_styles",
        &ConvertOptions::new(CompatMode::C2),
    );
    assert!(
        package
            .part_text("word/header1.xml")
            .contains("<w:sz w:val=\"18\"/>"),
        "header run keeps its 9pt size"
    );
}
