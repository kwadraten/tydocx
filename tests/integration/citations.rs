//! Citation marker, in-container citation, and REF-alias tests (HPL-AI
//! citations line). Fixtures live under `tests/fixtures/citation_*`,
//! `bibliography_prefix_*`, and `ref_*`.

use crate::common::{fixture_doc_xml, fixture_package_with_options, paragraph_containing};
use tydocx::{CompatMode, ConvertOptions};

/// `(w:anchor, concatenated w:t)` for every hyperlink in the document part.
fn anchor_texts(xml: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for m in xml.split("<w:hyperlink ").skip(1) {
        let anchor = m
            .split("w:anchor=\"")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .unwrap_or_default()
            .to_string();
        let body = m.split("</w:hyperlink>").next().unwrap_or_default();
        let text: String = body
            .split("<w:t")
            .skip(1)
            .filter_map(|t| t.split('>').nth(1).and_then(|t| t.split('<').next()))
            .collect();
        out.push((anchor, text));
    }
    out
}

fn bookmark_names(xml: &str) -> Vec<String> {
    xml.split("<w:bookmarkStart ")
        .skip(1)
        .filter_map(|m| {
            m.split("w:name=\"")
                .nth(1)
                .and_then(|s| s.split('"').next())
                .map(str::to_string)
        })
        .collect()
}

fn c3_doc_xml(fixture: &str) -> String {
    fixture_package_with_options(fixture, &ConvertOptions::new(CompatMode::C3))
        .part_text("word/document.xml")
        .to_owned()
}

#[test]
fn citation_marker_is_the_full_rendered_text() {
    // Every citation hyperlink's cached text must be the complete rendered
    // marker — never the first run only (the old pair-walk cut at the first
    // nested Tag::End, collapsing "[1]" to "[").
    let xml = fixture_doc_xml("citation_marker_full_text");
    let links = anchor_texts(&xml);
    assert_eq!(
        links.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>(),
        ["[1]", "[2]"],
        "stock numeric markers must be the full evaluated text: {links:?}"
    );
}

#[test]
fn citation_marker_gb7714_not_truncated() {
    // The reviewer's repro: every bib hyperlink in the paper's C2 DOCX
    // concatenated to "[". gb-7714-2015-numeric wraps the numeral in a link
    // element inside the marker; the fixed extraction takes the whole
    // rendered marker.
    let xml = fixture_doc_xml("citation_truncation_gb7714");
    let links = anchor_texts(&xml);
    assert_eq!(links.len(), 1, "one citation hyperlink expected: {links:?}");
    assert_eq!(
        links[0].1, "[1]",
        "the anchor's w:t must concatenate to \"[1]\": {links:?}"
    );

    let c3 = c3_doc_xml("citation_truncation_gb7714");
    assert!(
        c3.contains(" CITATION "),
        "C3 should emit a CITATION field: {c3}"
    );
    let cached = c3
        .split("fldCharType=\"separate\"")
        .nth(1)
        .and_then(|s| s.split("fldCharType=\"end\"").next())
        .unwrap_or_default();
    assert!(
        cached.contains("[1]"),
        "the C3 CITATION cache must carry the complete \"[1]\": {cached}"
    );
}

#[test]
fn citation_supplement_and_multikey_keep_cached_text() {
    let xml = fixture_doc_xml("citation_supplement_and_multikey");
    let texts: Vec<String> = anchor_texts(&xml).into_iter().map(|(_, t)| t).collect();
    assert!(
        texts.iter().any(|t| t == "[1, p. 40]"),
        "supplement must stay inside the cached marker: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t == "[1], [2]"),
        "multi-key group separator must survive: {texts:?}"
    );
}

/// Every container that used to abort conversion with Typst's
/// "citation could not be located" must now convert, link its citation to
/// the entry bookmark, and cache the full marker.
#[test]
fn citations_inside_containers_resolve_and_link() {
    for (fixture, expected) in [
        ("citation_in_enum_item", &["[1]", "[2]"][..]),
        ("citation_in_list_item", &["[1]"][..]),
        ("citation_in_table_cell", &["[1]"][..]),
        ("citation_in_figure_caption", &["[1]"][..]),
        ("citation_in_heading", &["[1]"][..]),
        ("citation_in_footnote", &["[1]"][..]),
    ] {
        // Footnote bodies emit into word/footnotes.xml, not document.xml.
        let xml = if fixture == "citation_in_footnote" {
            crate::common::fixture_part(fixture, "word/footnotes.xml")
        } else {
            fixture_doc_xml(fixture)
        };
        let mut texts: Vec<String> = anchor_texts(&xml).into_iter().map(|(_, t)| t).collect();
        texts.sort();
        let mut want = expected.to_vec();
        want.sort();
        assert_eq!(
            texts, want,
            "{fixture}: in-container citation must link with the full marker"
        );
        // The entry bookmark lives in document.xml; a footnote's link
        // targets it cross-part.
        let names = bookmark_names(&fixture_doc_xml(fixture));
        for (anchor, _) in anchor_texts(&xml) {
            assert!(
                names.iter().any(|n| *n == anchor),
                "{fixture}: anchor {anchor:?} has no matching entry bookmark"
            );
        }
    }
}

#[test]
fn citations_in_paragraph_and_quote_still_resolve() {
    // Negative control: paragraph and #quote citations never needed the
    // container feed — this pins that the change did not break them.
    let xml = fixture_doc_xml("citation_in_paragraph_and_quote");
    let mut texts: Vec<String> = anchor_texts(&xml).into_iter().map(|(_, t)| t).collect();
    texts.sort();
    assert_eq!(
        texts,
        ["[1]", "[1]", "[2]"],
        "repeated cites stay live: {xml}"
    );
}

#[test]
fn bibliography_entries_carry_ieee_markers() {
    // The "[n]" marker is extracted from the entry's own prefix runs — never
    // reconstructed by position.
    let xml = fixture_doc_xml("bibliography_prefix_ieee");
    assert_numbered_entries(&xml);
}

fn assert_numbered_entries(xml: &str) {
    for (author, marker) in [("N. Kudo", "[1]"), ("J. Smith", "[2]")] {
        let entry = paragraph_containing(xml, author);
        let (label, body) = entry
            .split_once("<w:tab/>")
            .expect("marker and body have separate slots");
        assert!(label.contains(marker) && body.contains(author), "{entry}");
    }
}

#[test]
fn bibliography_entries_have_no_positional_fallback() {
    // Author-date styles produce no numeric prefix — no "[n]" may be
    // fabricated by position (the old fallback guessed "[1]").
    let xml = fixture_doc_xml("bibliography_prefix_apa");
    assert!(
        xml.contains("Kudo, N.") && xml.contains("Smith, J."),
        "formatted author-date entries expected: {xml}"
    );
    assert!(
        !xml.contains("[1]") && !xml.contains("[2]"),
        "no positional \"[n]\" marker may be invented: {xml}"
    );
}

#[test]
fn bibliography_at_c2_emits_plain_paragraphs() {
    // At C2 the bibliography degrades to literal paragraphs: no SDT and no
    // BIBLIOGRAPHY field — only C3 emits them.
    let xml = fixture_doc_xml("bibliography_prefix_ieee");
    assert!(
        !xml.contains("<w:bibliography") && !xml.contains(" BIBLIOGRAPHY "),
        "C2 must not emit the bibliography field/SDT: {xml}"
    );
    assert_numbered_entries(&xml);
}

#[test]
fn bibliography_at_c3_emits_sdt_with_field_cache() {
    let xml = c3_doc_xml("bibliography_prefix_ieee");
    assert!(
        xml.contains("<w:bibliography") && xml.contains(" BIBLIOGRAPHY "),
        "C3 should emit the bibliography SDT + BIBLIOGRAPHY field: {xml}"
    );
    assert_numbered_entries(&xml);
}

#[test]
fn ref_fields_resolve_i_figured_alias_bookmarks() {
    // i-figured's show rules produce two live labels per element — the outer
    // `<my-figure>` and the inner `<fig:my-figure>`. Both must get bookmarks
    // so REFs to either name resolve.
    let xml = fixture_doc_xml("ref_ifigured_alias");
    let names = bookmark_names(&xml);
    for want in [
        "my-figure",
        "fig:my-figure",
        "my-equation",
        "eqt:my-equation",
    ] {
        assert!(
            names.iter().any(|n| n == want),
            "missing bookmark {want:?}; found {names:?}"
        );
    }
    for instr in [" REF fig:my-figure ", " REF eqt:my-equation "] {
        assert!(xml.contains(instr), "missing {instr} in {xml}");
    }
}

#[test]
fn ref_fields_resolve_plain_label_bookmarks() {
    let xml = fixture_doc_xml("ref_plain_label");
    let names = bookmark_names(&xml);
    for want in ["chap", "myfig", "myeq"] {
        assert!(
            names.iter().any(|n| n == want),
            "missing bookmark {want:?}; found {names:?}"
        );
    }
    for instr in [" REF myfig ", " REF myeq ", " REF chap "] {
        assert!(xml.contains(instr), "missing {instr} in {xml}");
    }
}

#[test]
fn ref_field_inside_table_cell_resolves() {
    // Labels cannot be registered inside table cells (Typst rejects them),
    // but a REF emitted inside a cell must still target a top-level bookmark.
    let xml = fixture_doc_xml("ref_in_table_cell");
    assert!(
        bookmark_names(&xml).iter().any(|n| n == "outfig"),
        "missing bookmark \"outfig\": {xml}"
    );
    assert!(xml.contains(" REF outfig "), "missing REF outfig in {xml}");
}
