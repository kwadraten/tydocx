//! C1 mode gates (Phase 1): each dropped/approximated aspect emits a
//! `Degradation` with a stable `construct` identifier, and C2/C3 output is
//! unchanged from the Phase 0 baseline. The default options are C2.

use crate::common::{fixture_outcome_with_options, fixture_package_with_options, paragraph_texts};
use typort_core::{CompatMode, ConvertOptions};

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
fn c2_c3_and_default_convert_identically() {
    // C1 legitimately differs on gated fixtures (see the tests below), so
    // the identical-output invariant now covers default/C2/C3 only.
    let modes = [
        ConvertOptions::default(),
        ConvertOptions::new(CompatMode::C2),
        ConvertOptions::new(CompatMode::C3),
    ];
    let first = fixture_package_with_options("complex_paper", &modes[0])
        .part_text("word/document.xml")
        .to_owned();
    for options in &modes[1..] {
        let package = fixture_package_with_options("complex_paper", options);
        let xml = package.part_text("word/document.xml");
        assert_eq!(
            first, xml,
            "mode {:?} must convert identically to the default",
            options.compat
        );
    }
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
    // C2 maps overline to underline (2 `<w:u ` runs); C1 keeps only the
    // real underline.
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
        2,
        "C2 maps overline to underline as well"
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
fn c1_header_style_flattened() {
    let constructs = c1_constructs("issue_c1_header_style");
    assert_has(&constructs, "header-footer-style");
    assert_clean("issue_c1_header_style");
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
        "outline-target",
        "outline-indent",
        "list-marker",
        "enum-reversed",
        "image-fit",
        "image-alt",
    ] {
        assert_has(&constructs, construct);
    }
    // The indent-only outline must NOT also report an outline-target:
    // the default target (headings) is not a custom target.
    assert_eq!(
        constructs
            .iter()
            .filter(|c| c.as_str() == "outline-target")
            .count(),
        1,
        "only the target: figure outline should report outline-target: {constructs:?}"
    );
    assert_clean("issue_c1_misc");
}
