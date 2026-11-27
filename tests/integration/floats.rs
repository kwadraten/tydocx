//! `#place(.., float: true)` (manuscript case M3): floats emit `wp:anchor`
//! drawings — images as `pic:pic`, text as an editable `wps:txbx` shape — at
//! the page-absolute position the paged frames measured, while a 1pt-mark
//! carrier paragraph keeps the float's logical position in document order.

use crate::common::{
    fixture_doc_xml, fixture_outcome_with_options, fixture_package, fixture_package_with_options,
    fixture_styles_xml,
};
use tydocx::{CompatMode, ConvertOptions};

fn anchor_block(xml: &str) -> &str {
    let start = xml
        .find("<wp:anchor")
        .unwrap_or_else(|| panic!("document should contain a wp:anchor:\n{xml}"));
    let end = xml[start..]
        .find("</wp:anchor>")
        .map_or(xml.len(), |e| start + e);
    &xml[start..end]
}

#[test]
fn float_text_anchors_to_page_with_editable_textbox() {
    let xml = fixture_doc_xml("conformance/conf-place-float");
    let anchor = anchor_block(&xml);
    assert_eq!(
        anchor.matches(r#"relativeFrom="page""#).count(),
        2,
        "both axes anchor relative to the page:\n{anchor}"
    );
    assert!(
        anchor.contains("<wp:wrapTopAndBottom/>"),
        "a top float wraps top-and-bottom:\n{anchor}"
    );
    assert!(
        anchor.contains("<wps:txbx>") && anchor.contains("<w:txbxContent>"),
        "text float is a DrawingML text box, not a raster:\n{anchor}"
    );
    let txbx = anchor
        .split("<w:txbxContent>")
        .nth(1)
        .expect("txbxContent present");
    assert!(
        txbx.contains("<w:b/>") && txbx.contains("Floated callout"),
        "the floated text stays editable runs inside the text box:\n{txbx}"
    );
}

#[test]
fn float_clearance_maps_to_anchor_dist() {
    let xml = fixture_doc_xml("conformance/conf-place-float");
    let anchor = anchor_block(&xml);
    assert!(
        anchor.contains(r#"distT="209550""#) && anchor.contains(r#"distB="209550""#),
        "the default 1.5em clearance maps to distT/distB in EMU:\n{anchor}"
    );
    let xml = fixture_doc_xml("place_float_image");
    let anchor = anchor_block(&xml);
    assert!(
        anchor.contains(r#"distT="254000""#) && anchor.contains(r#"distB="254000""#),
        "clearance: 20pt maps to distT/distB = 254000 EMU:\n{anchor}"
    );
}

#[test]
fn float_image_anchors_as_picture() {
    let package = fixture_package("place_float_image");
    let xml = package.part_text("word/document.xml");
    let anchor = anchor_block(xml);
    assert!(
        anchor.contains("<pic:pic") && anchor.contains("<a:blip r:embed="),
        "a floated image anchors as pic:pic, not a text box:\n{anchor}"
    );
    assert!(
        package.part_names().any(|n| n.starts_with("word/media/")),
        "the anchored image still ships its media part: {:?}",
        package.part_names().collect::<Vec<_>>()
    );
}

#[test]
fn float_bottom_wraps_top_and_bottom_and_sits_low() {
    let xml = fixture_doc_xml("place_float_bottom");
    let anchor = anchor_block(&xml);
    assert!(
        anchor.contains("<wp:wrapTopAndBottom/>"),
        "a bottom float also wraps top-and-bottom:\n{anchor}"
    );
    let pos_v = anchor
        .split(r#"<wp:positionV relativeFrom="page">"#)
        .nth(1)
        .and_then(|rest| {
            rest.split("<wp:posOffset>")
                .nth(1)
                .and_then(|n| n.split("</wp:posOffset>").next())
        })
        .and_then(|n| n.parse::<u64>().ok())
        .expect("vertical posOffset present");
    assert!(
        pos_v > 8_000_000,
        "a bottom float anchors near the page bottom, got {pos_v} EMU"
    );
}

#[test]
fn float_carrier_keeps_logical_order() {
    let xml = fixture_doc_xml("place_float_mid");
    let first_lorem = xml
        .find("Lorem")
        .expect("the preceding paragraph is present");
    let anchor_at = xml.find("<wp:anchor").expect("anchor present");
    assert!(
        first_lorem < anchor_at,
        "the carrier paragraph sits after the text that precedes the float in source"
    );
}

#[test]
fn float_c1_degrades_to_flow_text() {
    let outcome = fixture_outcome_with_options(
        "conformance/conf-place-float",
        &ConvertOptions::new(CompatMode::C1),
    );
    assert!(
        outcome
            .degradations
            .iter()
            .any(|d| d.construct == "placed-content"),
        "C1 reports placed-content: {:?}",
        outcome.degradations
    );
    let package = fixture_package_with_options(
        "conformance/conf-place-float",
        &ConvertOptions::new(CompatMode::C1),
    );
    let xml = package.part_text("word/document.xml");
    assert!(!xml.contains("<wp:anchor"), "C1 emits no anchor");
    assert!(xml.contains("Floated callout"), "C1 keeps the text in flow");
}

#[test]
fn float_c3_anchors_like_c2() {
    let c2 = fixture_package_with_options(
        "conformance/conf-place-float",
        &ConvertOptions::new(CompatMode::C2),
    );
    let c3 = fixture_package_with_options(
        "conformance/conf-place-float",
        &ConvertOptions::new(CompatMode::C3),
    );
    assert_eq!(
        c3.part_text("word/document.xml"),
        c2.part_text("word/document.xml"),
        "C3 document.xml is identical to C2 for floats"
    );
}

#[test]
fn issue_code_font_body_is_mono_keeps_declared_font() {
    let xml = fixture_doc_xml("issue_code_font_body_is_mono");
    assert!(
        !xml.contains("Courier New"),
        "body runs in a monospace face must not be rewritten to the code font:\n{xml}"
    );
    let styles = fixture_styles_xml("issue_code_font_body_is_mono");
    let normal = styles
        .split(r#"w:styleId="Normal""#)
        .nth(1)
        .expect("Normal style present");
    assert!(
        normal.contains("DejaVu Sans Mono"),
        "the declared monospace body font stays on Normal:\n{normal}"
    );
}

#[test]
#[ignore = "needs libreoffice + pdftotext; run with --ignored"]
fn visual_float_renders_at_top_not_at_flow_position() {
    let pdf = crate::visual::tydocx_to_pdf_via_docx("place_float_mid", "float_mid")
        .expect("libreoffice required: install it or do not opt into the --ignored visual tests");
    let tmp_dir = std::env::temp_dir().join("tydocx_visual_test");
    std::fs::create_dir_all(&tmp_dir).expect("temp dir");
    let pdf_path = tmp_dir.join("float_mid.pdf");
    std::fs::write(&pdf_path, &pdf).expect("write pdf");
    let out = std::process::Command::new("pdftotext")
        .args(["-bbox"])
        .arg(&pdf_path)
        .arg("-")
        .output()
        .expect("pdftotext required for word positions");
    let bbox = String::from_utf8_lossy(&out.stdout);
    let y_min = |word: &str| -> f64 {
        let start = bbox
            .find(&format!(">{word}</word>"))
            .unwrap_or_else(|| panic!("word {word:?} should render:\n{bbox}"));
        let tag_start = bbox[..start].rfind("<word").expect("word tag start");
        let tag = &bbox[tag_start..start];
        tag.split("yMin=\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .and_then(|n| n.parse::<f64>().ok())
            .expect("word tag carries yMin")
    };
    assert!(
        y_min("Floated") < y_min("Lorem"),
        "the float renders above the text that precedes it in logical order"
    );
}
