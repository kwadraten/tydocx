//! Bare shape elements (`rect`, `square`, `circle`, `ellipse`, `polygon`,
//! `curve`, `line`) lower to real DrawingML at C2/C3: `wp:inline` +
//! `wps:wsp` drawings with preset geometry where Word has one and
//! `a:custGeom` paths for polygons/curves, fills, strokes, and editable
//! `wps:txbx` bodies. C1 keeps the skip-and-flatten contract; a `#line`
//! with only `length` stays a paragraph rule, not a drawing.

use crate::common::{
    fixture_doc_xml, fixture_outcome_with_options, fixture_package_with_options,
    paragraph_containing,
};
use tydocx::{CompatMode, ConvertOptions};

/// The `<w:drawing>...</w:drawing>` blocks in document order.
fn drawings(xml: &str) -> Vec<String> {
    xml.split("<w:drawing>")
        .skip(1)
        .map(|tail| tail.split("</w:drawing>").next().unwrap().to_string())
        .collect()
}

/// The single drawing containing `needle`, or panic.
fn drawing_with<'a>(drawings: &'a [String], needle: &str) -> &'a str {
    drawings
        .iter()
        .find(|d| d.contains(needle))
        .unwrap_or_else(|| panic!("a drawing should contain {needle:?}"))
}

#[test]
fn shapes_draw_as_inline_drawingml() {
    let xml = fixture_doc_xml("shapes_drawingml");
    let drawings = drawings(&xml);
    assert_eq!(
        drawings.len(),
        9,
        "every bare shape draws once; the 100% line stays a rule:\n{xml}"
    );
    for drawing in &drawings {
        assert!(
            drawing.contains("<wp:inline") && drawing.contains("<wps:wsp"),
            "shapes draw as inline wordprocessingShape:\n{drawing}"
        );
    }
}

#[test]
fn rect_draws_rounded_rect_with_editable_body() {
    let xml = fixture_doc_xml("shapes_drawingml");
    let drawings = drawings(&xml);
    let rect = drawing_with(&drawings, "Rect body text");
    assert!(
        rect.contains(r#"prstGeom prst="roundRect""#),
        "a radius-bearing rect picks the roundRect preset:\n{rect}"
    );
    assert!(
        rect.contains(r#"wp:extent cx="1440000" cy="720000""#),
        "4cm x 2cm resolves to EMU:\n{rect}"
    );
    assert!(
        rect.contains(r#"srgbClr val="AACCFF""#),
        "the declared fill lands on the shape:\n{rect}"
    );
    assert!(
        rect.contains("<wps:txbx>") && rect.contains("<w:txbxContent>"),
        "the body stays editable text inside the shape:\n{rect}"
    );
}

#[test]
fn square_and_circle_draw_quadratic_presets() {
    let xml = fixture_doc_xml("shapes_drawingml");
    let drawings = drawings(&xml);
    let square = drawing_with(&drawings, r#"prst="rect""#);
    assert!(
        square.contains(r#"wp:extent cx="360000" cy="360000""#),
        "a 1cm square forces equal sides:\n{square}"
    );
    assert!(
        square.contains(r#"srgbClr val="2ECC40""#),
        "typst green fills the square:\n{square}"
    );
    let circle = drawings
        .iter()
        .find(|d| d.contains(r#"prst="ellipse""#) && d.contains(r#"a:ln w="25400""#))
        .expect("the 1cm circle draws as an ellipse preset");
    assert!(
        circle.contains(r#"wp:extent cx="360000" cy="360000""#),
        "radius 5mm forces a quadratic extent:\n{circle}"
    );
    assert!(
        circle.contains(r#"srgbClr val="FF4136""#),
        "the stroke colour lands on a:ln:\n{circle}"
    );
}

#[test]
fn polygon_draws_closed_custgeom() {
    let xml = fixture_doc_xml("shapes_drawingml");
    let drawings = drawings(&xml);
    let polygon = drawing_with(&drawings, "<a:custGeom>");
    assert!(
        polygon.contains(r#"<a:path w="762000" h="127000""#),
        "the path space matches the resolved extent:\n{polygon}"
    );
    assert!(
        polygon.contains(r#"<a:pt x="381000" y="127000"/>"#),
        "vertices resolve against the region in EMU:\n{polygon}"
    );
    assert!(
        polygon.contains("<a:close/>"),
        "a polygon path closes:\n{polygon}"
    );
    assert!(
        polygon.contains(r#"srgbClr val="FFDC00""#),
        "the fill lands on the path:\n{polygon}"
    );
}

#[test]
fn curve_draws_custgeom_with_cubic_segments() {
    let xml = fixture_doc_xml("shapes_drawingml");
    let drawings = drawings(&xml);
    let curve = drawings
        .iter()
        .find(|d| d.contains("<a:cubicBezTo>"))
        .expect("the curve's cubic components draw as cubicBezTo");
    assert!(
        curve.contains(r#"a:ln w="19050""#),
        "the 1.5pt stroke lands on a:ln:\n{curve}"
    );
    assert!(
        curve.contains("<a:noFill/>"),
        "an unfilled curve draws noFill:\n{curve}"
    );
}

#[test]
fn geometric_line_draws_the_line_preset() {
    let xml = fixture_doc_xml("shapes_drawingml");
    let drawings = drawings(&xml);
    let line = drawing_with(&drawings, r#"prstGeom prst="line""#);
    assert!(
        line.contains(r#"wp:extent cx="720000" cy="360000""#),
        "start (0,0) to end (2cm,1cm) spans the extent corner-to-corner:\n{line}"
    );
}

#[test]
fn inline_shape_sits_inside_the_paragraph() {
    let xml = fixture_doc_xml("shapes_drawingml");
    let para = paragraph_containing(&xml, "Heading with");
    assert!(
        para.contains("<w:drawing>") && para.contains("inline"),
        "the heading's inline rect keeps its run position inside the \
         paragraph:\n{para}"
    );
}

#[test]
fn full_width_line_stays_a_paragraph_rule() {
    let xml = fixture_doc_xml("shapes_drawingml");
    assert!(
        xml.contains("<w:pBdr"),
        "#line(length: 100%) is a horizontal rule, not a shape:\n{xml}"
    );
    let rule_para = xml
        .split("<w:p>")
        .skip(1)
        .find(|p| p.split("</w:p>").next().unwrap().contains("<w:pBdr"))
        .expect("the horizontal-rule paragraph exists");
    assert!(
        !rule_para.contains("<w:drawing>"),
        "the rule paragraph carries no drawing:\n{rule_para}"
    );
}

#[test]
fn non_solid_fill_reports_and_still_draws() {
    let outcome =
        fixture_outcome_with_options("shapes_drawingml", &ConvertOptions::new(CompatMode::C2));
    assert!(
        outcome
            .degradations
            .iter()
            .any(|d| { d.construct == "shape" && d.message.contains("non-solid fill") }),
        "the gradient fill reports a shape Approximated degradation: {:?}",
        outcome.degradations
    );
    let xml = fixture_doc_xml("shapes_drawingml");
    let drawings = drawings(&xml);
    let gradient = drawings
        .iter()
        .find(|d| d.contains(r#"prst="rect""#) && d.contains("<a:noFill/>"))
        .expect("the gradient rect still draws, unfilled");
    assert!(
        !gradient.contains("<a:ln"),
        "a dropped fill draws no invented stroke:\n{gradient}"
    );
}

#[test]
fn c1_drops_shapes_and_keeps_the_body_text() {
    let outcome =
        fixture_outcome_with_options("shapes_drawingml", &ConvertOptions::new(CompatMode::C1));
    assert!(
        outcome.degradations.iter().any(|d| d.construct == "shape"),
        "C1 reports the dropped shapes: {:?}",
        outcome.degradations
    );
    let xml =
        fixture_package_with_options("shapes_drawingml", &ConvertOptions::new(CompatMode::C1))
            .part_text("word/document.xml")
            .to_owned();
    assert!(
        !xml.contains("<wps:wsp"),
        "C1 emits no shape drawings:\n{xml}"
    );
    assert!(
        xml.contains("Rect body text"),
        "the shape body flattens into the flow at C1:\n{xml}"
    );
}

#[test]
fn c3_draws_the_same_shapes_as_c2() {
    let c2 = fixture_package_with_options("shapes_drawingml", &ConvertOptions::new(CompatMode::C2))
        .part_text("word/document.xml")
        .to_owned();
    let c3 = fixture_package_with_options("shapes_drawingml", &ConvertOptions::new(CompatMode::C3))
        .part_text("word/document.xml")
        .to_owned();
    let c2_count = c2.matches("<wps:wsp").count();
    assert!(c2_count > 0, "C2 draws the fixture's shapes");
    assert_eq!(
        c2_count,
        c3.matches("<wps:wsp").count(),
        "C3 draws every C2 shape"
    );
}
