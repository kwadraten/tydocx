//! Absolute placement: `#place` content and placed shapes.

use crate::common::fixture_doc_xml;

#[test]
fn issue_place_cover_rect_keeps_fill() {
    let doc_xml = fixture_doc_xml("issue_place_cover_rect");
    assert!(
        doc_xml.contains("Cover Title Here"),
        "placed cover title survives:\n{doc_xml}"
    );
    assert!(
        doc_xml.contains("7F8F83"),
        "a placed filled rect must keep its fill color, not vanish:\n{doc_xml}"
    );
    assert!(
        doc_xml.contains("After place."),
        "body text after the placed content survives:\n{doc_xml}"
    );
}
