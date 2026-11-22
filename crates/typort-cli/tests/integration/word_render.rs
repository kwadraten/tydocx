//! Real-Word rendering behavior that the manuscript audit pinned down.

use crate::common::{fixture_doc_xml, fixture_package, paragraph_containing};

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

fn extent(anchor: &str) -> Option<(u64, u64)> {
    fn attr(tag: &str, name: &str) -> Option<u64> {
        let start = tag.find(name)? + name.len();
        let end = tag[start..].find('"')? + start;
        tag[start..end].parse().ok()
    }
    let tag = &anchor[anchor.find("<wp:extent")?..];
    Some((attr(tag, "cx=\"")?, attr(tag, "cy=\"")?))
}

#[test]
fn auto_grid_rows_carry_no_exact_height() {
    let xml = fixture_doc_xml("grid_auto_rows");
    assert!(
        xml.contains("<w:tbl>"),
        "the grid lowers to a table:\n{xml}"
    );
    assert!(
        !xml.contains("w:trHeight"),
        "auto-sized grid rows must not pin an exact height:\n{xml}"
    );
}

#[test]
fn realized_break_hints_are_dropped() {
    let xml = fixture_doc_xml("zwsp_url_text");
    for ch in ['\u{200B}', '\u{2060}'] {
        assert!(
            !xml.contains(ch),
            "U+{:04X} must not reach document.xml:\n{xml}",
            ch as u32,
        );
    }
    assert!(
        xml.contains("en.wikipedia.org"),
        "the URL text survives once the break hints are dropped:\n{xml}"
    );
}

#[test]
fn heading_keeps_scoped_par_overrides() {
    let xml = fixture_doc_xml("heading_par_style");
    let heading = paragraph_containing(&xml, "Chapter One");
    assert!(
        heading.contains("Heading1"),
        "the title lowers as a heading:\n{heading}"
    );
    assert!(
        heading.contains(r#"w:firstLine="0""#),
        "the show rule's first-line-indent: 0pt reaches the heading:\n{heading}"
    );
    let body = paragraph_containing(&xml, "A later body paragraph");
    assert!(
        !body.contains("w:ind"),
        "the body paragraph inherits Normal's indent silently:\n{body}"
    );
}

#[test]
fn placed_text_box_covers_its_underlay() {
    let xml = fixture_doc_xml("place_cover_backdrop");
    let found = anchors(&xml);
    let title = found
        .iter()
        .find(|a| a.contains("Cover title"))
        .expect("the title anchors as a text box");
    assert!(
        title.contains("<wps:txbx>"),
        "the title stays an editable text box:\n{title}"
    );
    assert!(
        title.contains(r#"srgbClr val="7F8F83""#),
        "the box fills with the band beneath it so the underlay is covered:\n{title}"
    );
    let earlier = found
        .iter()
        .find(|a| a.contains("Earlier title"))
        .expect("the earlier title anchors too");
    assert!(
        earlier.contains(r#"srgbClr val="7F8F83""#),
        "a box over the same band takes the same fill:\n{earlier}"
    );
}

#[test]
fn placed_text_box_keeps_true_extent() {
    let xml = fixture_doc_xml("place_cover_backdrop");
    let found = anchors(&xml);
    let title = found
        .iter()
        .find(|a| a.contains("Cover title"))
        .expect("the title anchors as a text box");
    let (cx, cy) = extent(title).expect("the anchor carries an extent");
    // The band's own 30pt at dy 66pt sits inside a 200pt page: an ink-bound
    // box is far smaller than the old page-spanning extent.
    let band_bottom_emu = 96_u64 * 12_700;
    assert!(
        cy <= band_bottom_emu,
        "the box ends at the text's true bottom, not the page edge:\n{title}"
    );
    let page_width_emu = 200_u64 * 12_700;
    assert!(
        cx < page_width_emu,
        "the box keeps its measured width, not the full page:\n{title}"
    );
    assert!(
        title.contains("<wp:wrapNone/>") && title.contains(r#"wrap="none""#),
        "wrap stays none so the single line never re-wraps:\n{title}"
    );
}

#[test]
fn plain_linebreak_paragraph_pins_left() {
    let xml = fixture_doc_xml("linebreak_justify");
    let para = paragraph_containing(&xml, "Jeanne Féaux");
    assert!(
        para.contains(r#"<w:jc w:val="left"/>"#),
        "a paragraph of plain #linebreak() lines must not stretch:\n{para}"
    );
    let wrapped = paragraph_containing(&xml, "A wrapped paragraph");
    assert!(
        !wrapped.contains("w:jc"),
        "a wrapped-only paragraph inherits Normal's justify:\n{wrapped}"
    );
}

#[test]
fn justified_linebreak_stays_stretchable() {
    let xml = fixture_doc_xml("linebreak_justify");
    let para = paragraph_containing(&xml, "Stretched line");
    assert!(
        para.contains("<w:br/>"),
        "the explicit break is emitted:\n{para}"
    );
    assert!(
        !para.contains(r#"w:val="left""#),
        "linebreak(justify: true) keeps the paragraph stretchable:\n{para}"
    );
}

#[test]
fn cover_places_stay_out_of_header_furniture() {
    let package = fixture_package("place_cover_no_furniture");
    let doc_xml = package.part_text("word/document.xml");
    let body_anchors = anchors(&doc_xml);
    let title: Vec<&&str> = body_anchors
        .iter()
        .filter(|a| a.contains("Cover title"))
        .collect();
    assert_eq!(
        title.len(),
        1,
        "the cover's placed title anchors exactly once, in the body:\n{doc_xml}"
    );
    let title = title[0];
    let (_, cy) = extent(title).expect("the body anchor carries a measured extent");
    let page_height_emu = 200_u64 * 12_700;
    assert!(
        cy < page_height_emu,
        "the body copy keeps its true ink extent, not the page-sized one:\n{title}"
    );
    assert!(
        title.contains(r#"srgbClr val="7F8F83""#),
        "the body copy keeps the band fill that covers its underlay:\n{title}"
    );
    for name in package.part_names() {
        if !name.starts_with("word/header") && !name.starts_with("word/footer") {
            continue;
        }
        let text = package.part_text(name);
        assert!(
            !text.contains("Cover title") && !text.contains("Cover author"),
            "a cover `#place` must not duplicate into furniture parts: {name}"
        );
        assert!(
            !text.contains("<pic:pic>"),
            "a cover image `#place` must not duplicate into furniture parts: {name}"
        );
    }
    let doi_in_header = package
        .part_names()
        .filter(|name| name.starts_with("word/header"))
        .any(|name| package.part_text(name).contains("DOI-MARK"));
    assert!(
        doi_in_header,
        "the declared page(foreground:) still lands in a header part"
    );
}
