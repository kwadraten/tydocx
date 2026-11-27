//! C1 mode gates (Phase 1): each dropped/approximated aspect emits a
//! `Degradation` with a stable `construct` identifier, and C2/C3 report no
//! degradations for them. C2 output is byte-identical to the Phase 0
//! baseline; C3 adds Word fields whose cached results equal that baseline's
//! text. The default options are C2.

use crate::common::{
    fixture_outcome_with_options, fixture_package, fixture_package_with_options, paragraph_texts,
};
use tydocx::{CompatMode, ConvertOptions};

/// The `construct` identifiers C1 reported for a fixture, in order.
fn c1_constructs(fixture: &str) -> Vec<String> {
    fixture_outcome_with_options(fixture, &ConvertOptions::new(CompatMode::C1))
        .degradations
        .iter()
        .map(|d| d.construct.to_string())
        .collect()
}

/// The `construct` identifiers a mode reported for a fixture.
fn constructs_for(fixture: &str, mode: CompatMode) -> Vec<String> {
    fixture_outcome_with_options(fixture, &ConvertOptions::new(mode))
        .degradations
        .iter()
        .map(|d| d.construct.to_string())
        .collect()
}

fn assert_has(constructs: &[String], construct: &str) {
    assert!(
        constructs.iter().any(|c| c == construct),
        "expected a {construct:?} degradation, got {constructs:?}"
    );
}

fn assert_clean(fixture: &str) {
    for mode in [CompatMode::C2, CompatMode::C3] {
        let constructs = constructs_for(fixture, mode);
        assert!(
            constructs.is_empty(),
            "{fixture} under {mode:?} should report no degradations, got {constructs:?}"
        );
    }
}

#[test]
fn c3_cached_results_match_the_c2_literal() {
    // C1 legitimately differs on gated fixtures (see the tests below). The
    // default is C2, so the byte-identical invariant covers default/C2 only;
    // C3 wraps dynamic values in fields whose cached results are the C2
    // literal text (spec D1).
    let default = fixture_package_with_options("complex_paper", &ConvertOptions::default())
        .part_text("word/document.xml")
        .to_owned();
    let c2 = fixture_package_with_options("complex_paper", &ConvertOptions::new(CompatMode::C2))
        .part_text("word/document.xml")
        .to_owned();
    assert_eq!(
        default, c2,
        "the default mode is C2 and must convert identically"
    );
    let c3 = fixture_package_with_options("complex_paper", &ConvertOptions::new(CompatMode::C3))
        .part_text("word/document.xml")
        .to_owned();
    assert_eq!(
        paragraph_texts(&c2),
        paragraph_texts(&c3),
        "C3 field cached results must equal the C2 literal text"
    );
}

#[test]
fn c2_and_c3_report_no_degradations() {
    for mode in [CompatMode::C2, CompatMode::C3] {
        let outcome = fixture_outcome_with_options("complex_paper", &ConvertOptions::new(mode));
        assert!(
            outcome.degradations.is_empty(),
            "mode {mode:?} should report no degradations, got {:?}",
            outcome.degradations
        );
    }
}

#[test]
fn c1_overline_drops_decoration() {
    let constructs = c1_constructs("conformance/conf-overline");
    assert_has(&constructs, "overline");
    assert_clean("conformance/conf-overline");
    // C2 draws the overline as a floating line shape; the only `<w:u ` in
    // either mode is the fixture's real underline.
    let c1 = fixture_package_with_options(
        "conformance/conf-overline",
        &ConvertOptions::new(CompatMode::C1),
    )
    .part_text("word/document.xml")
    .to_owned();
    let c2 = fixture_package_with_options(
        "conformance/conf-overline",
        &ConvertOptions::new(CompatMode::C2),
    )
    .part_text("word/document.xml")
    .to_owned();
    assert_eq!(
        c1.matches("<w:u ").count(),
        1,
        "C1 should underline only the real underline"
    );
    assert_eq!(
        c2.matches("<w:u ").count(),
        1,
        "C2 underlines only the real underline; the overline is a floating line"
    );
    assert!(
        c2.contains("<wp:anchor"),
        "C2 should emit the overline as an anchored shape"
    );
}

#[test]
fn c2_c3_overline_draws_line_above_text() {
    for mode in [CompatMode::C2, CompatMode::C3] {
        let xml = fixture_package_with_options(
            "conformance/conf-overline-line",
            &ConvertOptions::new(mode),
        )
        .part_text("word/document.xml")
        .to_owned();
        assert!(
            xml.contains("<wp:anchor"),
            "{mode:?}: overline should emit an anchored shape: {xml}"
        );
        assert!(
            xml.contains(r#"xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing""#),
            "{mode:?}: the wp: prefix used by the anchor must be declared: {xml}"
        );
        assert!(
            xml.contains(r#"relativeFrom="line""#),
            "{mode:?}: the shape is positioned relative to the text line: {xml}"
        );
        assert!(
            xml.contains(r#"prstGeom prst="line""#),
            "{mode:?}: the shape is a line: {xml}"
        );
        assert!(
            xml.contains("<wp:posOffset>1306527</wp:posOffset>"),
            "{mode:?}: measured page-x of the overline: {xml}"
        );
        assert!(
            xml.contains("<wp:posOffset>18999</wp:posOffset>"),
            "{mode:?}: measured line-relative y, just below the ascender top: {xml}"
        );
        assert!(
            xml.contains(r#"cx="789864""#),
            "{mode:?}: measured length of the overline: {xml}"
        );
        assert!(
            xml.contains(r#"<a:srgbClr val="000000""#),
            "{mode:?}: a colorless overline strokes in black: {xml}"
        );
        // The vertical offset is measured from Typst's own deco line and is
        // small (near the top of the line box): a real above-text position,
        // not an underline.
        assert!(
            xml.contains("overlined text"),
            "{mode:?}: overlined text should survive: {xml}"
        );
        assert_eq!(
            xml.matches("<w:u ").count(),
            0,
            "{mode:?}: nothing is underlined in this fixture: {xml}"
        );
        let constructs = constructs_for("conformance/conf-overline-line", mode);
        assert!(
            constructs.is_empty(),
            "{mode:?}: a measured overline reports no degradations, got {constructs:?}"
        );
    }
    let c1 = fixture_package_with_options(
        "conformance/conf-overline-line",
        &ConvertOptions::new(CompatMode::C1),
    )
    .part_text("word/document.xml")
    .to_owned();
    assert!(
        !c1.contains("<wp:anchor"),
        "C1 should not emit the anchored overline shape"
    );
    assert!(
        c1.contains("overlined text"),
        "C1 keeps the overlined text: {c1}"
    );
    let constructs = c1_constructs("conformance/conf-overline-line");
    assert_has(&constructs, "overline");
}

#[test]
fn c2_overline_position_tracks_text_offset() {
    let xml = fixture_package("conformance/conf-overline-offset")
        .part_text("word/document.xml")
        .to_owned();
    assert!(
        xml.contains("<wp:posOffset>1620000</wp:posOffset>"),
        "a leading #h(2cm) places the line at margin + 2cm (1620000 EMU): {xml}"
    );
    assert!(
        xml.contains("<wp:posOffset>18999</wp:posOffset>"),
        "the line-relative y is unchanged by the horizontal shift: {xml}"
    );
}

#[test]
fn c2_overline_split_segments_each_get_an_anchor() {
    let xml = fixture_package("conformance/conf-overline-split")
        .part_text("word/document.xml")
        .to_owned();
    assert_eq!(
        xml.matches("<wp:anchor").count(),
        3,
        "each evade-split segment emits its own anchored line: {xml}"
    );
}

#[test]
fn c2_overline_in_footnote_declares_drawing_namespaces() {
    let package = fixture_package("conformance/conf-overline-footnote");
    let footnotes = package.part_text("word/footnotes.xml");
    assert!(
        footnotes.contains("<wp:anchor"),
        "the footnote overline should emit an anchored shape: {footnotes}"
    );
    assert!(
        footnotes.contains(
            r#"xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing""#
        ),
        "footnotes.xml must declare the wp: prefix its anchor uses: {footnotes}"
    );
}

#[test]
fn c1_enum_numbering_falls_back_to_decimal() {
    let constructs = c1_constructs("issue_enum_numbering_patterns");
    assert_has(&constructs, "enum-numbering");
    assert_clean("issue_enum_numbering_patterns");
    // C1 keeps the shared decimal definition: its `lvlOverride`s carry only
    // the `startOverride` (the list still starts at 1); the full `w:lvl`
    // (numFmt + lvlText) belongs to the dropped pattern override.
    let c1 = fixture_package_with_options(
        "issue_enum_numbering_patterns",
        &ConvertOptions::new(CompatMode::C1),
    )
    .part_text("word/numbering.xml")
    .to_owned();
    let c2 = fixture_package_with_options(
        "issue_enum_numbering_patterns",
        &ConvertOptions::new(CompatMode::C2),
    )
    .part_text("word/numbering.xml")
    .to_owned();
    fn lvl_override_blocks(xml: &str) -> Vec<&str> {
        let mut blocks = Vec::new();
        let mut rest = xml;
        while let Some(start) = rest.find("<w:lvlOverride") {
            let end = rest[start..]
                .find("</w:lvlOverride>")
                .expect("lvlOverride end")
                + "</w:lvlOverride>".len();
            blocks.push(&rest[start..start + end]);
            rest = &rest[start + end..];
        }
        blocks
    }
    let c1_blocks = lvl_override_blocks(&c1);
    assert!(
        !c1_blocks.is_empty(),
        "C1 should still emit start-only lvlOverrides"
    );
    for block in &c1_blocks {
        assert!(
            !block.contains("<w:lvl "),
            "C1 lvlOverride must not carry a level definition: {block}"
        );
    }
    assert!(
        lvl_override_blocks(&c2)
            .iter()
            .any(|block| block.contains("<w:lvl ")),
        "C2 should emit level definitions in lvlOverrides"
    );
}

#[test]
fn c1_table_cell_fill_dropped() {
    let constructs = c1_constructs("issue_table_cell_fill");
    assert_has(&constructs, "table-cell-fill");
    assert_clean("issue_table_cell_fill");
    // C1 emits no cell shading; C2 emits three (`w:shd` in document.xml).
    let c1 = fixture_package_with_options(
        "issue_table_cell_fill",
        &ConvertOptions::new(CompatMode::C1),
    )
    .part_text("word/document.xml")
    .to_owned();
    let c2 = fixture_package_with_options(
        "issue_table_cell_fill",
        &ConvertOptions::new(CompatMode::C2),
    )
    .part_text("word/document.xml")
    .to_owned();
    assert_eq!(c1.matches("<w:shd ").count(), 0, "C1 should shade no cells");
    assert_eq!(
        c2.matches("<w:shd ").count(),
        3,
        "C2 should shade three cells"
    );
}

#[test]
fn c1_svg_image_rasterized() {
    let constructs = c1_constructs("conformance/conf-svg-image");
    assert_has(&constructs, "vector-image");
    assert_clean("conformance/conf-svg-image");
}

#[test]
fn c1_grid_layout_dropped() {
    let constructs = c1_constructs("conformance/conf-grid");
    assert_has(&constructs, "grid-layout");
    assert_clean("conformance/conf-grid");
    // The cell text survives as paragraphs; only the layout is lost.
    let xml = fixture_package_with_options(
        "conformance/conf-grid",
        &ConvertOptions::new(CompatMode::C1),
    )
    .part_text("word/document.xml")
    .to_owned();
    let texts = paragraph_texts(&xml).join(" ");
    for cell in [
        "Cell A1", "Cell B1", "Cell C1", "Cell A2", "Cell B2", "Cell C2",
    ] {
        assert!(
            texts.contains(cell),
            "grid cell text {cell:?} missing: {texts}"
        );
    }
}

#[test]
fn c1_pure_shapes_dropped() {
    let constructs = c1_constructs("conformance/conf-shapes");
    assert_has(&constructs, "shape");
    assert_clean("conformance/conf-shapes");
    // The text inside the rect survives; only the shape itself is lost.
    let xml = fixture_package_with_options(
        "conformance/conf-shapes",
        &ConvertOptions::new(CompatMode::C1),
    )
    .part_text("word/document.xml")
    .to_owned();
    assert!(
        paragraph_texts(&xml)
            .join(" ")
            .contains("Text inside a box"),
        "rect text should survive"
    );
}

#[test]
fn c1_section_break_kept_single() {
    let constructs = c1_constructs("issue_c1_section_columns");
    assert_has(&constructs, "section-break");
    assert_clean("issue_c1_section_columns");
}

#[test]
fn c1_header_variant_kept_first() {
    let constructs = c1_constructs("issue_c1_header_variant");
    assert_has(&constructs, "header-footer-variant");
    assert_clean("issue_c1_header_variant");
    // Only page 1's header survives, in both modes (the gate only reports it).
    let header = fixture_package_with_options(
        "issue_c1_header_variant",
        &ConvertOptions::new(CompatMode::C1),
    )
    .part_text("word/header1.xml")
    .to_owned();
    assert!(header.contains("First header"), "header was: {header}");
    assert!(!header.contains("Second header"), "header was: {header}");
}

#[test]
fn c1_header_weight_kept() {
    assert_clean("issue_c1_header_style");
    let header = fixture_package_with_options(
        "issue_c1_header_style",
        &ConvertOptions::new(CompatMode::C1),
    )
    .part_text("word/header1.xml")
    .to_owned();
    assert!(
        header.contains("<w:b/>"),
        "the bold header run emits w:b, header was: {header}"
    );
}

#[test]
fn c1_footnote_numbering_literalized() {
    let constructs = c1_constructs("issue_c1_footnote_numbering");
    assert_has(&constructs, "footnote-numbering");
    assert_clean("issue_c1_footnote_numbering");
}

#[test]
fn c1_table_gutter_dropped() {
    let constructs = c1_constructs("issue_c1_table_gutter");
    assert_has(&constructs, "table-gutter");
    assert_clean("issue_c1_table_gutter");
}

#[test]
fn c1_misc_gates() {
    let constructs = c1_constructs("issue_c1_misc");
    for construct in [
        "figure-placement",
        "list-marker",
        "enum-reversed",
        "image-fit",
        "image-alt",
    ] {
        assert_has(&constructs, construct);
    }
    // The figure-targeted outline resolves its entries and emits a `\c`
    // field, so no outline degradation survives in any mode — and the
    // `indent: 2em` outline is an explicit relative indent, which is
    // honoured exactly.
    assert!(
        !constructs.iter().any(|c| c.starts_with("outline-")),
        "resolved outlines report nothing: {constructs:?}"
    );
    for mode in [CompatMode::C2, CompatMode::C3] {
        // `enum-reversed` is literal at every level and still reports it —
        // nothing else remains.
        assert_eq!(
            constructs_for("issue_c1_misc", mode),
            vec!["enum-reversed"],
            "{mode:?} should report only the enum-reversed remainder"
        );
    }
}
