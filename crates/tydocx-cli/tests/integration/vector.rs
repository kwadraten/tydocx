//! L4 vector preservation: SVG images and drawn figure canvases ship as a
//! single `a:blip` whose `a:extLst` carries `asvg:svgBlip` (the real vector
//! bytes), with the rasterized PNG as the `r:embed` for consumers without
//! the SVG extension — no `mc:AlternateContent` wrapper.

use crate::common::{fixture_doc_xml, fixture_package};

#[test]
fn svg_image_keeps_vector_blip_with_png_embed() {
    let xml = fixture_doc_xml("svg_test");
    assert!(!xml.contains("mc:AlternateContent"));
    assert!(xml.contains("asvg:svgBlip"));
    let blip = xml.find("<a:blip ").unwrap();
    let blip_end = xml[blip..].find("</a:blip>").unwrap();
    let body = &xml[blip..blip + blip_end];
    assert!(body.contains("<a:extLst>") && body.contains("asvg:svgBlip"));

    let package = fixture_package("svg_test");
    let names: Vec<&str> = package.part_names().collect();
    assert!(
        names.contains(&"word/media/image1.svg"),
        "the original SVG bytes ship beside the PNG, got: {names:?}"
    );
    assert!(
        names.contains(&"word/media/image1.png"),
        "a PNG fallback ships for consumers without SVG support"
    );
    let svg = package.part_text("word/media/image1.svg");
    assert!(
        svg.starts_with("<svg"),
        "the SVG part is real vector markup"
    );

    let rels = package.part_text("word/_rels/document.xml.rels");
    assert!(rels.contains(r#"Target="media/image1.png""#));
    assert!(rels.contains(r#"Target="media/image1.svg""#));

    let content_types = package.part_text("[Content_Types].xml");
    assert!(
        content_types.contains("image/svg+xml"),
        "the SVG part declares its content type"
    );
}

#[test]
fn drawn_figure_body_embeds_svg_canvas() {
    let xml = fixture_doc_xml("edge_figure_rasterized");
    assert!(
        xml.contains("asvg:svgBlip"),
        "the drawn figure canvas embeds as SVG on the realize path"
    );
    assert!(!xml.contains("mc:AlternateContent"));
    let package = fixture_package("edge_figure_rasterized");
    let svg = package.part_text("word/media/image1.svg");
    assert!(svg.starts_with("<svg"));
    assert!(
        !xml.contains("ZZLABELZZ"),
        "canvas-internal text stays inside the image, not leaked as body text"
    );
    assert!(xml.contains("Drawn figure"), "the caption survives");
    assert!(
        xml.contains("<w:tbl>"),
        "the sibling table figure stays a table"
    );
}

#[test]
fn footnote_svg_image_uses_footnote_relationships() {
    let package = fixture_package("footnote_svg_image");
    let footnotes = package.part_text("word/footnotes.xml");
    assert!(
        footnotes.contains("asvg:svgBlip") && !footnotes.contains("mc:AlternateContent"),
        "the footnote SVG embeds as a single blip+extLst inside footnotes.xml"
    );
    let rels = package.part_text("word/_rels/footnotes.xml.rels");
    assert!(
        rels.contains("media/image1.png") && rels.contains("media/image1.svg"),
        "both image variants are related from footnotes.xml.rels, got: {rels}"
    );
}
