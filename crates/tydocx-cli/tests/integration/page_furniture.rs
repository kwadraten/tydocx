//! Page furniture anchoring tests: `page(foreground:)` / `page(background:)` /
//! `page(fill:)` and `#place` inside the header/footer zones become
//! `wp:anchor` drawings in header/footer parts in C2/C3, with the
//! default/even/first part slots carrying the page-variation shapes Word can
//! express. Unrepresentable variation and unsupported layer content are
//! reported, never dropped silently.

use crate::common::{FixturePackage, fixture_outcome_with_options, fixture_package_with_options};
use tydocx_core::{CompatMode, ConvertOptions};

fn c2(fixture: &str) -> FixturePackage {
    fixture_package_with_options(fixture, &ConvertOptions::new(CompatMode::C2))
}

/// The header/footer part containing `needle`, or fail naming what was found.
fn furniture_part<'a>(package: &'a FixturePackage, needle: &str) -> &'a str {
    for name in package.part_names() {
        if (name.contains("header") || name.contains("footer"))
            && package.part_text(name).contains(needle)
        {
            return package.part_text(name);
        }
    }
    panic!("no header/footer part contains {needle:?}");
}

fn has_construct(outcome: &tydocx_core::ConvertOutcome, construct: &str) -> bool {
    outcome
        .degradations
        .iter()
        .any(|d| d.construct == construct)
}

#[test]
fn issue_page_doi_foreground_c2_anchors_editable_text_box() {
    let package = c2("issue_page_doi_foreground");
    let body = package.part_text("word/document.xml");
    assert!(
        !body.contains("DOI-PLACEHOLDER"),
        "foreground furniture stays out of the reading order"
    );
    let header = furniture_part(&package, "DOI-PLACEHOLDER");
    assert!(
        header.contains("<wp:anchor"),
        "furniture is an anchored drawing, got {header:?}"
    );
    assert!(
        !header.contains("<wp:inline"),
        "furniture must never be an inline drawing, got {header:?}"
    );
    assert!(
        header.contains(r#"positionH relativeFrom="page""#)
            && header.contains(r#"positionV relativeFrom="page""#),
        "anchor positions resolve against the page, got {header:?}"
    );
    assert!(
        header.contains("wps:txbx") && header.contains("txbxContent"),
        "furniture text stays editable in a text box, got {header:?}"
    );
}

#[test]
fn issue_page_foreground_place_c2_anchors_text() {
    let package = c2("issue_page_foreground_place");
    assert!(
        !package
            .part_text("word/document.xml")
            .contains("SPECTROSCOPY MARK"),
        "foreground furniture stays out of the reading order"
    );
    let header = furniture_part(&package, "SPECTROSCOPY MARK");
    assert!(
        header.contains("<wp:anchor") && header.contains("txbxContent"),
        "foreground text anchors as an editable text box, got {header:?}"
    );
}

#[test]
fn issue_page_background_furniture_c2_anchors_behind_doc() {
    let package = c2("issue_page_background_furniture");
    let header = furniture_part(&package, "BG-STAMP");
    assert!(
        header.contains("<wp:anchor") && header.contains(r#"behindDoc="1""#),
        "background furniture anchors behind the document, got {header:?}"
    );
    assert!(
        header.contains("pic:pic"),
        "the background image anchors as a picture, got {header:?}"
    );
    assert!(
        header.contains("a:solidFill") && header.contains(r#"val="C8102E""#),
        "the background rect anchors as a filled shape, got {header:?}"
    );
    assert!(
        header.contains("txbxContent"),
        "the background text stays editable, got {header:?}"
    );
}

#[test]
fn issue_page_header_place_c2_anchor_coexists_with_header_text() {
    let package = c2("issue_page_header_place");
    let header = package.part_text("word/header1.xml");
    assert!(
        header.contains("Doc Title"),
        "ordinary header text is preserved, got {header:?}"
    );
    assert!(
        header.contains("<wp:anchor") && header.contains("HDR-PLACE"),
        "the header-zone #place anchors in the same part, got {header:?}"
    );
}

#[test]
fn issue_page_footer_place_c2_anchor_coexists_with_page_number() {
    let package = c2("issue_page_footer_place");
    let footer = package.part_text("word/footer1.xml");
    assert!(
        footer.contains("<wp:anchor") && footer.contains("FTR-STAMP"),
        "the footer-zone #place anchors in the footer part, got {footer:?}"
    );
    assert!(
        footer.contains(">1<") || footer.contains("PAGE"),
        "the page number still renders beside the anchor, got {footer:?}"
    );
}

#[test]
fn issue_page_fill_solid_c2_full_page_shape() {
    let package = c2("issue_page_fill_solid");
    let header = furniture_part(&package, "2A4B7C");
    assert!(
        header.contains("<wp:anchor") && header.contains(r#"behindDoc="1""#),
        "page fill anchors behind the document, got {header:?}"
    );
    assert!(
        header.contains(r#"cx="7560000" cy="10692000""#),
        "A4 page fill covers the whole page, got {header:?}"
    );
    assert!(
        header.contains("a:solidFill") && header.contains(r#"val="2A4B7C""#),
        "the declared colour is the shape fill, got {header:?}"
    );
}

#[test]
fn issue_page_fill_gradient_c2_reports_unsupported() {
    let outcome = fixture_outcome_with_options(
        "issue_page_fill_gradient",
        &ConvertOptions::new(CompatMode::C2),
    );
    assert!(
        has_construct(&outcome, "page-foreground"),
        "a non-solid page fill must be reported, got {:?}",
        outcome.degradations
    );
    assert!(
        outcome
            .degradations
            .iter()
            .any(|d| d.message.contains("non-solid page(fill:)")),
        "the remainder names the non-solid fill, got {:?}",
        outcome.degradations
    );
}

#[test]
fn issue_page_foreground_odd_even_c2_emits_even_and_default_parts() {
    let package = c2("issue_page_foreground_odd_even");
    let document = package.part_text("word/document.xml");
    assert!(
        document.contains(r#"<w:headerReference w:type="default""#)
            && document.contains(r#"<w:headerReference w:type="even""#),
        "odd/even foreground maps to default + even parts, got {document:?}"
    );
    assert!(
        package
            .part_text("word/settings.xml")
            .contains("evenAndOddHeaders"),
        "evenAndOddHeaders is enabled"
    );
    let odd = furniture_part(&package, "ODD");
    let even = furniture_part(&package, "EVEN");
    assert!(
        odd.contains("<wp:anchor") && even.contains("<wp:anchor"),
        "both slots carry the anchored furniture"
    );
}

#[test]
fn issue_page_foreground_first_rest_c2_emits_title_pg() {
    let package = c2("issue_page_foreground_first_rest");
    let document = package.part_text("word/document.xml");
    assert!(
        document.contains("<w:titlePg/>")
            && document.contains(r#"<w:headerReference w:type="first""#)
            && document.contains(r#"<w:headerReference w:type="default""#),
        "first-vs-rest foreground maps to titlePg + first + default, got {document:?}"
    );
    let first = furniture_part(&package, "FIRSTPAGE");
    let rest = furniture_part(&package, "RESTPAGE");
    assert!(
        first.contains("<wp:anchor") && rest.contains("<wp:anchor"),
        "both slots carry the anchored furniture, not literalized text"
    );
}

#[test]
fn issue_page_foreground_varies_c2_approximates_and_reports() {
    let outcome = fixture_outcome_with_options(
        "issue_page_foreground_varies",
        &ConvertOptions::new(CompatMode::C2),
    );
    assert!(
        has_construct(&outcome, "page-foreground"),
        "unrepresentable per-page variation must be reported, got {:?}",
        outcome.degradations
    );
    let package = c2("issue_page_foreground_varies");
    let document = package.part_text("word/document.xml");
    assert!(
        document.contains("<w:titlePg/>"),
        "the page-1 variant rides the first slot, got {document:?}"
    );
    furniture_part(&package, "PAGE 1 MARK");
    assert!(
        !package
            .part_names()
            .filter(|name| name.contains("header"))
            .any(|name| package.part_text(name).contains("PAGE 2 MARK")),
        "no per-physical-page part is invented"
    );
}

#[test]
fn universe_page_foreground_c2_keeps_text_reports_transform() {
    let outcome = fixture_outcome_with_options(
        "universe_page_foreground",
        &ConvertOptions::new(CompatMode::C2),
    );
    assert!(
        has_construct(&outcome, "page-foreground"),
        "the rotated furniture must name its transform loss, got {:?}",
        outcome.degradations
    );
    let package = c2("universe_page_foreground");
    let header = furniture_part(&package, "DRAFT");
    assert!(
        header.contains("<wp:anchor") && header.contains("txbxContent"),
        "the text itself survives as an editable anchor, got {header:?}"
    );
}

#[test]
fn issue_page_doi_foreground_c1_keeps_text_without_anchor() {
    let package = fixture_package_with_options(
        "issue_page_doi_foreground",
        &ConvertOptions::new(CompatMode::C1),
    );
    let header = furniture_part(&package, "DOI-PLACEHOLDER");
    assert!(
        !header.contains("<wp:anchor"),
        "C1 literalizes furniture text without drawings, got {header:?}"
    );
}

#[test]
fn issue_page_doi_foreground_c3_anchors_like_c2() {
    let package = fixture_package_with_options(
        "issue_page_doi_foreground",
        &ConvertOptions::new(CompatMode::C3),
    );
    let header = furniture_part(&package, "DOI-PLACEHOLDER");
    assert!(
        header.contains("<wp:anchor") && header.contains("txbxContent"),
        "C3 carries the same anchored furniture as C2, got {header:?}"
    );
}
