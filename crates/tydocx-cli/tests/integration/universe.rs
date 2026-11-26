//! Universe template-compat survey cases V-01–V-34: one test per case, pinning
//! the semantic preservation or the reported degradation.

use crate::common::{
    fixture_doc_xml, fixture_outcome_with_options, fixture_package, paragraph_containing,
};
use tydocx_core::{CompatMode, ConvertOptions};

fn c1_constructs(fixture: &str) -> Vec<String> {
    fixture_outcome_with_options(fixture, &ConvertOptions::new(CompatMode::C1))
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

#[test]
fn v01_show_heading_rule() {
    let xml = fixture_doc_xml("universe_show_heading");
    let para = paragraph_containing(&xml, "Shown Chapter");
    assert!(
        para.contains("Heading"),
        "identity rule keeps the Heading style"
    );
    assert!(xml.contains("REWRITTEN"), "rewritten heading text kept");
    assert_has(&c1_constructs("universe_show_heading"), "show-heading-rule");
}

#[test]
fn v02_heading_func_numbering() {
    let xml = fixture_doc_xml("universe_heading_func_numbering");
    assert!(
        xml.contains("Part"),
        "func-numbered heading keeps its number"
    );
    assert!(!xml.contains("w:numPr"), "func numbering is not a numPr");
    let constructs = c1_constructs("universe_heading_func_numbering");
    assert_has(&constructs, "heading-numbering");
    assert!(
        !constructs.iter().any(|c| c == "h-negative-space"),
        "numbering-realized spacing must not degrade: {constructs:?}"
    );
}

#[test]
fn v03_not_outlined_heading_is_plain() {
    let xml = fixture_doc_xml("universe_heading_not_outlined");
    let para = paragraph_containing(&xml, "Abstract");
    assert!(
        !para.contains("Heading"),
        "outlined:false must not be a Heading"
    );
    let para = paragraph_containing(&xml, "Real Chapter");
    assert!(para.contains("Heading"));
    assert_has(
        &c1_constructs("universe_heading_not_outlined"),
        "unoutlined-heading",
    );
}

#[test]
fn v04_show_outline_entry_rule() {
    assert_has(
        &c1_constructs("universe_outline_entry"),
        "show-outline-entry-rule",
    );
    let xml = fixture_doc_xml("universe_outline_entry");
    assert!(xml.contains("Chapter"));
}

#[test]
fn v05_outline_target_figure() {
    let xml = fixture_doc_xml("universe_outline_target");
    assert!(xml.contains("List of Figures"));
    assert!(
        xml.contains("TOC \\h \\z \\c"),
        "a figure-targeted outline emits the \\c switch"
    );
    assert!(
        xml.contains("A rect") && xml.contains("PAGEREF outline-target-0"),
        "the figure entry is cached behind the field"
    );
    assert!(
        !xml.contains("TOC \\o"),
        "a heading TOC must not masquerade as a list of figures"
    );
}

#[test]
fn v06_place_float() {
    let constructs = c1_constructs("universe_place_float");
    assert_has(&constructs, "placed-content");
    let xml = fixture_doc_xml("universe_place_float");
    assert!(xml.contains("Floating block"), "placed text survives");
}

#[test]
fn v07_shapes_and_dividers() {
    let constructs = c1_constructs("universe_shapes");
    assert_has(&constructs, "shape");
    let xml = fixture_doc_xml("universe_shapes");
    assert!(xml.contains("Body text"));
    assert!(!xml.contains("Divider"), "#divider stays invisible");
    assert!(
        xml.contains(r#"wp:extent cx="2880000" cy="12700""#),
        "#line(length: 50%) draws a half-width rule, not a full-width border:\n{xml}"
    );
}

#[test]
fn v08_svg_image() {
    let constructs = c1_constructs("universe_image_svg");
    assert_has(&constructs, "vector-image");
    let package = fixture_package("universe_image_svg");
    assert!(
        package.part_names().any(|n| n.starts_with("word/media/")),
        "rasterized media should ship: {:?}",
        package.part_names().collect::<Vec<_>>()
    );
}

#[test]
fn v09_place_absolute() {
    assert_has(&c1_constructs("universe_place_absolute"), "placed-content");
    let xml = fixture_doc_xml("universe_place_absolute");
    assert!(xml.contains("Corner tag"));
}

#[test]
fn v10_move_rotate() {
    let constructs = c1_constructs("universe_move_rotate");
    assert_has(&constructs, "moved-content");
    assert_has(&constructs, "rotated-content");
    let xml = fixture_doc_xml("universe_move_rotate");
    assert!(xml.contains("Raised"));
    assert!(xml.contains("Sideways"));
}

#[test]
fn v11_page_foreground() {
    assert_has(
        &c1_constructs("universe_page_foreground"),
        "page-foreground",
    );
    let xml = fixture_doc_xml("universe_page_foreground");
    assert!(xml.contains("Body text"));
    assert!(
        !xml.contains("DRAFT"),
        "foreground watermark is page furniture, not body text"
    );
}

#[test]
fn v12_grid_to_table() {
    assert_has(&c1_constructs("universe_grid"), "grid-layout");
    let xml = fixture_doc_xml("universe_grid");
    assert!(xml.contains("w:tbl"), "grid becomes a table");
    assert!(xml.contains("Left cell"));
    assert!(xml.contains("Right cell"));
}

#[test]
fn v13_pagebreak_to_odd() {
    let xml = fixture_doc_xml("universe_pagebreak_odd");
    assert_eq!(
        xml.matches("oddPage").count(),
        1,
        "exactly one oddPage break"
    );
    assert!(xml.contains("Next chapter"));
}

#[test]
fn v14_header_footer_distances() {
    let xml = fixture_doc_xml("universe_page_header_footer_dist");
    assert!(xml.contains("w:header=\"800\""), "header-ascent → w:header");
    assert!(
        xml.contains("w:footer=\"600\""),
        "footer-descent → w:footer"
    );
}

#[test]
fn v15_par_line_numbering() {
    let xml = fixture_doc_xml("universe_par_line");
    assert!(xml.contains("w:lnNumType"), "par.line → line numbering");
    assert!(xml.contains("w:countBy=\"1\""));
    assert!(xml.contains("w:start=\"0\""));
    assert!(xml.contains("w:distance=\"440\""));
    assert!(xml.contains("w:restart=\"continuous\""));
}

#[test]
fn v16_list_enum_indent() {
    let xml = fixture_doc_xml("universe_list_indent");
    let list_para = paragraph_containing(&xml, "Alpha");
    assert!(
        list_para.contains("w:left=\"720\"") && list_para.contains("w:hanging=\"240\""),
        "list indent 24pt+body 12pt → left 720 hanging 240: {list_para}"
    );
    let enum_para = paragraph_containing(&xml, "One");
    assert!(
        enum_para.contains("w:left=\"830\"") && enum_para.contains("w:hanging=\"110\""),
        "enum indent 36pt → left 830 hanging 110: {enum_para}"
    );
}

#[test]
fn v17_block_spacing() {
    let xml = fixture_doc_xml("universe_block_spacing");
    let para = paragraph_containing(&xml, "Spaced content");
    assert!(
        para.contains("w:before=\"400\"") && para.contains("w:after=\"400\""),
        "block spacing 20pt → before/after 400 twips: {para}"
    );
}

#[test]
fn v18_figure_gap() {
    let xml = fixture_doc_xml("universe_figure_gap");
    let caption = paragraph_containing(&xml, "Gapped");
    assert!(
        caption.contains("w:before=\"480\""),
        "figure gap 24pt → caption spacing before: {caption}"
    );
}

#[test]
fn v19_footnote_entry() {
    let package = fixture_package("universe_footnote_entry");
    let footnotes = package.part_text("word/footnotes.xml");
    assert!(footnotes.contains("w:separator"));
    assert!(footnotes.contains("w:sz=\"16\""), "separator stroke 2pt");
    assert!(
        footnotes.contains("w:before=\"240\""),
        "entry clearance 12pt"
    );
    assert!(footnotes.contains("w:after=\"120\""), "entry gap 6pt");
    assert!(
        footnotes.contains("w:firstLine=\"160\""),
        "entry indent 8pt"
    );
    let xml = fixture_doc_xml("universe_footnote_entry");
    assert!(xml.contains("w:footnoteReference"));
}

#[test]
fn v20_document_metadata() {
    let package = fixture_package("universe_document_metadata");
    let core = package.part_text("docProps/core.xml");
    assert!(core.contains("A template survey"), "description → core.xml");
    assert!(core.contains("typst"), "keywords → core.xml");
    assert!(core.contains("dc:title"), "title → core.xml");
    assert!(core.contains("A Author"), "author → dc:creator");
}

#[test]
fn v21_block_box_width() {
    let constructs = c1_constructs("universe_block_width");
    assert_has(&constructs, "block-width");
    assert_has(&constructs, "box-width");
    let xml = fixture_doc_xml("universe_block_width");
    assert!(xml.contains("Narrowed"));
    assert!(xml.contains("Wide inline"));
}

#[test]
fn v22_h_fr_space() {
    let constructs = c1_constructs("universe_h_fr");
    assert_has(&constructs, "h-fr-space");
    let xml = fixture_doc_xml("universe_h_fr");
    assert!(xml.contains("Left text"));
    assert!(xml.contains("Right text"));
    assert!(
        xml.contains("w:spacing w:val=\"140\""),
        "fixed h(7pt) → 140 twips run spacing"
    );
}

#[test]
fn v23_v_variants() {
    let constructs = c1_constructs("universe_v_variants");
    assert_has(&constructs, "v-negative-space");
    assert_has(&constructs, "v-fr-space");
    assert_has(&constructs, "v-ratio-space");
    let xml = fixture_doc_xml("universe_v_variants");
    assert!(xml.contains("Anchored"));
}

#[test]
fn v24_align_horizon() {
    let xml = fixture_doc_xml("universe_align_horizon");
    assert!(xml.contains("w:vAlign"), "horizon → vertical alignment");
    assert!(xml.contains("w:val=\"center\""), "horizon → center");
}

#[test]
fn v25_hide() {
    let constructs = c1_constructs("universe_hide");
    assert_has(&constructs, "hidden-content");
    let xml = fixture_doc_xml("universe_hide");
    assert!(!xml.contains("secret"), "hidden text dropped");
    assert!(xml.contains("Visible two"));
}

#[test]
fn v26_box_block_decoration() {
    let constructs = c1_constructs("universe_box_decoration");
    assert_has(&constructs, "block-decoration");
    assert_has(&constructs, "box-decoration");
    let xml = fixture_doc_xml("universe_box_decoration");
    assert!(xml.contains("Decorated block"));
    assert!(xml.contains("pill"));
}

#[test]
fn v27_highlight_geometry() {
    let constructs = c1_constructs("universe_highlight_geometry");
    assert_has(&constructs, "highlight-geometry");
    let xml = fixture_doc_xml("universe_highlight_geometry");
    assert!(xml.contains("highlighted"));
}

#[test]
fn v28_image_fit_alt() {
    let constructs = c1_constructs("universe_image_fit_alt");
    assert_has(&constructs, "image-fit");
    assert_has(&constructs, "image-alt");
    let xml = fixture_doc_xml("universe_image_fit_alt");
    assert!(xml.contains("w:drawing"), "image embedded");
}

#[test]
fn v29_opentype_props() {
    let xml = fixture_doc_xml("universe_opentype");
    assert!(xml.contains("w:numForm w:val=\"oldStyle\""), "number-type");
    assert!(xml.contains("w:ligatures w:val=\"none\""), "ligatures");
    assert!(
        xml.contains("w:numberSpacing w:val=\"tabular\""),
        "number-width"
    );
    assert!(xml.contains("w:smallCaps"), "features smcp → smallCaps");
    assert!(xml.contains("123"));
}

#[test]
fn v30_underline_offset() {
    let constructs = c1_constructs("universe_underline_offset");
    assert_has(&constructs, "underline-offset");
    let xml = fixture_doc_xml("universe_underline_offset");
    let para = paragraph_containing(&xml, "underlined text");
    assert!(para.contains("<w:u"), "underline still renders");
}

#[test]
fn v31_text_edges_box_baseline() {
    let constructs = c1_constructs("universe_text_edges");
    assert_has(&constructs, "text-edge-metrics");
    assert_has(&constructs, "box-baseline");
    let xml = fixture_doc_xml("universe_text_edges");
    assert!(xml.contains("Shifted"), "box content kept");
    assert!(xml.contains("w:position"), "text(baseline:) → w:position");
}

#[test]
fn v32_block_sticky() {
    let xml = fixture_doc_xml("universe_block_sticky");
    let para = paragraph_containing(&xml, "Stuck intro");
    assert!(para.contains("w:keepNext"), "sticky → keepNext");
}

#[test]
fn v33_list_marker_override() {
    let package = fixture_package("universe_list_marker");
    let numbering = package.part_text("word/numbering.xml");
    assert!(numbering.contains("&gt;"), "level-0 marker → lvlText");
    assert!(numbering.contains("→"), "level-1 marker → lvlText");
    assert!(numbering.contains("•"), "level-2 marker → lvlText");
}

#[test]
fn v34_columns_gutter() {
    let xml = fixture_doc_xml("universe_columns_gutter");
    assert!(xml.contains("w:num=\"2\""), "two columns");
    assert!(
        xml.contains("w:space=\"480\""),
        "columns gutter 24pt → w:space 480"
    );
}
