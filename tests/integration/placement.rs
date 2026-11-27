//! Absolute placement: `#place` content and placed shapes.

use crate::common::{fixture_doc_xml, paragraph_containing};

fn anchors(xml: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<wp:anchor") {
        let tail = &rest[start..];
        let end = tail
            .find("</wp:anchor>")
            .map_or(tail.len(), |e| e + "</wp:anchor>".len());
        out.push(&tail[..end]);
        rest = &tail[end..];
    }
    out
}

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

/// A non-floating `#place` overlays the page: Typst paints it over earlier
/// content and under later content without displacing the flow, so it emits
/// a `wp:anchor` with `behindDoc="1"` and `wp:wrapNone` — never a flow
/// paragraph.
#[test]
fn placed_image_anchors_behind_text() {
    let xml = fixture_doc_xml("place_overlay_absolute");
    let anchors = anchors(&xml);
    assert_eq!(anchors.len(), 3, "one anchor per placed element:\n{xml}");
    let image = anchors
        .iter()
        .find(|a| a.contains("<pic:pic"))
        .expect("the placed image anchors as a picture");
    assert!(
        image.contains(r#"behindDoc="1""#) && image.contains("<wp:wrapNone/>"),
        "a placed image overlays instead of displacing text:\n{image}"
    );
    assert_eq!(
        image.matches(r#"relativeFrom="page""#).count(),
        2,
        "both axes anchor to the page edge:\n{image}"
    );
}

#[test]
fn placed_text_stays_an_editable_text_box() {
    let xml = fixture_doc_xml("place_overlay_absolute");
    let anchors = anchors(&xml);
    let text = anchors
        .iter()
        .find(|a| a.contains("Placed Cover Text"))
        .expect("the placed text anchors as a text box");
    assert!(
        text.contains("<wps:txbx>") && text.contains("<w:txbxContent>"),
        "placed text stays editable runs in a DrawingML text box:\n{text}"
    );
    // Typst's `linebreak(justify:)` defaults to false, so a placed line must
    // pin an explicit alignment rather than inherit a justified Normal.
    assert!(
        text.contains(r#"<w:jc w:val="left"/>"#),
        "placed text pins left alignment, never an inherited justify:\n{text}"
    );
}

#[test]
fn placed_rect_keeps_its_fill_as_a_box() {
    let xml = fixture_doc_xml("place_overlay_absolute");
    let anchors = anchors(&xml);
    let rect = anchors
        .iter()
        .find(|a| a.contains(r#"srgbClr val="7F8F83""#))
        .expect("the placed rect keeps its fill color");
    assert!(
        rect.contains("<wps:txbx>"),
        "a filled rect draws as a filled text box, not text-line shading:\n{rect}"
    );
}

#[test]
fn in_flow_image_stays_inline() {
    let xml = fixture_doc_xml("place_overlay_absolute");
    assert_eq!(
        xml.matches("<wp:inline").count(),
        1,
        "only the in-flow image is inline; placed images anchor:\n{xml}"
    );
    for needle in ["Cover line before.", "After text.", "Trailing line."] {
        assert!(
            xml.contains(needle),
            "flow text survives the overlay places:\n{xml}"
        );
    }
    let flow = paragraph_containing(&xml, "After text.");
    assert!(
        !flow.contains("<w:drawing"),
        "the paragraph after placed content is plain flow text:\n{flow}"
    );
}
