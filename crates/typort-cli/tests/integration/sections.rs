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
        header.contains("Odd Title 1"),
        "the first page's header text is kept, got {header:?}"
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
fn issue_header_run_styles_keep_size() {
    let package = fixture_package("issue_header_run_styles");
    let header = package.part_text("word/header1.xml");
    assert!(
        header.contains("<w:sz w:val=\"18\"/>"),
        "header run keeps its 9pt size, got {header:?}"
    );
    assert!(
        header.contains("Running title 1"),
        "header text and the literal page number are present"
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
