//! Footnote and endnote tests.

use crate::common::{fixture_doc_xml, fixture_document, fixture_package, fixture_styles_xml};

#[test]
fn issue_blockquote_in_footnote_content() {
    let xml = fixture_doc_xml("issue_blockquote_in_footnote");
    assert!(
        xml.contains("w:footnoteReference"),
        "footnote reference should be present"
    );
    assert!(
        xml.contains("Another paragraph"),
        "body text should be present"
    );
}

#[test]
fn issue_footnote_with_link_refs() {
    let xml = fixture_doc_xml("issue_footnote_with_link");
    assert!(xml.contains("Introduction"), "heading should be present");
    assert!(
        xml.contains("Conclusion"),
        "second heading should be present"
    );
    let fn_count = xml.matches("w:footnoteReference").count();
    assert!(
        fn_count >= 2,
        "should have at least 2 footnotes, got {fn_count}"
    );
    assert!(
        xml.matches("w:bookmarkStart").count() >= 2,
        "cross-references should produce bookmarks"
    );
}

#[test]
fn issue_footnote_separator_has_refs() {
    let xml = fixture_doc_xml("issue_footnote_separator");
    let fn_count = xml.matches("w:footnoteReference").count();
    assert_eq!(
        fn_count, 3,
        "should have 3 footnote references, got {fn_count}"
    );
}

#[test]
fn issue_footnote_math_formatting_content() {
    let xml = fixture_doc_xml("issue_footnote_math_formatting");
    let fn_count = xml.matches("w:footnoteReference").count();
    assert_eq!(fn_count, 3, "should have 3 footnotes, got {fn_count}");
}

#[test]
fn issue_endnote_vs_footnote_refs() {
    let xml = fixture_doc_xml("issue_endnote_vs_footnote");
    let fn_count = xml.matches("w:footnoteReference").count();
    assert!(
        fn_count >= 4,
        "should have at least 4 footnote references, got {fn_count}"
    );
}

#[test]
fn issue_footnote_tab_format() {
    let xml = fixture_doc_xml("issue_footnote_tab_format");
    assert!(
        xml.contains("footnote"),
        "footnote reference should be in document"
    );
}

#[test]
fn issue_footnote_in_heading_toc() {
    let xml = fixture_doc_xml("issue_footnote_in_heading_toc");
    let heading_count = xml.matches("Heading1").count();
    assert!(
        heading_count >= 3,
        "should have 3 Heading1 styles, got {heading_count}"
    );
    assert!(xml.contains("TOC"), "should have TOC field");
    assert!(xml.contains("Introduction"), "first heading text present");
    assert!(xml.contains("Results"), "third heading text present");
}

#[test]
fn formatted_footnote_preserves_bold_and_italic() {
    let doc = fixture_document("formatted_footnote");

    // Check the footnote content runs preserve formatting
    assert!(
        !doc.footnotes.is_empty(),
        "should have at least one footnote"
    );
    let fn_content = &doc.footnotes[0].content;
    let has_bold = fn_content
        .iter()
        .any(|i| matches!(i, tydocx_ooxml::document::InlineElement::Text(r) if r.bold));
    let has_italic = fn_content
        .iter()
        .any(|i| matches!(i, tydocx_ooxml::document::InlineElement::Text(r) if r.italic));
    assert!(has_bold, "footnote content should have a bold run");
    assert!(has_italic, "footnote content should have an italic run");
}

#[test]
fn formatted_footnote_xml_has_formatting_elements() {
    let package = fixture_package("formatted_footnote");
    let fn_xml = package.part_text("word/footnotes.xml");

    assert!(
        fn_xml.contains("<w:b/>"),
        "footnotes.xml should contain <w:b/> for bold formatting"
    );
    assert!(
        fn_xml.contains("<w:i/>"),
        "footnotes.xml should contain <w:i/> for italic formatting"
    );
}

#[test]
fn footnote_text_size_is_body_size_not_marker_size() {
    // Regression for detect_footnote_size (page.rs): it took the global-minimum
    // small size, which is the superscript reference/marker size (~6.5pt), and
    // pinned FootnoteText to it. The fix measures the footnote BODY runs from the
    // Paged render (located by the semantic footnote content), giving the real
    // footnote text size (~9pt). See the `edge_footnote_size_not_marker` fixture.
    let styles = fixture_styles_xml("edge_footnote_size_not_marker");
    let block = styles
        .split(r#"w:styleId="FootnoteText""#)
        .nth(1)
        .expect("FootnoteText style present");
    let block = block.split("</w:style>").next().unwrap();
    let sz: u32 = block
        .split(r#"<w:sz w:val=""#)
        .nth(1)
        .and_then(|s| s.split('"').next())
        .and_then(|s| s.parse().ok())
        .expect("FootnoteText sz present");
    // Body is 10.5pt (sz 21); the footnote body renders ~9pt (sz 16-20); the
    // superscript marker is ~6.5pt (sz 13). The footnote style must take the
    // footnote-body size, not the marker size.
    assert!(
        (16..21).contains(&sz),
        "FootnoteText size must be the footnote body size (~9pt, sz 16-20), not the \
         superscript marker size (~6.5pt, sz 13); got sz={sz}"
    );
}

#[test]
fn features_footnote_restart_and_font_hint() {
    let package = fixture_package("complex_paper");

    // Feature 1: footnote properties are present, but numbering is
    // continuous by default — Typst's default. Per-page restart only happens
    // when the source resets the footnote counter in a page header.
    let settings_xml = package.part_text("word/settings.xml");
    assert!(
        settings_xml.contains("w:footnotePr"),
        "settings.xml should contain w:footnotePr"
    );
    assert!(
        !settings_xml.contains("eachPage"),
        "settings.xml must NOT restart numbering each page by default"
    );

    // Feature 1: sectPr also has footnote properties
    let doc_xml = package.part_text("word/document.xml");
    // The sectPr should contain footnotePr
    let sect_pr_pos = doc_xml.find("w:sectPr").expect("should have sectPr");
    let after_sect = &doc_xml[sect_pr_pos..];
    assert!(
        after_sect.contains("w:footnotePr"),
        "sectPr should contain w:footnotePr for per-section footnote restart"
    );

    // Feature 9: East Asian font hint
    let styles_xml = package.part_text("word/styles.xml");
    assert!(
        styles_xml.contains("w:hint=\"eastAsia\""),
        "styles.xml should contain w:hint=\"eastAsia\" for proper font selection"
    );
}

#[test]
fn footnote_circled_numbering_format_emitted() {
    // Regression (typst 0.15 migration): the `doc-noteref` role moved ONTO the
    // `<sup>` element (whose child is `<a>N`), so detect_footnote_format's old
    // `<sup>`-child scan never matched and the circled (①②③) footnote numbering
    // format was silently dropped — Word then rendered ①②③ as 1,2,3. fn25_test
    // declares `#set footnote(numbering: "①")`.
    let package = fixture_package("fn25_test");
    let settings_xml = package.part_text("word/settings.xml");
    assert!(
        settings_xml.contains("decimalEnclosedCircle"),
        "circled footnote numbering must emit w:numFmt=decimalEnclosedCircle, got: {settings_xml}"
    );
}

#[test]
fn footnote_per_page_restart_when_header_resets_counter() {
    // A `#set page(header: context counter(footnote).update(0))` source opts
    // into per-page footnote numbering: the writer must emit `eachPage`.
    let package = fixture_package("issue_footnote_per_page");
    let settings_xml = package.part_text("word/settings.xml");
    assert!(
        settings_xml.contains("eachPage"),
        "a header counter reset must restart numbering each page, got: {settings_xml}"
    );
    let doc_xml = package.part_text("word/document.xml");
    let sect_pr_pos = doc_xml.find("w:sectPr").expect("should have sectPr");
    assert!(
        doc_xml[sect_pr_pos..].contains("eachPage"),
        "sectPr should restart numbering each page"
    );
}

#[test]
fn footnote_continuous_by_default() {
    // No header reset anywhere: numbering is continuous, so no `eachPage`.
    let package = fixture_package("issue_footnote_continuous");
    let settings_xml = package.part_text("word/settings.xml");
    assert!(
        settings_xml.contains("w:footnotePr"),
        "settings.xml should contain w:footnotePr"
    );
    assert!(
        !settings_xml.contains("eachPage"),
        "no header reset means continuous numbering, got: {settings_xml}"
    );
}

#[test]
fn circled_footnote_marks_are_word_generated() {
    // Regression: circled footnote numbering used to be emitted as literal
    // ①②③… characters (with customMarkFollows), freezing the global
    // sequence and contradicting a per-page restart. Word now generates the
    // marks from `decimalEnclosedCircle`; the writer emits no literal mark.
    // (The recovery layer still re-inserts the Paged footnote-area text as
    // literal superscript runs in stray body paragraphs — pre-existing,
    // identical on the clean baseline, out of this batch's scope; it is why
    // this test does NOT assert "no circled char anywhere in document.xml".)
    let package = fixture_package("fn25_test");
    let doc_xml = package.part_text("word/document.xml");
    assert!(
        !doc_xml.contains("customMarkFollows"),
        "no literal custom marks should be emitted"
    );
    assert_eq!(
        doc_xml.matches("FootnoteReference").count(),
        25,
        "exactly one FootnoteReference-styled run per reference (baseline had 50: one reference run plus one literal mark run each)"
    );
    let fn_xml = package.part_text("word/footnotes.xml");
    assert!(
        !fn_xml.contains('①') && !fn_xml.contains('㉑'),
        "no literal circled characters should be emitted in footnote bodies"
    );
    assert_eq!(
        fn_xml.matches("w:footnoteRef").count(),
        25,
        "each footnote body should start with a generated mark run"
    );
}

#[test]
fn footnote_math_not_duplicated() {
    // Regression (typst 0.15 migration): collect_footnote_inlines descended into
    // the new native MathML `<math>` element and emitted the equation a SECOND
    // time as literal Mathematical-Alphanumeric glyphs in a body `<w:t>` run, on
    // top of the correct OMML. Body runs must carry no math-script glyphs (those
    // belong only in OMML `<m:t>`).
    let package = fixture_package("edge_math_in_footnote");
    let fn_xml = package.part_text("word/footnotes.xml");
    assert!(
        fn_xml.contains("<m:oMath>"),
        "footnote math should be present as OMML, got: {fn_xml}"
    );
    let mut rest = fn_xml;
    let mut leaked = false;
    while let Some(pos) = rest.find("<w:t") {
        rest = &rest[pos + 4..];
        let Some(open_end) = rest.find('>') else {
            break;
        };
        let head = &rest[..open_end];
        // a body text run is `<w:t>` or `<w:t attr…>`, not `<w:tab/>`
        if (head.is_empty() || head.starts_with(' '))
            && let Some(close) = rest[open_end + 1..].find("</w:t>")
            && rest[open_end + 1..open_end + 1 + close]
                .chars()
                .any(|c| ('\u{1D400}'..='\u{1D7FF}').contains(&c))
        {
            leaked = true;
            break;
        }
        rest = &rest[open_end..];
    }
    assert!(
        !leaked,
        "footnote math must not be duplicated as literal glyphs in a <w:t> run: {fn_xml}"
    );
}

#[test]
fn edge_math_in_footnote_preserved() {
    let package = fixture_package("edge_math_in_footnote");
    let fn_xml = package.part_text("word/footnotes.xml");

    assert!(
        fn_xml.contains("m:oMath"),
        "footnotes.xml should contain m:oMath elements for math in footnotes"
    );
}

#[test]
fn smoke_style_footnotes() {
    let package = fixture_package("style_footnotes");
    assert!(
        package.byte_len() > 100,
        "docx output should be non-trivial"
    );
}

#[test]
fn issue_footnote_hyperlink_and_image() {
    // A footnote carrying an external hyperlink and an image: the link must
    // survive as a hyperlink field (not plain text) and the image as an
    // embedded drawing with its own relationships part.
    let package = fixture_package("issue_footnote_hyperlink_image");
    let footnotes_xml = package.part_text("word/footnotes.xml");
    assert!(
        footnotes_xml.contains("HYPERLINK"),
        "footnote link must survive as a hyperlink field, got:\n{footnotes_xml}"
    );
    assert!(
        footnotes_xml.contains("https://example.com"),
        "footnote link URL must survive, got:\n{footnotes_xml}"
    );
    assert!(
        footnotes_xml.contains("the example site"),
        "footnote link display text must survive, got:\n{footnotes_xml}"
    );
    assert!(
        footnotes_xml.contains("wp:inline"),
        "footnote image must survive as an inline drawing, got:\n{footnotes_xml}"
    );
    assert!(
        footnotes_xml.contains("a:blip"),
        "footnote image drawing must reference embedded media, got:\n{footnotes_xml}"
    );
    // 3cm wide image: 1080000 EMU is the laid-out display size, not the
    // 10px intrinsic size (95250 EMU).
    assert!(
        footnotes_xml.contains(r#"cx="1080000""#),
        "footnote image must use its laid-out 3cm width, got:\n{footnotes_xml}"
    );
    let rels = package.part_text("word/_rels/footnotes.xml.rels");
    assert!(
        rels.contains("media/image"),
        "footnote relationships must reference the embedded image, got:\n{rels}"
    );
    let names: Vec<&str> = package.part_names().collect();
    assert!(
        names.iter().any(|n| n.starts_with("word/media/image")),
        "the footnote image bytes must be packaged as a media part, got: {names:?}"
    );
}

/// The `<w:footnote w:id="...">...</w:footnote>` block for one footnote in
/// footnotes.xml (ids 0 and 1 are the separator footnotes, so real footnotes
/// start at 2).
fn footnote_block<'a>(footnotes_xml: &'a str, id: u32) -> &'a str {
    let open = format!(r#"<w:footnote w:id="{id}">"#);
    let after_open = footnotes_xml
        .split(&open)
        .nth(1)
        .unwrap_or_else(|| panic!("footnote {id} missing"));
    let block = after_open
        .split("</w:footnote>")
        .next()
        .expect("footnote closes");
    block
}

#[test]
fn issue_two_footnotes_with_images_embed_their_own_image() {
    // Two footnotes, each with a different image at a different size: the
    // first footnote's drawing must embed rId1 (red, 2cm) and the second
    // footnote's rId2 (blue, 4cm). The image ordinal counts across footnotes,
    // so a later footnote must not embed the first image again.
    let package = fixture_package("issue_footnote_two_images");
    let footnotes_xml = package.part_text("word/footnotes.xml");
    let first = footnote_block(&footnotes_xml, 2);
    assert!(
        first.contains(r#"r:embed="rId1""#),
        "first footnote must embed rId1, got:\n{first}"
    );
    assert!(
        first.contains(r#"cx="720000""#),
        "first footnote image must use its laid-out 2cm width, got:\n{first}"
    );
    let second = footnote_block(&footnotes_xml, 3);
    assert!(
        second.contains(r#"r:embed="rId2""#),
        "second footnote must embed rId2, not rId1, got:\n{second}"
    );
    assert!(
        second.contains(r#"cx="1440000""#),
        "second footnote image must use its laid-out 4cm width, got:\n{second}"
    );
    let rels = package.part_text("word/_rels/footnotes.xml.rels");
    assert!(
        rels.contains(r#"Id="rId1""#) && rels.contains("media/image1.png"),
        "rId1 must point at the first footnote image, got:\n{rels}"
    );
    assert!(
        rels.contains(r#"Id="rId2""#) && rels.contains("media/image2.png"),
        "rId2 must point at the second footnote image, got:\n{rels}"
    );
    let names: Vec<&str> = package.part_names().collect();
    assert!(
        names.iter().any(|n| *n == "word/media/image1.png"),
        "the first footnote image bytes must be packaged, got: {names:?}"
    );
    assert!(
        names.iter().any(|n| *n == "word/media/image2.png"),
        "the second footnote image bytes must be packaged, got: {names:?}"
    );
}

#[test]
fn issue_fn_paren_numbering_c1_reports_and_keeps_body() {
    let outcome = crate::common::fixture_outcome_with_options(
        "issue_fn_paren_numbering",
        &tydocx_core::ConvertOptions::new(tydocx_core::CompatMode::C1),
    );
    assert!(
        outcome
            .degradations
            .iter()
            .any(|d| d.construct == "footnote-numbering"),
        "expected a footnote-numbering degradation, got {:?}",
        outcome.degradations
    );
    let package = crate::common::fixture_package_with_options(
        "issue_fn_paren_numbering",
        &tydocx_core::ConvertOptions::new(tydocx_core::CompatMode::C1),
    );
    assert!(
        package
            .part_text("word/document.xml")
            .contains("w:footnoteReference"),
        "the marker degrades to a decimal footnote reference"
    );
    assert!(
        package
            .part_text("word/footnotes.xml")
            .contains("Parenthesized footnote body."),
        "the footnote body text survives"
    );
}

#[test]
fn issue_fn_semibold_weight_emits_bold_in_footnotes_part() {
    let package = fixture_package("issue_fn_semibold_weight");
    let fn_xml = package.part_text("word/footnotes.xml");
    let pos = fn_xml
        .find("semibold footnote body")
        .expect("footnote body present");
    let run_start = fn_xml[..pos].rfind("<w:r>").unwrap_or(0);
    let run_end = fn_xml[pos..]
        .find("</w:r>")
        .map_or(fn_xml.len(), |e| pos + e);
    let run = &fn_xml[run_start..run_end];
    assert!(
        run.contains("<w:b/>"),
        "a weight-630 footnote run must serialize as <w:b/>:\n{run}"
    );
}

#[test]
fn footnote_entry_par_show_rule_pins_indent_and_justify() {
    // `#show footnote.entry: set par(first-line-indent: 0pt, justify: false)`
    // is entry-scoped: the footnote paragraphs must pin both values or they
    // inherit Normal's 2em indent and `both` justification via FootnoteText.
    let package = fixture_package("footnote_entry_par");
    let fn_xml = package.part_text("word/footnotes.xml");
    let entry = footnote_block(&fn_xml, 2);
    assert!(
        entry.contains(r#"<w:ind w:firstLine="0"/>"#),
        "the entry pins firstLine=0 against Normal's indent:\n{entry}"
    );
    assert!(
        entry.contains(r#"<w:jc w:val="left"/>"#),
        "the entry pins jc=left against Normal's both:\n{entry}"
    );
}
