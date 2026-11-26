//! Section, header/footer, and page-furniture tests (manuscript cases
//! T-01/M7, T-02/U13, T-06/M6, T-07/U31, T-08/U30, T-22/L3).

use crate::common::{
    fixture_doc_xml, fixture_outcome_with_options, fixture_package, fixture_package_with_options,
    paragraph_containing,
};
use tydocx_core::{CompatMode, ConvertOptions};

fn c1(fixture: &str) -> tydocx_core::ConvertOutcome {
    fixture_outcome_with_options(fixture, &ConvertOptions::new(CompatMode::C1))
}

fn constructs(outcome: &tydocx_core::ConvertOutcome) -> Vec<String> {
    outcome
        .degradations
        .iter()
        .map(|d| d.construct.to_string())
        .collect()
}

fn assert_has(outcome: &tydocx_core::ConvertOutcome, construct: &str) {
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

/// Per-section header parts: each `w:sectPr` references the parts derived
/// from that section's own pages — the cover section emits no
/// `w:headerReference` at all, and the two later sections reference their own
/// `word/headerN.xml` parts (numbered past the document-level slots 1–3).
/// Revert proof: at bf52fa5 the top zone is classified once over the whole
/// document, so the cover's empty zone becomes the default and no
/// `word/headerN.xml` part is emitted at all.
#[test]
fn per_section_headers_c2_each_sectpr_references_its_own_header() {
    let package =
        fixture_package_with_options("per_section_headers", &ConvertOptions::new(CompatMode::C2));
    let document = package.part_text("word/document.xml");
    assert_eq!(
        document.matches("<w:sectPr").count(),
        3,
        "one w:sectPr per page-settings section, got {document:?}"
    );
    let sectprs: Vec<&str> = document.split("<w:sectPr").collect();
    assert!(
        !sectprs[1].contains("w:headerReference"),
        "the cover section's sectPr references no header part, got {:?}",
        sectprs[1]
    );
    assert!(
        sectprs[2].contains(r#"<w:headerReference w:type="default""#),
        "the abstract section's sectPr references its header, got {:?}",
        sectprs[2]
    );
    assert!(
        sectprs[3].contains(r#"<w:headerReference w:type="default""#),
        "the body section's sectPr references its header, got {:?}",
        sectprs[3]
    );
    let abstract_header = package.part_text("word/header4.xml");
    assert!(
        abstract_header.contains("Abstract Header"),
        "the abstract section's part carries its header, got {abstract_header:?}"
    );
    let body_header = package.part_text("word/header5.xml");
    assert!(
        body_header.contains("Body Header"),
        "the body section's part carries its header, got {body_header:?}"
    );
    let rels = package.part_text("word/_rels/document.xml.rels");
    assert!(
        rels.contains("header4.xml") && rels.contains("header5.xml"),
        "both section header parts are wired into the package, got {rels:?}"
    );
}

/// The same document at C1 keeps the single-section contract: one sectPr,
/// and the variant is still reported rather than silently dropped.
#[test]
fn per_section_headers_c1_reports_variant() {
    let outcome = c1("per_section_headers");
    assert_has(&outcome, "header-footer-variant");
    let package =
        fixture_package_with_options("per_section_headers", &ConvertOptions::new(CompatMode::C1));
    let xml = package.part_text("word/document.xml");
    assert_eq!(xml.matches("<w:sectPr").count(), 1, "C1 keeps one section");
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
fn issue_inside_outside_margins_c2_emits_flag() {
    let package = fixture_package_with_options(
        "issue_inside_outside_margins",
        &ConvertOptions::new(CompatMode::C2),
    );
    let settings = package.part_text("word/settings.xml");
    assert!(
        settings.contains("<w:mirrorMargins/>"),
        "C2 emits w:mirrorMargins, got {settings:?}"
    );
    let xml = package.part_text("word/document.xml");
    assert!(
        xml.contains(r#"w:left="1417""#) && xml.contains(r#"w:right="850""#),
        "w:pgMar keeps left=inside (25mm), right=outside (15mm), got {xml:?}"
    );
    let outcome = fixture_outcome_with_options(
        "issue_inside_outside_margins",
        &ConvertOptions::new(CompatMode::C2),
    );
    assert!(
        !constructs(&outcome).iter().any(|c| c == "mirrored-margins"),
        "a representable mirrored document reports no degradation"
    );
}

#[test]
fn mirrored_margins_inside_outside_c2_emits_flag() {
    let package = fixture_package_with_options(
        "mirrored_margins_inside_outside",
        &ConvertOptions::new(CompatMode::C2),
    );
    let settings = package.part_text("word/settings.xml");
    assert!(
        settings.contains("<w:mirrorMargins/>"),
        "C2 emits w:mirrorMargins, got {settings:?}"
    );
    let xml = package.part_text("word/document.xml");
    assert!(
        xml.contains(r#"w:left="1134""#) && xml.contains(r#"w:right="567""#),
        "w:pgMar keeps left=inside (20mm), right=outside (10mm), got {xml:?}"
    );
}

#[test]
fn mirrored_margins_inside_outside_c3_emits_flag() {
    let c3 = fixture_package_with_options(
        "mirrored_margins_inside_outside",
        &ConvertOptions::new(CompatMode::C3),
    );
    assert!(
        c3.part_text("word/settings.xml")
            .contains("<w:mirrorMargins/>"),
        "C3 emits w:mirrorMargins"
    );
    // D1: the document body is field-free, so C3 reproduces C2 exactly.
    let c2 = fixture_package_with_options(
        "mirrored_margins_inside_outside",
        &ConvertOptions::new(CompatMode::C2),
    );
    assert_eq!(
        c2.part_text("word/document.xml"),
        c3.part_text("word/document.xml"),
        "C3 reproduces the C2 document text"
    );
}

#[test]
fn mirrored_margins_inside_outside_c1_reports() {
    let outcome = c1("mirrored_margins_inside_outside");
    assert_has(&outcome, "mirrored-margins");
    let package = fixture_package_with_options(
        "mirrored_margins_inside_outside",
        &ConvertOptions::new(CompatMode::C1),
    );
    assert!(
        !package
            .part_text("word/settings.xml")
            .contains("mirrorMargins"),
        "C1 does not emit w:mirrorMargins"
    );
}

/// `w:mirrorMargins` is document-global: a document that mixes inside/outside
/// margins with a one-sided asymmetric section cannot be faithful on both
/// halves. The flag still goes out — the mirrored section needs it — and the
/// one-sided section's even-page swap is reported.
#[test]
fn mirrored_margins_mixed_section_c2_flags_and_reports() {
    let outcome = fixture_outcome_with_options(
        "mirrored_margins_mixed",
        &ConvertOptions::new(CompatMode::C2),
    );
    assert!(
        outcome.degradations.iter().any(|d| {
            d.construct == "mirrored-margins"
                && d.action == tydocx_core::DegradationAction::Approximated
        }),
        "the one-sided section's swap is reported, got {:?}",
        outcome.degradations
    );
    let package = fixture_package_with_options(
        "mirrored_margins_mixed",
        &ConvertOptions::new(CompatMode::C2),
    );
    assert!(
        package
            .part_text("word/settings.xml")
            .contains("<w:mirrorMargins/>"),
        "the flag is still emitted — the mirrored section needs it"
    );
}

/// Margins declared through a `#let` variable reach the `StyleChain` but not
/// the source-AST read — `w:mirrorMargins` accumulates any section's
/// `two_sided`, so a later one-sided section cannot mask it.
#[test]
fn mirrored_margins_variable_c2_flags_and_reports() {
    let package = fixture_package_with_options(
        "mirrored_margins_variable",
        &ConvertOptions::new(CompatMode::C2),
    );
    assert!(
        package
            .part_text("word/settings.xml")
            .contains("<w:mirrorMargins/>"),
        "C2 emits w:mirrorMargins when any section uses inside/outside margins"
    );
    let outcome = fixture_outcome_with_options(
        "mirrored_margins_variable",
        &ConvertOptions::new(CompatMode::C2),
    );
    assert!(
        outcome.degradations.iter().any(|d| {
            d.construct == "mirrored-margins"
                && d.action == tydocx_core::DegradationAction::Approximated
        }),
        "the one-sided section's swap is reported: {:?}",
        outcome.degradations
    );
}

#[test]
fn mirrored_margins_variable_c3_emits_flag() {
    let package = fixture_package_with_options(
        "mirrored_margins_variable",
        &ConvertOptions::new(CompatMode::C3),
    );
    assert!(
        package
            .part_text("word/settings.xml")
            .contains("<w:mirrorMargins/>"),
        "C3 emits w:mirrorMargins"
    );
}

/// Pin for the pre-existing C1 quirk: `margins_mirrored` reads the chain's
/// last-wins value, so a document ending on a one-sided margin reports only
/// the merged section — not `mirrored-margins` — and emits no flag.
#[test]
fn mirrored_margins_variable_c1_reports_section_break_only() {
    let outcome = c1("mirrored_margins_variable");
    assert_has(&outcome, "section-break");
    assert!(
        !constructs(&outcome).iter().any(|c| c == "mirrored-margins"),
        "C1 keeps the chain's last-wins read — the mirrored section is not reported"
    );
    let package = fixture_package_with_options(
        "mirrored_margins_variable",
        &ConvertOptions::new(CompatMode::C1),
    );
    assert!(
        !package
            .part_text("word/settings.xml")
            .contains("mirrorMargins"),
        "C1 does not emit w:mirrorMargins"
    );
}

/// A one-sided section whose lateral margins are symmetric is unaffected by
/// the document-global flag — representable, so no report.
#[test]
fn mirrored_margins_mixed_symmetric_c2_flags_without_report() {
    let outcome = fixture_outcome_with_options(
        "mirrored_margins_mixed_symmetric",
        &ConvertOptions::new(CompatMode::C2),
    );
    assert!(
        !constructs(&outcome).iter().any(|c| c == "mirrored-margins"),
        "a symmetric one-sided section is unaffected by the flag, got {:?}",
        outcome.degradations
    );
    let package = fixture_package_with_options(
        "mirrored_margins_mixed_symmetric",
        &ConvertOptions::new(CompatMode::C2),
    );
    assert!(
        package
            .part_text("word/settings.xml")
            .contains("<w:mirrorMargins/>"),
        "the flag is still emitted for the mirrored section"
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

/// The `w:type w:val` of every `w:sectPr` in document order — the mid-body
/// `w:pPr` sectPrs plus the body-final one.
fn section_types(xml: &str) -> Vec<&str> {
    xml.split("<w:type w:val=\"")
        .skip(1)
        .map(|s| s.split('"').next().unwrap_or_default())
        .collect()
}

/// A `#columns(n)[…]` block lowers to a continuous section pair: the
/// ambient section ends at the paragraph before the block, the block's
/// paragraphs sit inside a `w:cols` section that ends `continuous`, and
/// the resumed ambient section also starts (and so ends) `continuous`.
/// Each `w:sectPr` describes the section that *ends* at it, so the
/// `w:type` sequence records how each section started.
#[test]
fn columns_block_emits_continuous_section_pair() {
    let xml = fixture_doc_xml("columns_two");
    assert_eq!(
        section_types(&xml),
        ["nextPage", "continuous", "continuous"],
        "ambient -> columns -> ambient, all continuous but the first: {xml}"
    );
    assert!(
        xml.contains(r#"<w:cols w:num="2" w:space="363"/>"#),
        "two columns, 4% of the 160mm text width ≈ 363 twips: {xml}"
    );
    let in_break = paragraph_containing(&xml, "Before the columns block.");
    assert!(
        in_break.contains("<w:sectPr>") && !in_break.contains("<w:cols"),
        "the ambient section ends on the paragraph before the block"
    );
    let out_break = paragraph_containing(&xml, "Second column text.");
    assert!(
        out_break.contains(r#"<w:cols w:num="2""#),
        "the columns section ends on the block's last paragraph"
    );
    let after = paragraph_containing(&xml, "After the columns block.");
    assert!(
        !after.contains("<w:sectPr>") && !after.contains("<w:cols"),
        "the paragraph after the block is plain single-column flow"
    );
}

/// A declared `gutter:` on the block is used verbatim — 8mm ≈ 454 twips.
#[test]
fn columns_block_declared_gutter() {
    let xml = fixture_doc_xml("columns_three_gutter");
    assert!(
        xml.contains(r#"<w:cols w:num="3" w:space="454"/>"#),
        "three columns with the declared 8mm gutter: {xml}"
    );
}

/// A block mid-page still gets the full pair — text before and after it
/// stays in the ambient single-column flow on the same page.
#[test]
fn columns_block_mid_page_sections_cleanly() {
    let xml = fixture_doc_xml("columns_mid_page");
    assert_eq!(
        section_types(&xml),
        ["nextPage", "continuous", "continuous"],
        "the block sections without disturbing the ambient flow: {xml}"
    );
    assert!(
        xml.contains(r#"<w:cols w:num="2" w:space="408"/>"#),
        "4% of the 180mm text width ≈ 408 twips: {xml}"
    );
    for text in [
        "Intro paragraph that shares the page.",
        "A mid-page column block.",
        "And a following paragraph, still on the first page.",
    ] {
        assert!(xml.contains(text), "missing {text:?}: {xml}");
    }
}

/// A `#pagebreak()` immediately before the block is real content the
/// continuous in-break must not swallow — it survives in the paragraph
/// that carries the ending `w:sectPr`.
#[test]
fn columns_block_after_pagebreak_keeps_break() {
    let xml = fixture_doc_xml("columns_page_break");
    let break_para = paragraph_containing(&xml, r#"<w:br w:type="page"/>"#);
    assert!(
        break_para.contains("<w:sectPr>"),
        "the explicit page break shares the in-break paragraph: {xml}"
    );
    assert!(
        xml.contains(r#"<w:cols w:num="2""#),
        "the columns section still forms: {xml}"
    );
}

/// `w:start` belongs to the OOXML section that began the restart: the
/// continuous breaks a `#columns` block inserts *inside* that section keep
/// the numbering format but must not re-initiate the restart.
#[test]
fn columns_inside_restart_section_stamps_start_once() {
    for mode in [CompatMode::C2, CompatMode::C3] {
        let xml = fixture_package_with_options(
            "issue_columns_numbering_restart",
            &ConvertOptions::new(mode),
        )
        .part_text("word/document.xml")
        .to_owned();
        assert_eq!(
            xml.matches(r#"w:start="7""#).count(),
            1,
            "{mode:?}: the restart stamps once, on the section that began it: {xml}"
        );
        assert!(
            xml.contains(r#"<w:pgNumType w:fmt="lowerRoman" w:start="7"/>"#),
            "{mode:?}: the restarted section keeps its format and start: {xml}"
        );
        assert_eq!(
            xml.matches(r#"<w:pgNumType w:fmt="lowerRoman"/>"#).count(),
            2,
            "{mode:?}: the columns section and the resumed ambient section \
             keep the format without re-initiating the restart: {xml}"
        );
    }
}

/// A `#pagebreak()` right before the block is real content the continuous
/// in-break keeps; the restart still stamps exactly once, on the section
/// boundary that began it.
#[test]
fn columns_after_pagebreak_in_restart_section_stamps_start_once() {
    for mode in [CompatMode::C2, CompatMode::C3] {
        let xml = fixture_package_with_options(
            "issue_columns_restart_pagebreak",
            &ConvertOptions::new(mode),
        )
        .part_text("word/document.xml")
        .to_owned();
        assert_eq!(
            xml.matches(r#"w:start="7""#).count(),
            1,
            "{mode:?}: the restart stamps once at the real section boundary: {xml}"
        );
        assert_eq!(
            xml.matches(r#"<w:br w:type="page"/>"#).count(),
            1,
            "{mode:?}: the explicit page break beside the columns boundary is kept: {xml}"
        );
    }
}

/// Representable top-level blocks never report `column-block` — at any
/// compat level the continuous-section pair is plain OOXML.
#[test]
fn columns_block_reports_no_degradation() {
    for mode in [CompatMode::C1, CompatMode::C2, CompatMode::C3] {
        let outcome = fixture_outcome_with_options("columns_two", &ConvertOptions::new(mode));
        assert!(
            !outcome
                .degradations
                .iter()
                .any(|d| d.construct == "column-block"),
            "{mode:?} reports a representable block: {:?}",
            outcome.degradations
        );
    }
}

/// Sections cannot scope inside a table cell — the nested block reports
/// `column-block` and keeps its content in single-column flow.
#[test]
fn columns_block_nested_degrades_and_keeps_content() {
    let outcome =
        fixture_outcome_with_options("columns_nested", &ConvertOptions::new(CompatMode::C2));
    assert!(
        outcome
            .degradations
            .iter()
            .any(|d| { d.construct == "column-block" && d.message.contains("single-column") }),
        "the nested block reports its single-column flattening: {:?}",
        outcome.degradations
    );
    let xml = fixture_doc_xml("columns_nested");
    assert_eq!(
        xml.matches("<w:sectPr").count(),
        1,
        "only the body-final sectPr remains: {xml}"
    );
    assert!(
        xml.contains("Cell text that stays single-column inside a table."),
        "the nested content survives: {xml}"
    );
}

/// `page(columns:)` without a declared gutter resolves Typst's 4%-of-text-
/// width default — not the writer's old 720-twip fallback.
#[test]
fn page_columns_default_gutter_resolves_against_text_width() {
    let xml = fixture_doc_xml("columns_page_default_gutter");
    assert!(
        xml.contains(r#"<w:cols w:num="2" w:space="340"/>"#),
        "4% of the 150mm text width ≈ 340 twips, not 720: {xml}"
    );
    assert!(
        !xml.contains(r#"w:space="720""#),
        "the hardcoded fallback is gone: {xml}"
    );
}

/// A declared `#set columns(gutter:)` keeps its value on a multi-column
/// page — the 4% default only applies when nothing is declared.
#[test]
fn page_columns_declared_gutter_wins() {
    let xml = fixture_doc_xml("columns_page_set_gutter");
    assert!(
        xml.contains(r#"<w:cols w:num="2" w:space="567"/>"#),
        "the declared 10mm gutter ≈ 567 twips is kept: {xml}"
    );
}

/// A declared *ratio* gutter resolves against the text width like the
/// default does — the AST decomposition drops the ratio part, so the
/// chain resolution is the only correct value.
#[test]
fn page_columns_ratio_gutter_resolves_against_text_width() {
    let xml = fixture_doc_xml("columns_page_ratio_gutter");
    assert!(
        xml.contains(r#"<w:cols w:num="2" w:space="425"/>"#),
        "5% of the 150mm text width ≈ 425 twips, not 720: {xml}"
    );
}
