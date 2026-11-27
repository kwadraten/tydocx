//! `#link` child-node preservation: images, OMML math, and tracked spacing
//! inside a link body lower to their own run-level elements inside the
//! hyperlink, not through a `Vec<Run>` flattening that silently dropped them.

use crate::common::fixture_doc_xml;

/// Position of `needle` inside `xml`, or a panic naming what was searched.
fn pos(xml: &str, needle: &str) -> usize {
    xml.find(needle)
        .unwrap_or_else(|| panic!("document.xml should contain {needle:?}"))
}

/// Assert `inner` appears inside the `w:fldSimple` HYPERLINK element that
/// starts at or after `xml`.
fn assert_inside_hyperlink(xml: &str, inner: &str) {
    let link = pos(xml, r#"w:instr="HYPERLINK"#);
    let child = pos(xml, inner);
    let end = pos(&xml[link..], "</w:fldSimple>") + link;
    assert!(
        link < child && child < end,
        "{inner:?} must be emitted inside the hyperlink, got positions link={link} child={child} end={end}"
    );
}

#[test]
fn link_image_drawing_inside_hyperlink() {
    let xml = fixture_doc_xml("link_children_image");
    assert_inside_hyperlink(&xml, "<w:drawing>");
    assert_inside_hyperlink(&xml, "<pic:pic");
}

#[test]
fn link_math_omml_inside_hyperlink() {
    let xml = fixture_doc_xml("link_children_math");
    assert_inside_hyperlink(&xml, "<m:oMath>");
    assert_inside_hyperlink(&xml, "<m:t>");
}

#[test]
fn link_tracked_space_inside_hyperlink() {
    let xml = fixture_doc_xml("link_children_space");
    // `#h(2cm)` inside a link lowers to a tracked space run — previously a
    // flattened child that could not reach the hyperlink at all.
    assert_inside_hyperlink(&xml, "<w:spacing w:val=");
    assert_inside_hyperlink(&xml, ">pre<");
    assert_inside_hyperlink(&xml, ">post<");
}
