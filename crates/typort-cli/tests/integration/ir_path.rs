//! Equivalence between the realize path (`convert`) and the legacy
//! tag-walker path (`convert_legacy`).
//!
//! The legacy path is the baseline: where it is correct, the new path's
//! `word/document.xml` must be byte-identical. Every fixture where the two
//! outputs differ is listed in `DOCUMENTED_DIFFS` with the ruling —
//! old-path bug or new-path tradeoff — and a targeted assertion instead of
//! a blanket equality check. A fixture moves from `IDENTICAL` to
//! `DOCUMENTED_DIFFS` only with that ruling written down here.

use crate::common;

/// Fixtures where the realize path must produce byte-identical
/// `word/document.xml` to the legacy path.
const IDENTICAL: &[&str] = &[
    "hello",
    "style_default",
    "style_custom_page",
    "style_custom_spacing",
    "style_indent",
    "style_justify",
    "style_heading_custom",
    "issue_deep_headings",
    "rowspan_test",
    "edge_complex_table",
    "nested_list",
    "enum_custom_start",
    "style_footnotes",
    "footnote_in_table",
    "formatted_footnote",
    "image_test",
    "svg_test",
    "figure_caption",
    "pagebreak_test",
    "style_columns",
    "header_footer_test",
    "page_numbering",
];

/// Fixtures with ruled differences, covered by
/// `ir_path_documented_diffs` (not byte-identical by design; see the
/// rulings there). Every name here must appear in that test.
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
];

/// Convert a fixture through both paths and return the two
/// `word/document.xml` strings (new path first).
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

/// The first differing line of two documents, with one line of context
/// each side, for triage output.
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

/// Fixtures with known, ruled differences. Each entry names the fixture,
/// the ruling (old-path bug or new-path tradeoff), and asserts the exact
/// expected difference — never a blanket inequality.
///
/// Rulings (verified against `~/workspace/dump-all4/`):
/// - Paged-recovery duplication (old-path bug): the legacy path re-emits
///   content from the Paged rendering that the source walk already emitted:
///   `mixed_table_borders` (centered "A borderless table 1:" with a
///   literal footnote marker), `nested_table_test` (stray "Inner AInner B"
///   paragraph), `two_ordered_lists` (centered "1.one2.two" paragraph),
///   `toc_test` (the whole TOC as centered bold literal paragraphs).
/// - Quote inline segments split into paragraphs (old-path bug):
///   `edge_blockquote` (one source paragraph becomes five, dropping
///   inter-segment spaces) and `business_report` (quote + attribution
///   split into four paragraphs).
/// - Inline-code 0.8em size dropped by legacy (old-path bug): Typst
///   renders inline raw at 0.8em; the new path emits sz 18, the legacy
///   omits it: `general_elements`, `columns_test`, `tech_doc`,
///   `style_code_blocks`.
/// - Display-equation centering (new correct): Typst centers display
///   math; the new path emits `jc center`, the legacy a bare `<w:p>`:
///   `math_test`, `aligned_equations`, `numbered_eq`, and the three
///   equation paragraphs of `complex_paper`.
/// - Grid→table (new improvement, M4): `memo`, `grid_test`.
/// - `#line` horizontal rule (M5 gap, new drops, legacy recovers):
///   `general_elements`, `memo`.
/// - Bibliography entries missing (known gap): the fragment realization
///   has no entry grid, so the new path renders citations as literal
///   markers and an empty bibliography field; the legacy renders entries
///   from the HtmlDocument: `bibliography_basic`, `bibliography_multiple`.
/// - Drawn/vector figure rasterization (known gap): the legacy rasterizes
///   the figure to PNG; the new path emits an empty paragraph with a
///   `figure-body-empty` degradation: `edge_figure_rasterized`.
/// - Legacy run-style over-emission (old-path bug, Paged span/text-keyed
///   pollution): `tech_doc` (sz 20 = body size on 41 runs; DejaVu Sans
///   Mono on the CJK "。" after code), `complex_paper` (bold on the
///   author superscripts, the ":" after a footnote, and the "[n]" digits
///   of hand-written reference markers), `style_code_blocks` (Libertinus
///   + sz 22 on body runs, compensating for a Normal font misdetected as
///   the code font — shared with the new path's styles.xml, so the new
///   body text renders in the mono font there; see fallback-registry.md).
/// - Legacy strips a real source space after an initial bold label
///   (old-path bug): `complex_paper` (" 摘要：* 本文…" etc.).
/// - Legacy redundant `jc left` on table-cell paragraphs (cosmetic):
///   `complex_paper` (4 cells).
/// - Syntax highlighting (new improvement): the new path keeps Typst's
///   highlight colors on code runs; the legacy drops them and re-adds the
///   mono font per run (the new runs inherit it from the CodeBlock style):
///   `general_elements`, `style_code_blocks`.
/// - Tight term lists (old-path bug): the legacy splits every tight term
///   item into two paragraphs (bold term + description); Typst renders
///   one paragraph per item, as the new path does: `general_elements`,
///   `business_report`.
/// - Wide term lists (cosmetic): the legacy emits an explicit zero-indent
///   pPr (`ind left="0" firstLine="0"`) on description paragraphs; the new
///   path emits no pPr there — rendering-equivalent: `terms_wide`.
fn count(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

/// Lines of `xml` with every line containing any of `needles` removed
/// (for normalized-equality checks that drop only the ruled feature).
fn without_lines<'a>(xml: &'a str, needles: &[&str]) -> Vec<&'a str> {
    xml.lines()
        .filter(|line| !needles.iter().any(|needle| line.contains(needle)))
        .collect()
}

#[test]
fn ir_path_documented_diffs() {
    // Display equations: the only difference is the new path's `jc
    // center` (plus the writer's `ind firstLine="0"` that accompanies a
    // centered paragraph on both sides in different places).
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

    // math_test: same centering, plus an empty `<w:pPr>` the writer
    // leaves on the centered equation paragraph (the legacy paragraph
    // has no pPr at all), so this one asserts counts, not normalized
    // equality.
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

    // Inline code: the new path emits the 0.8em size (sz 18); the legacy
    // drops it. Nothing else differs.
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

    // general_elements: #line (legacy-only pBdr paragraph), inline-code
    // sz 18, syntax-highlight colors (new) vs per-run DejaVu (legacy),
    // tight term items split in two by the legacy.
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
            new_xml.contains(" A systems programming language"),
            "tight term merged"
        );
        assert!(legacy_xml.contains(">A systems programming language<"));
    }

    // tech_doc: legacy over-emits sz 20 (the body size) on 41 runs and
    // DejaVu Sans Mono on the CJK "。" after a code span; the new path
    // also keeps the inline-code 0.8em size the legacy drops.
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

    // business_report: the quote is one paragraph in the new path and
    // four in the legacy; tight term items are one paragraph in the new
    // path and two in the legacy.
    {
        let (new_xml, legacy_xml) = convert_both("business_report");
        assert!(new_xml.contains("“数字化转型不是选择题，而是生存题。” —— CEO 张明"));
        assert!(!legacy_xml.contains("“数字化转型不是选择题，而是生存题。” —— CEO 张明"));
        assert!(legacy_xml.contains(">数字化转型不是选择题，而是生存题。<"));
        assert!(legacy_xml.contains(">—— CEO 张明<"));
        assert!(
            new_xml.contains(" 预计贡献营收 2000 万元"),
            "tight term merged"
        );
        assert!(legacy_xml.contains(">预计贡献营收 2000 万元<"));
    }

    // memo: #line (legacy-only pBdr) + the memo-header grid becomes a
    // table (nil borders, M4) in the new path and flat paragraphs in the
    // legacy.
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

    // complex_paper: equation centering (new, 3 more than the legacy's
    // 2 shared centered paragraphs), redundant legacy `jc left` on 4
    // table cells, 5 legacy-only bolds (2 author superscripts, the ":"
    // after a footnote, 2 hand-written "[n]" digits), and 4 source
    // spaces after bold labels that the legacy strips.
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

    // Paged-recovery duplicates (old-path bug): the legacy re-emits
    // content the source walk already emitted, centered.
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

    // Bibliography: citations are literal markers in the new path (the
    // legacy hyperlinks them) and the entries are missing (empty
    // bibliography field); the legacy renders the entries.
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

    // Drawn/vector figure: the legacy rasterizes to PNG; the new path
    // emits an empty paragraph (rasterization gap).
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

    // grid_test: the grid becomes a table (nil borders) in the new path
    // and flat paragraphs in the legacy.
    {
        let (new_xml, legacy_xml) = convert_both("grid_test");
        assert_eq!(count(&new_xml, "<w:tbl>"), 1);
        assert_eq!(count(&legacy_xml, "<w:tbl>"), 0);
        assert!(
            new_xml.contains("w:val=\"nil\""),
            "strokless grid maps to nil borders"
        );
    }

    // edge_blockquote: one paragraph with five runs in the new path,
    // five paragraphs (spaces dropped at the splits) in the legacy.
    {
        let (new_xml, legacy_xml) = convert_both("edge_blockquote");
        assert!(new_xml.contains("A second block quote with "));
        assert!(legacy_xml.contains("A second block quote with<"));
        assert!(!new_xml.contains("A second block quote with<"));
    }

    // terms_wide: the legacy emits an explicit zero-indent pPr
    // (`ind left="0" firstLine="0"`) on wide-term description paragraphs;
    // the new path emits no pPr there — rendering-equivalent, since the
    // defaults are zero indent.
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

    // style_code_blocks: inline-code 0.8em size (new), legacy body-run
    // over-emission (Libertinus + sz 22), syntax-highlight colors (new)
    // vs per-run Courier New (legacy).
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

/// Overline (M9): the IR path drops the decoration and records it under
/// C1 — the text survives, and unlike the legacy path it is NOT
/// rewritten to underline. (Under C2/C3 both paths map it to underline,
/// per `docs/compat-modes.md` §4.)
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
    assert!(
        !xml.contains("<w:u "),
        "IR path drops overline instead of rewriting it to underline like the legacy path"
    );
}
