//! Bibliography and citation tests.

use crate::common::{
    fixture_doc_xml, fixture_doc_xml_legacy, fixture_document, fixture_outcome_with_options,
    fixture_package, fixture_package_with_options, fixture_styles_xml, paragraph_containing,
};
use std::path::PathBuf;

#[test]
fn bibliography_citations_are_markers_not_field_refs() {
    let xml = fixture_doc_xml("bibliography_basic");
    // Citations must NOT be REF fields to bibliography keys: a REF to a
    // non-existent bookmark renders in Word as "Error! Reference source not
    // found". They render instead as the marker Typst produced.
    for key in ["REF smith2020", "REF knuth1997", "REF wang2023"] {
        assert!(
            !xml.contains(key),
            "citation must not be a REF field: {key}"
        );
    }
    // The default style renders inline numeric markers ([1]/[2]/[3]).
    assert!(
        xml.contains("[1]") && xml.contains("[2]") && xml.contains("[3]"),
        "expected inline numeric citation markers: {xml}"
    );
}

#[test]
fn superscript_citation_style_raises_the_marker() {
    // Retargeted to the legacy path (step 2): the superscript half of the same Cite path as the bibliography-entries gap.
    // See docs/fallback-registry.md.
    let xml = fixture_doc_xml_legacy("bibliography_superscript");
    // A superscript numeric style (here "nature") must raise the in-text marker,
    // detected from the rendered <sup> — not assumed from the style name.
    assert!(
        !xml.contains("REF smith2020"),
        "citation must not be a broken REF field: {xml}"
    );
    assert!(
        xml.contains(r#"<w:vertAlign w:val="superscript"/>"#),
        "superscript citation style must produce a raised marker: {xml}"
    );
}

#[test]
fn bibliography_entries_are_not_a_bulleted_list() {
    // Retargeted to the legacy path (step 2): bibliography entries are a registered IR gap (not collected from fragments).
    // See docs/fallback-registry.md.
    // Regression: bibliography entries arrive as a Typst <ul>, so each kept a
    // bullet-list numPr on top of its "[n]" label — a double marker. The "[n]" is
    // the marker; entries should carry only a hanging indent, no list numbering.
    let doc_xml = fixture_doc_xml_legacy("bibliography_basic");
    let entry = paragraph_containing(&doc_xml, "An Example Article");
    assert!(
        !entry.contains("<w:numPr>"),
        "bibliography entries must not be a bulleted/numbered list:\n{entry}"
    );
    assert!(
        entry.contains("w:hanging"),
        "bibliography entries should keep a hanging indent"
    );
}

#[test]
fn bibliography_style_is_defined() {
    // The reference field-code paragraph carries the "Bibliography" style; it must
    // be defined in styles.xml, not left as a dangling reference relying on Word's
    // built-in (which WPS/LibreOffice may not have).
    let styles = fixture_styles_xml("bibliography_basic");
    assert!(
        styles.contains(r#"w:styleId="Bibliography""#),
        "styles.xml should define the Bibliography style"
    );
}

#[test]
fn bibliography_sdt_and_field_only_at_c3() {
    // C2 keeps literal paragraphs — the BIBLIOGRAPHY field + SDT only exist
    // at C3 where Word can evaluate them.
    let c2 = fixture_doc_xml("bibliography_basic");
    assert!(
        !c2.contains("<w:bibliography") && !c2.contains(" BIBLIOGRAPHY "),
        "C2 must not emit the bibliography SDT/field: {c2}"
    );
    let c3 = fixture_package_with_options(
        "bibliography_basic",
        &tydocx_core::ConvertOptions::new(tydocx_core::CompatMode::C3),
    )
    .part_text("word/document.xml")
    .to_owned();
    assert!(
        c3.contains("w:bibliography"),
        "expected w:bibliography SDT marker at C3"
    );
    assert!(
        c3.contains("BIBLIOGRAPHY"),
        "expected BIBLIOGRAPHY field code at C3"
    );
}

#[test]
fn bibliography_has_custom_xml_sources() {
    let package = fixture_package("bibliography_basic");
    assert!(
        package
            .part_names()
            .any(|name| name == "customXml/item1.xml"),
        "expected customXml/item1.xml in ZIP"
    );
    let xml = package.part_text("customXml/item1.xml");
    assert!(xml.contains("b:Sources"), "expected b:Sources root");
    assert!(xml.contains("smith2020"), "expected smith2020 tag");
    assert!(xml.contains("knuth1997"), "expected knuth1997 tag");
    assert!(xml.contains("wang2023"), "expected wang2023 tag");
    assert!(
        xml.contains("JournalArticle"),
        "expected JournalArticle source type"
    );
    assert!(
        xml.contains("Book"),
        "expected Book source type for knuth1997"
    );
}

#[test]
fn bibliography_custom_xml_has_author_metadata() {
    let package = fixture_package("bibliography_basic");
    let xml = package.part_text("customXml/item1.xml");
    assert!(
        xml.contains("<b:Last>Smith</b:Last>"),
        "expected Smith author"
    );
    assert!(xml.contains("<b:Year>2020</b:Year>"), "expected year 2020");
    assert!(xml.contains("<b:Title>"), "expected title element");
    assert!(
        xml.contains("<b:Last>Knuth</b:Last>"),
        "expected Knuth author"
    );
    assert!(xml.contains("<b:Year>1997</b:Year>"), "expected year 1997");
}

#[test]
fn bibliography_citation_sources_count() {
    let doc = fixture_document("bibliography_basic");
    assert_eq!(
        doc.citation_sources.len(),
        3,
        "expected 3 citation sources, got {}",
        doc.citation_sources.len()
    );
}

#[test]
fn bibliography_field_codes_have_begin_and_end() {
    // Field codes only exist at C3 — C2 emits literal text and hyperlinks.
    let xml = fixture_package_with_options(
        "bibliography_basic",
        &tydocx_core::ConvertOptions::new(tydocx_core::CompatMode::C3),
    )
    .part_text("word/document.xml")
    .to_owned();
    // Both inline citations and bibliography block use field codes
    assert!(
        xml.contains("fldCharType=\"begin\""),
        "expected fldChar begin in citation field"
    );
    assert!(
        xml.contains("fldCharType=\"end\""),
        "expected fldChar end in citation field"
    );
}

#[test]
fn bibliography_display_text_present() {
    let xml = fixture_doc_xml("bibliography_basic");
    // The display text for citations should appear (e.g., "[1]", "[2]", "[3]")
    assert!(xml.contains("[1]"), "expected display text [1]");
    assert!(xml.contains("[2]"), "expected display text [2]");
    assert!(xml.contains("[3]"), "expected display text [3]");
}

#[test]
fn bibliography_section_heading_present() {
    // The "Bibliography" string only exists inside the C3 bibliography SDT
    // (pStyle + sdt markers); C2's plain paragraphs carry no section markup.
    let xml = fixture_package_with_options(
        "bibliography_basic",
        &tydocx_core::ConvertOptions::new(tydocx_core::CompatMode::C3),
    )
    .part_text("word/document.xml")
    .to_owned();
    // The bibliography section heading should be present
    assert!(
        xml.contains("Bibliography"),
        "expected Bibliography heading text"
    );
}

#[test]
fn bibliography_body_text_preserved() {
    let xml = fixture_doc_xml("bibliography_basic");
    // Body text around citations should be preserved
    assert!(
        xml.contains("Introduction"),
        "expected Introduction heading"
    );
    assert!(xml.contains("Methods"), "expected Methods heading");
    assert!(
        xml.contains("methodology is sound"),
        "expected body text near citation"
    );
}

#[test]
fn multiple_bibliographies_collect_sources_from_all() {
    let doc = fixture_document("bibliography_multiple");
    let tags: Vec<&str> = doc
        .citation_sources
        .iter()
        .map(|s| s.tag.as_str())
        .collect();
    assert!(
        tags.contains(&"alpha2021"),
        "first bib's key missing: {tags:?}"
    );
    assert!(
        tags.contains(&"beta2022"),
        "second bib's key missing: {tags:?}"
    );
}

#[test]
fn multiple_bibliographies_render_two_blocks() {
    // Retargeted to the legacy path (step 2): bibliography entries are a registered IR gap (not collected from fragments).
    // See docs/fallback-registry.md.
    let doc_xml = fixture_doc_xml_legacy("bibliography_multiple");
    assert!(
        doc_xml.contains("First Library Article"),
        "first bib entry text missing"
    );
    assert!(
        doc_xml.contains("Second Library Book"),
        "second bib entry text missing"
    );
    let sdt_count = doc_xml.matches("<w:bibliography/>").count();
    assert_eq!(
        sdt_count, 2,
        "expected one bibliography SDT per #bibliography()"
    );
}

#[test]
fn duplicate_citation_key_across_bibliographies_keeps_first_occurrence() {
    // Regression: typst allows the same citation key to appear in separate
    // `#bibliography()` calls in one document (it compiles without error), but
    // the merge that combines each bibliography's hayagriva::Library used
    // `merge_library`, which is last-wins (`Library::push` -> `IndexMap::insert`
    // overwrites). That silently let a later bibliography's entry clobber an
    // earlier one's metadata for the same key. The fix keeps the first
    // (earliest-in-document) occurrence via `merge_library_keep_first`. See
    // the `bibliography_duplicate_key` fixture, which cites `dup2020` in
    // both "Part One" (bibliography A, title "First Title") and "Part Two"
    // (bibliography B, title "Second Title").
    let doc = fixture_document("bibliography_duplicate_key");
    let dup_sources: Vec<_> = doc
        .citation_sources
        .iter()
        .filter(|s| s.tag == "dup2020")
        .collect();
    assert_eq!(
        dup_sources.len(),
        1,
        "BibliographyElem::keys() yields one (label, ..) tuple per bibliography \
         that defines the key, so the naive filter_map produced one CitationSource \
         PER bibliography (same tag, same writer-derived GUID) instead of deduping \
         by tag; expected exactly one citation_sources entry for dup2020, got: \
         {dup_sources:?}"
    );
    for src in &dup_sources {
        assert_eq!(
            src.title.as_deref(),
            Some("First Title"),
            "duplicate key across bibliographies must keep the FIRST bibliography's \
             metadata (document order), not the last one's"
        );
    }
}

#[test]
fn citations_link_to_their_bibliography_entry() {
    // Retargeted to the legacy path (step 2): bibliography entries are a registered IR gap (not collected from fragments).
    // See docs/fallback-registry.md.
    // @key citations must become clickable cross-references to the matching
    // reference entry: each bib entry gets a bookmark, each in-text citation an
    // internal hyperlink (w:anchor) pointing at it. Typst's HTML already carries the
    // pairing (citation href="#loc-N" <-> entry id="loc-N"). See
    // the `bibliography_basic` fixture.
    let doc_xml = fixture_doc_xml_legacy("bibliography_basic");
    let vals = |pre: &str| -> Vec<String> {
        doc_xml
            .match_indices(pre)
            .filter_map(|(i, _)| {
                doc_xml[i + pre.len()..]
                    .split('"')
                    .next()
                    .map(str::to_string)
            })
            .collect()
    };
    let anchors = vals(r#"<w:hyperlink w:anchor=""#);
    let bookmarks: std::collections::HashSet<String> = vals(r#"w:name=""#).into_iter().collect();
    assert!(
        !anchors.is_empty(),
        "@key citations must become internal hyperlinks:\n{doc_xml}"
    );
    for a in &anchors {
        assert!(
            bookmarks.contains(a),
            "citation anchor {a:?} has no matching bibliography bookmark; bookmarks={bookmarks:?}"
        );
    }
}

#[test]
fn bibliography_field_cache_contains_formatted_entries() {
    // The BIBLIOGRAPHY field's cached result must carry the formatted entries,
    // not an empty field — the realize pairs never see them (they are
    // layout-time `Works` content), so a regression here silently empties the
    // rendered bibliography. The field exists only at C3.
    let xml = fixture_package_with_options(
        "bibliography_basic",
        &tydocx_core::ConvertOptions::new(tydocx_core::CompatMode::C3),
    )
    .part_text("word/document.xml")
    .to_owned();
    let sdt_start = xml.find("<w:bibliography/>").expect("bibliography SDT");
    let sdt_end = xml[sdt_start..]
        .find("</w:sdt>")
        .map(|i| sdt_start + i)
        .expect("SDT close");
    let sdt = &xml[sdt_start..sdt_end];
    for needle in ["An Example Article", "The Art of Computer Programming"] {
        assert!(
            sdt.contains(needle),
            "formatted entry missing from BIBLIOGRAPHY field cache: {needle}"
        );
    }
}

#[test]
fn citations_link_to_bibliography_entry_bookmarks() {
    let xml = fixture_doc_xml("bibliography_basic");
    let vals = |pre: &str| -> Vec<String> {
        xml.match_indices(pre)
            .filter_map(|(i, _)| xml[i + pre.len()..].split('"').next().map(str::to_string))
            .collect()
    };
    let anchors = vals(r#"<w:hyperlink w:anchor=""#);
    let bookmarks: std::collections::HashSet<String> = vals(r#"w:name=""#).into_iter().collect();
    assert!(
        !anchors.is_empty(),
        "citations must become internal hyperlinks:\n{xml}"
    );
    for a in &anchors {
        assert!(
            bookmarks.contains(a),
            "citation anchor {a:?} has no matching entry bookmark"
        );
    }
}

#[test]
fn bibliography_style_xsl_travels_in_the_package() {
    // `--bibliography-style` must carry the XSL bytes into the DOCX: the
    // selection points Word at the named style while the embedded copy lets
    // tools extract and install it. The cached field result keeps the CSL
    // render — that remainder is a named degradation, never silent APA.
    let style = PathBuf::from("../../tests/fixtures/test_style.xsl");
    let mut options = tydocx_core::ConvertOptions::default();
    options.bibliography_style = Some(style.to_string_lossy().into_owned());
    let outcome = fixture_outcome_with_options("bibliography_basic", &options);
    assert!(
        outcome
            .degradations
            .iter()
            .any(|d| d.construct == "bibliography-style"),
        "the unrendered cache must stay a named degradation: {:?}",
        outcome.degradations
    );
    let package = fixture_package_with_options("bibliography_basic", &options);
    let expected = std::fs::read_to_string(&style).unwrap();
    assert_eq!(
        package.part_text("customXml/item2.xml"),
        expected,
        "the XSL bytes must ship verbatim as a custom-XML item"
    );
    assert!(
        package
            .part_text("customXml/itemProps2.xml")
            .contains("XSL/Transform"),
        "itemProps2 declares the XSLT schema"
    );
    let sources = package.part_text("customXml/item1.xml");
    assert!(
        sources.contains("SelectedStyle=\"\\test_style.xsl\"")
            && sources.contains("StyleName=\"Test Style\""),
        "Sources.xml must select the requested style: {sources}"
    );
}

#[test]
fn bibliography_style_without_sources_reports_a_skip() {
    // A requested style on a document with no bibliography cannot ride
    // along — the drop is named rather than silent, as a *skip* of the
    // selection (not the cache-approximation a sourced document reports).
    let mut options = tydocx_core::ConvertOptions::default();
    options.bibliography_style = Some("../../tests/fixtures/test_style.xsl".to_string());
    let outcome = fixture_outcome_with_options("toc_test", &options);
    assert!(
        outcome
            .degradations
            .iter()
            .any(|d| d.construct == "bibliography-style"
                && matches!(d.action, tydocx_core::DegradationAction::Skipped)
                && d.message.contains("no bibliography sources")),
        "the dropped selection must be reported as a skip naming the missing sources: {:?}",
        outcome.degradations
    );
}

#[test]
fn bibliography_title_is_a_semantic_heading_block() {
    // `#bibliography`'s evaluated title lowers as a Heading1-styled block
    // before the entries, not as dropped or plain text.
    let doc_xml = fixture_doc_xml("bibliography_title");
    let title = paragraph_containing(&doc_xml, "Bibliography");
    assert!(
        title.contains(r#"w:pStyle w:val="Heading1""#),
        "the bibliography title keeps the Heading1 style: {title}"
    );
    let title_pos = doc_xml.find("Bibliography").unwrap();
    let entry_pos = doc_xml.find("An Example Article").unwrap();
    assert!(
        title_pos < entry_pos,
        "the title block precedes the entries: {doc_xml}"
    );
}
