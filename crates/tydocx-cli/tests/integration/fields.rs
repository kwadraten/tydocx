//! The C3 field layer: dynamic Word fields whose cached result is the C2
//! literal output, so disconnecting every field reproduces C2 (spec D1).
//! Each test pins both halves — the field instruction is present at C3, and
//! the visible text is identical to the C2 output — plus the field's absence
//! at C1/C2.

use crate::common::{fixture_outcome_with_options, fixture_package_with_options, paragraph_texts};
use tydocx_core::{CompatMode, ConvertOptions};

fn part(fixture: &str, mode: CompatMode, part: &str) -> String {
    fixture_package_with_options(fixture, &ConvertOptions::new(mode))
        .part_text(part)
        .to_owned()
}

/// Every `<w:t>` text in document order — what the reader sees before any
/// field update. C3's cached results must concatenate to the C2 literal
/// output exactly.
fn visible_text(xml: &str) -> String {
    paragraph_texts(xml).concat()
}

/// The visible text of `word/document.xml` plus every header/footer part —
/// fields in margin zones count too.
fn all_visible_text(fixture: &str, mode: CompatMode) -> String {
    let package = fixture_package_with_options(fixture, &ConvertOptions::new(mode));
    let mut text = visible_text(package.part_text("word/document.xml"));
    for name in package.part_names() {
        if name.starts_with("word/header") || name.starts_with("word/footer") {
            text.push_str(&visible_text(package.part_text(name)));
        }
    }
    text
}

/// C3's rendered text equals C2's on every field-bearing fixture — the D1
/// invariant, corpus-wide.
#[test]
fn c3_cached_results_reproduce_c2_text() {
    for fixture in [
        "fields_numpages",
        "fields_datetime",
        "fields_docproperty",
        "fields_noteref",
        "fields_styleref",
        "fields_seq",
        "fields_hyperlink",
        "bibliography_basic",
    ] {
        assert_eq!(
            all_visible_text(fixture, CompatMode::C2),
            all_visible_text(fixture, CompatMode::C3),
            "{fixture}: C3 cached results must reproduce the C2 literal text"
        );
    }
}

#[test]
fn numpages_field_replaces_total_page_literal() {
    let c3 = part("fields_numpages", CompatMode::C3, "word/document.xml");
    assert!(
        c3.contains(" NUMPAGES "),
        "C3 should emit a NUMPAGES field: {c3}"
    );
    let c2 = part("fields_numpages", CompatMode::C2, "word/document.xml");
    assert!(
        !c2.contains(" NUMPAGES "),
        "C2 keeps the total page count literal: {c2}"
    );
    assert_eq!(
        visible_text(&c2),
        visible_text(&c3),
        "the NUMPAGES cached result must be the C2 literal"
    );
}

#[test]
fn datetime_today_emits_date_field_with_cached_text() {
    let c3 = part("fields_datetime", CompatMode::C3, "word/document.xml");
    assert_eq!(
        c3.matches(" DATE ").count(),
        2,
        "both display forms should emit DATE fields: {c3}"
    );
    let c2 = part("fields_datetime", CompatMode::C2, "word/document.xml");
    assert!(!c2.contains(" DATE "), "C2 stays literal: {c2}");
    assert_eq!(
        visible_text(&c2),
        visible_text(&c3),
        "the explicit-format date keeps its C2 literal as the cached result"
    );
}

#[test]
fn document_metadata_reads_emit_docproperty_fields() {
    let c3 = part("fields_docproperty", CompatMode::C3, "word/document.xml");
    for name in ["Title", "Author", "Keywords", "Comments", "CreateDate"] {
        assert!(
            c3.contains(&format!(" DOCPROPERTY {name} ")),
            "C3 should emit DOCPROPERTY {name}: {c3}"
        );
    }
    let c2 = part("fields_docproperty", CompatMode::C2, "word/document.xml");
    assert!(!c2.contains(" DOCPROPERTY "), "C2 stays literal: {c2}");
    assert_eq!(visible_text(&c2), visible_text(&c3));
}

#[test]
fn footnote_counter_reference_emits_noteref_and_bookmark() {
    let package =
        fixture_package_with_options("fields_noteref", &ConvertOptions::new(CompatMode::C3));
    let doc = package.part_text("word/document.xml");
    assert!(
        doc.contains(" NOTEREF fn1 \\h "),
        "C3 should emit a hyperlinked NOTEREF: {doc}"
    );
    let footnotes = package.part_text("word/footnotes.xml");
    assert!(
        footnotes.contains("w:name=\"fn1\""),
        "the labelled footnote must carry the bookmark the NOTEREF targets: {footnotes}"
    );
    let c2 = part("fields_noteref", CompatMode::C2, "word/document.xml");
    assert!(!c2.contains(" NOTEREF "), "C2 stays literal: {c2}");
    assert_eq!(visible_text(&c2), visible_text(doc));

    let c2_constructs: Vec<String> =
        fixture_outcome_with_options("fields_noteref", &ConvertOptions::new(CompatMode::C2))
            .degradations
            .iter()
            .map(|d| d.construct.to_string())
            .collect();
    assert!(
        c2_constructs
            .iter()
            .any(|c| c == "footnote-counter-reference"),
        "C2 reports the literalized reference: {c2_constructs:?}"
    );
    let c3_constructs: Vec<String> =
        fixture_outcome_with_options("fields_noteref", &ConvertOptions::new(CompatMode::C3))
            .degradations
            .iter()
            .map(|d| d.construct.to_string())
            .collect();
    assert!(
        !c3_constructs
            .iter()
            .any(|c| c == "footnote-counter-reference"),
        "C3 emits the field instead of degrading: {c3_constructs:?}"
    );
}

#[test]
fn heading_query_header_emits_styleref() {
    let header = part("fields_styleref", CompatMode::C3, "word/header1.xml");
    assert!(
        header.contains(" STYLEREF &quot;Heading 1&quot; "),
        "C3 should emit STYLEREF for the heading-query header: {header}"
    );
    assert!(
        header.contains(">Intro<"),
        "the cached result is the rendered heading text: {header}"
    );
    let c2 = part("fields_styleref", CompatMode::C2, "word/header1.xml");
    assert!(
        !c2.contains(" STYLEREF "),
        "C2 keeps the header literal: {c2}"
    );
    assert_eq!(visible_text(&c2), visible_text(&header));
}

#[test]
fn figure_table_equation_numbers_emit_seq() {
    let c3 = part("fields_seq", CompatMode::C3, "word/document.xml");
    for instruction in [" SEQ Figure ", " SEQ Table ", " SEQ equation "] {
        assert!(
            c3.contains(instruction),
            "C3 should emit {instruction:?}: {c3}"
        );
    }
    let c2 = part("fields_seq", CompatMode::C2, "word/document.xml");
    assert!(!c2.contains(" SEQ "), "C2 keeps the numbers literal: {c2}");
    assert_eq!(
        visible_text(&c2),
        visible_text(&c3),
        "SEQ cached numerals must be the C2 literals"
    );
}

#[test]
fn citations_emit_citation_fields_and_sources() {
    let package =
        fixture_package_with_options("bibliography_basic", &ConvertOptions::new(CompatMode::C3));
    let doc = package.part_text("word/document.xml");
    assert!(
        doc.contains(" CITATION smith2020 \\l 1033 "),
        "C3 should emit CITATION fields with the English locale: {doc}"
    );
    let sources = package.part_text("customXml/item1.xml");
    assert!(
        sources.contains("<b:Sources") && sources.contains("<b:Tag>smith2020</b:Tag>"),
        "the bibliography sources custom XML carries the cited keys: {sources}"
    );
    let c2 = part("bibliography_basic", CompatMode::C2, "word/document.xml");
    assert!(
        !c2.contains(" CITATION "),
        "C2 keeps the citation markers literal: {c2}"
    );
    assert_eq!(visible_text(&c2), visible_text(doc));
}

#[test]
fn external_links_use_the_hyperlink_field_form() {
    for mode in [CompatMode::C2, CompatMode::C3] {
        let doc = part("fields_hyperlink", mode, "word/document.xml");
        assert!(
            doc.contains("HYPERLINK &amp;quot;https://example.com&amp;quot;"),
            "{mode:?}: external links emit the HYPERLINK field: {doc}"
        );
        assert!(
            doc.contains(">the example site<"),
            "{mode:?}: the link text is the field result: {doc}"
        );
    }
}

#[test]
fn update_fields_is_emitted_only_at_c3() {
    for (fixture, present) in [("fields_numpages", true)] {
        let c3 = part(fixture, CompatMode::C3, "word/settings.xml");
        assert_eq!(
            c3.contains("<w:updateFields"),
            present,
            "C3 settings should carry w:updateFields: {c3}"
        );
    }
    for mode in [CompatMode::C1, CompatMode::C2] {
        let settings = part("fields_numpages", mode, "word/settings.xml");
        assert!(
            !settings.contains("<w:updateFields"),
            "{mode:?} must not ask Word to update fields: {settings}"
        );
    }
}
