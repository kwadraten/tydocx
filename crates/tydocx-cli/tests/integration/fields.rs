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
        "fields_styleref_mixed",
        "fields_styleref_footer",
        "fields_enum_seq",
        "fields_enum_seq_degraded",
        "fields_seq",
        "fields_seq_multi",
        "fields_hyperlink",
        "fields_page_counters",
        "fields_ref_marker",
        "bibliography_basic",
        "columns_two",
        "columns_three_gutter",
        "columns_mid_page",
        "columns_page_break",
        "columns_page_default_gutter",
        "columns_page_set_gutter",
        "columns_page_ratio_gutter",
        "columns_nested",
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
    for picture in [
        "yyyy-MM-dd",
        "yyyy/MM/dd",
        "MMMM d, yy",
        "MMM d, yyyy",
        "dddd, d MMMM",
    ] {
        assert!(
            c3.contains(&format!("\\@ &quot;{picture}&quot;")),
            "C3 should emit a DATE \\@ {picture} switch: {c3}"
        );
    }
    // The unmappable `[ordinal]` pattern and the computed pattern emit bare
    // DATE fields — the switch is dropped, not guessed.
    assert_eq!(
        c3.matches(" DATE ").count() - c3.matches(" DATE \\@").count(),
        2,
        "two bare DATE fields (ordinal, computed): {c3}"
    );
    let c2 = part("fields_datetime", CompatMode::C2, "word/document.xml");
    assert!(!c2.contains(" DATE "), "C2 stays literal: {c2}");
    assert_eq!(
        visible_text(&c2),
        visible_text(&c3),
        "the explicit-format date keeps its C2 literal as the cached result"
    );
    // A fixed `datetime(...)` is already a fixed value — it must stay a
    // literal, never a recomputing field.
    for literal in ["2026-03-04", "14:30:00"] {
        assert!(
            c3.contains(literal),
            "the fixed datetime's literal survives: {c3}"
        );
    }
    assert!(
        !c3.contains(" TIME "),
        "a fixed time must not become a recomputing TIME field: {c3}"
    );
}

#[test]
fn unmappable_and_computed_date_patterns_report_a_degradation() {
    let outcome =
        fixture_outcome_with_options("fields_datetime", &ConvertOptions::new(CompatMode::C3));
    let switches: Vec<&str> = outcome
        .degradations
        .iter()
        .filter(|d| d.construct == "date-format-switch")
        .map(|d| d.message.as_str())
        .collect();
    assert_eq!(
        switches.len(),
        2,
        "the ordinal component and the computed pattern each report: {switches:?}"
    );
    assert!(
        switches.iter().any(|m| m.contains("ordinal")),
        "the unmappable component is named: {switches:?}"
    );
    assert!(
        switches.iter().any(|m| m.contains("computed")),
        "the computed pattern is named: {switches:?}"
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
    assert!(
        c3.contains(" DOCPROPERTY CreateDate \\@ &quot;yyyy&quot; "),
        "the explicit [year] pattern becomes an \\@ switch: {c3}"
    );
    assert!(
        c3.contains(" DOCPROPERTY CreateDate \\@ &quot;yyyy-MM-dd&quot; "),
        "the auto display on a date-only value becomes yyyy-MM-dd: {c3}"
    );
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

/// The page-counter fields carry the *paged* values as cached results —
/// the realize path has no pages, so the paged patch rewrites every cached
/// literal to Typst's own logical page numbers and numbering.
#[test]
fn page_counter_fields_cache_the_paged_values() {
    let c3 = part("fields_page_counters", CompatMode::C3, "word/document.xml");
    for instruction in [" PAGE ", " NUMPAGES ", "PAGEREF start"] {
        assert!(
            c3.contains(instruction),
            "C3 should emit {instruction:?}: {c3}"
        );
    }
    let text = visible_text(&c3);
    for expected in [
        "Current: iii",
        "Upper: III",
        "Back to start: i",
        "Both: 3 / 3",
        "Total: (3,)",
        "Start state: 1",
    ] {
        assert!(
            text.contains(expected),
            "the paged literal {expected:?} should be the cached result: {text}"
        );
    }
    // Typst has no per-section total: the `#set page(numbering: "i")`
    // section keeps `counter(page).final()` whole-document, so NUMPAGES is
    // the only faithful field — no SECTIONPAGES, ever.
    assert!(
        !c3.contains("SECTIONPAGES"),
        "Typst has no per-section page total: {c3}"
    );
    let c2 = part("fields_page_counters", CompatMode::C2, "word/document.xml");
    assert_eq!(
        visible_text(&c2),
        visible_text(&c3),
        "the cached results must reproduce the C2 literal"
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

/// A mixed header is `literal + query result`: only the query-result span is
/// inside the field, so a Word field update keeps the literal.
#[test]
fn mixed_heading_query_header_wraps_only_the_query_result() {
    let header = part("fields_styleref_mixed", CompatMode::C3, "word/header1.xml");
    assert!(
        header.contains(" STYLEREF &quot;Heading 1&quot; "),
        "C3 should emit STYLEREF for the mixed header: {header}"
    );
    let literal_pos = header.find("Report — ").expect("literal run present");
    let field_pos = header
        .find(" STYLEREF ")
        .expect("STYLEREF instruction present");
    assert!(
        literal_pos < field_pos,
        "the literal runs stay outside the field: {header}"
    );
    assert!(
        header.contains(">Intro<"),
        "the field's cached result is the query result alone: {header}"
    );
    // Page 1 has no preceding heading — the else branch renders bare
    // `Report`, which stays literal.
    let first = part("fields_styleref_mixed", CompatMode::C3, "word/header3.xml");
    assert!(
        !first.contains(" STYLEREF ") && visible_text(&first) == "Report",
        "the else-branch header is literal: {first}"
    );
    let outcome = fixture_outcome_with_options(
        "fields_styleref_mixed",
        &ConvertOptions::new(CompatMode::C3),
    );
    assert!(
        outcome
            .degradations
            .iter()
            .any(|d| d.construct == "styleref-mixed" && d.message.contains("header")),
        "the unwrappable page reports styleref-mixed: {:?}",
        outcome.degradations
    );
}

#[test]
fn heading_query_footer_emits_styleref() {
    let footer = part("fields_styleref_footer", CompatMode::C3, "word/footer1.xml");
    assert!(
        footer.contains(" STYLEREF &quot;Heading 1&quot; "),
        "C3 should emit STYLEREF for the heading-query footer: {footer}"
    );
    assert!(
        footer.contains("> — end<"),
        "the suffix literal stays outside the field: {footer}"
    );
    assert!(
        footer.contains(">Intro<"),
        "the field's cached result is the query result: {footer}"
    );
    let c2 = part("fields_styleref_footer", CompatMode::C2, "word/footer1.xml");
    assert!(
        !c2.contains(" STYLEREF "),
        "C2 keeps the footer literal: {c2}"
    );
    assert_eq!(visible_text(&c2), visible_text(&footer));
}

/// The unguarded mixed header is a Typst compile error (`array is empty` on
/// the page without a preceding heading) — the conversion must fail with
/// that diagnostic, matching stock Typst, not silently drop the paged layer.
#[test]
fn paged_compile_failure_surfaces_the_typst_diagnostic() {
    let world =
        tydocx_core::TydocxWorld::new(&crate::common::fixture_path("paged_compile_failure"))
            .unwrap();
    let errors = tydocx_core::convert(&world, &ConvertOptions::new(CompatMode::C3))
        .expect_err("the unguarded header fails stock Typst too");
    assert!(
        errors
            .iter()
            .any(|e| e.message.as_str() == "array is empty"),
        "the Typst diagnostic is surfaced: {errors:?}"
    );
}

/// `datetime.today()` carries no time, so a time-only display pattern is a
/// Typst compile error — the conversion must surface it, matching stock
/// Typst. `IrField::Time` stays an unreachable forward hook until Typst
/// gains a current-time API.
#[test]
fn time_only_display_on_today_surfaces_the_typst_diagnostic() {
    let world =
        tydocx_core::TydocxWorld::new(&crate::common::fixture_path("fields_time_error")).unwrap();
    let errors = tydocx_core::convert(&world, &ConvertOptions::new(CompatMode::C3))
        .expect_err("a time-only display on today() fails stock Typst too");
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("insufficient information")),
        "the Typst diagnostic is surfaced: {errors:?}"
    );
}

/// A function-numbered enum (`n => "§" + str(n) + "."`) is literal at C2;
/// at C3 the numeral is spliced into a `SEQ enum-N` field — the §/. affixes
/// stay literal, identifiers are per-enum, and `start:` resets the first
/// SEQ with `\r`.
#[test]
fn function_numbered_enum_emits_seq_fields() {
    let c2 = part("fields_enum_seq", CompatMode::C2, "word/document.xml");
    let c3 = part("fields_enum_seq", CompatMode::C3, "word/document.xml");
    assert!(
        !c2.contains("w:numPr"),
        "function-numbered enums are literal, never decimalized: {c2}"
    );
    assert_eq!(
        c3.matches(" SEQ enum-1 ").count(),
        4,
        "the first enum's items share one SEQ identifier: {c3}"
    );
    assert_eq!(
        c3.matches(" SEQ enum-2 ").count(),
        2,
        "the second enum uses a distinct SEQ identifier: {c3}"
    );
    assert!(
        c3.contains(" SEQ enum-2 \\r 4 "),
        "`enum(start: 4)` resets the first SEQ of its enum: {c3}"
    );
    let text = visible_text(&c3);
    for expected in [
        "§1. First item",
        "§2. Second item",
        "§3. Other first",
        "§4. Other second",
        "§4. Restarted a",
        "§5. Restarted b",
    ] {
        assert!(
            text.contains(expected),
            "the C2 literal {expected:?} is the cached result: {text}"
        );
    }
    assert_eq!(
        visible_text(&c2),
        text,
        "the SEQ cached results reproduce the C2 literals"
    );
}

/// The SEQ splice's boundaries — nested literal enums, `number:` overrides,
/// and reversed enums — stay literal and report.
#[test]
fn enum_seq_unexpressible_items_stay_literal_and_report() {
    let outcome = fixture_outcome_with_options(
        "fields_enum_seq_degraded",
        &ConvertOptions::new(CompatMode::C3),
    );
    let c3 = fixture_package_with_options(
        "fields_enum_seq_degraded",
        &ConvertOptions::new(CompatMode::C3),
    )
    .part_text("word/document.xml")
    .to_owned();
    let text = visible_text(&c3);
    for expected in [
        "§1. Inner a",
        "§2. Inner b",
        "§7. Overridden",
        "§3. Back a",
        "§2. Back b",
        "§1. Back c",
    ] {
        assert!(
            text.contains(expected),
            "unexpressible item stays literal {expected:?}: {text}"
        );
    }
    assert!(
        c3.contains(" SEQ enum-1 \\r 8 "),
        "the item after a literal `number:` override resets the SEQ: {c3}"
    );
    for construct in ["enum-seq-nested", "enum-seq-override", "enum-reversed"] {
        assert!(
            outcome
                .degradations
                .iter()
                .any(|d| d.construct == construct),
            "missing {construct} degradation: {:?}",
            outcome.degradations
        );
    }
    let c2_outcome = fixture_outcome_with_options(
        "fields_enum_seq_degraded",
        &ConvertOptions::new(CompatMode::C2),
    );
    assert!(
        c2_outcome
            .degradations
            .iter()
            .any(|d| d.construct == "enum-reversed"),
        "reversed enums report at C2 too: {:?}",
        c2_outcome.degradations
    );
    let c2 = part(
        "fields_enum_seq_degraded",
        CompatMode::C2,
        "word/document.xml",
    );
    assert_eq!(visible_text(&c2), text);
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

/// The SEQ guard is the *rendered* numeral's arity: a `"1.1"`-pattern
/// figure renders a single-piece `1` (a figure counter is single-level), so
/// it gets a SEQ field exactly like a single-piece pattern.
#[test]
fn multi_piece_pattern_with_single_piece_numeral_emits_seq() {
    let c3 = part("fields_seq_multi", CompatMode::C3, "word/document.xml");
    for instruction in [" SEQ Figure ", " SEQ equation "] {
        assert!(
            c3.contains(instruction),
            "C3 should emit {instruction:?}: {c3}"
        );
    }
    let c2 = part("fields_seq_multi", CompatMode::C2, "word/document.xml");
    assert!(!c2.contains(" SEQ "), "C2 keeps the numbers literal: {c2}");
    let text = visible_text(&c3);
    for expected in ["Figure\u{a0}1: A figure", "(1)"] {
        assert!(
            text.contains(expected),
            "the C2 literal {expected:?} is the cached result: {text}"
        );
    }
    assert_eq!(visible_text(&c2), text);
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

/// `next_marker_text` concatenates every `TextElem` of a ref's rendered
/// marker up to the tag's `End` — a `[Section ]`/`[Chapter ]` supplement
/// prefix plus the number is one cached result; taking only the first run
/// would silently drop the rest.
#[test]
fn ref_supplement_marker_caches_the_full_text() {
    for mode in [CompatMode::C2, CompatMode::C3] {
        let doc = part("fields_ref_marker", mode, "word/document.xml");
        assert_eq!(
            doc.matches(" REF intro ").count(),
            2,
            "{mode:?}: both refs emit REF fields against the heading bookmark: {doc}"
        );
        let cached: Vec<&str> = doc
            .split("fldCharType=\"separate\"")
            .skip(1)
            .map(|s| s.split("fldCharType=\"end\"").next().unwrap_or_default())
            .collect();
        for expected in ["Section\u{a0}1", "Chapter\u{a0}1"] {
            assert!(
                cached.iter().any(|s| s.contains(expected)),
                "{mode:?}: supplement prefix + number is one cached result {expected:?}: {cached:?}"
            );
        }
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
