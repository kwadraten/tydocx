//! Vector pictures with a PNG base and an optional SVG DrawingML extension.

use std::fs;
use std::process::Command;

use crate::common::wps::{run_tool, temp_dir, wps_export};
use crate::common::{fixture_doc_xml, fixture_document, fixture_package};

fn assert_svg_drawings(xml: &str, inline: usize, anchored: usize) {
    let count = inline + anchored;
    assert!(!xml.contains("mc:AlternateContent"), "{xml}");
    assert_eq!(xml.matches("<w:drawing>").count(), count, "{xml}");
    assert_eq!(xml.matches("<wp:inline ").count(), inline, "{xml}");
    assert_eq!(xml.matches("<wp:anchor ").count(), anchored, "{xml}");
    assert_eq!(xml.matches("<a:blip r:embed=").count(), count, "{xml}");
    assert_eq!(xml.matches("<asvg:svgBlip ").count(), count, "{xml}");
}

#[test]
fn svg_image_keeps_png_base_and_vector_extension() {
    let package = fixture_package("svg_test");
    let xml = package.part_text("word/document.xml");
    assert_svg_drawings(xml, 1, 0);

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
    let base = xml
        .split_once(r#"<a:blip r:embed=""#)
        .and_then(|(_, tail)| tail.split_once('"'))
        .unwrap()
        .0;
    let extension = xml
        .split_once("<asvg:svgBlip ")
        .and_then(|(_, tail)| tail.split_once(r#"r:embed=""#))
        .and_then(|(_, tail)| tail.split_once('"'))
        .unwrap()
        .0;
    for (id, ext) in [(base, "png"), (extension, "svg")] {
        let relationship = rels
            .split("<Relationship ")
            .find(|tag| tag.contains(&format!(r#"Id="{id}""#)))
            .unwrap();
        assert!(relationship.contains(&format!(r#"Target="media/image1.{ext}""#)));
    }

    let content_types = package.part_text("[Content_Types].xml");
    assert!(
        content_types.contains("image/svg+xml"),
        "the SVG part declares its content type"
    );
}

#[test]
fn drawn_figure_body_embeds_svg_canvas() {
    let xml = fixture_doc_xml("edge_figure_rasterized");
    assert_svg_drawings(&xml, 1, 0);
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
    assert_svg_drawings(footnotes, 1, 0);
    let rels = package.part_text("word/_rels/footnotes.xml.rels");
    assert!(
        rels.contains("media/image1.png") && rels.contains("media/image1.svg"),
        "both image variants are related from footnotes.xml.rels, got: {rels}"
    );
}

#[test]
fn svg_anchors_use_native_drawings_in_body_header_and_footer() {
    let package = fixture_package("render_svg_single_drawing");
    let body = package.part_text("word/document.xml");
    assert_svg_drawings(body, 1, 1);
    for part in ["header1.xml", "footer1.xml"] {
        assert_svg_drawings(package.part_text(&format!("word/{part}")), 0, 1);
        let rels = package.part_text(&format!("word/_rels/{part}.rels"));
        assert!(rels.contains(".png\"") && rels.contains(".svg\""), "{rels}");
    }
}

#[test]
#[ignore = "needs Windows WPS Writer (Kwps.Application) and pdftoppm"]
fn wps_renders_png_based_svg_drawings() {
    let dir = temp_dir("wps-svg");
    let docx = dir.join("candidate.docx");
    let pdf = dir.join("candidate.pdf");
    tydocx_ooxml::write_docx(
        &fixture_document("render_svg_single_drawing"),
        fs::File::create(&docx).expect("create visual fixture DOCX"),
    )
    .unwrap();
    wps_export(&docx, &pdf, &dir, false);
    let page = run_tool(
        Command::new("pdftoppm")
            .args(["-r", "72", "-f", "1", "-l", "1", "-singlefile"])
            .arg(pdf),
        "pdftoppm",
    );
    let mut ppm = page.stdout.splitn(4, |&byte| byte == b'\n');
    assert_eq!(ppm.next(), Some(b"P6".as_slice()));
    let dimensions = std::str::from_utf8(ppm.next().expect("PPM dimensions")).unwrap();
    let (width, height) = dimensions.split_once(' ').expect("PPM width and height");
    let width = width.parse::<usize>().unwrap();
    let height = height.parse::<usize>().unwrap();
    assert_eq!(ppm.next(), Some(b"255".as_slice()));
    let pixels = ppm.next().expect("PPM pixels");
    assert_eq!(pixels.len(), width * height * 3);
    // Unique colors distinguish the four branches. Black text can't pass any of these.
    for (branch, [r, g, b]) in [
        ("inline", [174u8, 24, 171]),
        ("body anchor", [224, 37, 27]),
        ("header anchor", [12, 171, 59]),
        ("footer anchor", [29, 76, 218]),
    ] {
        let (ink, left, right) = pixels
            .chunks_exact(3)
            .enumerate()
            .filter(|(_, rgb)| {
                rgb[0].abs_diff(r) < 25 && rgb[1].abs_diff(g) < 25 && rgb[2].abs_diff(b) < 25
            })
            .fold((0, width, 0), |(ink, left, right), (index, _)| {
                let x = index % width;
                (ink + 1, left.min(x), right.max(x))
            });
        assert!(ink > 180, "WPS didn't paint the {branch} SVG: {ink} pixels");
        if branch == "inline" {
            // At 72 DPI, the 83.455 pt badge should occupy about 83 pixels and
            // be centered on the symmetric page despite the 2em body indent.
            assert!(
                right.abs_diff(left + 83) <= 5,
                "badge spans {left}..{right}"
            );
            assert!(
                (left + right).abs_diff(width) <= 4,
                "badge spans {left}..{right}"
            );
        }
    }
}
