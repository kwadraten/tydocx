//! Clip-derived `a:srcRect` image crops (`block/box(clip:)`,
//! `image(fit: "cover")`), lone-table `w:tblpPr` floats (`#place`,
//! `figure(placement:)`), and the margin-relative anchor of margin notes.

use crate::common::fixture_doc_xml;

#[test]
fn clip_and_negative_inset_derives_src_rect() {
    let xml = fixture_doc_xml("image_clip_crop");
    // box(inset: -10/-20/-50/-10) around a 200x100pt stretch-fit image: the
    // clip window shows 5%/20%/25%/10% of the source from each edge.
    assert!(
        xml.contains(r#"<a:srcRect l="5000" t="20000" r="25000" b="10000"/>"#),
        "the clip+negative-inset idiom crops the embedded image:\n{xml}"
    );
    // block(clip: true, inset: (bottom: -30pt)) around the default cover fit:
    // the cover-fit center crop (25% top and bottom) composes with the block
    // clip, leaving 25% top / 40% bottom.
    assert!(
        xml.contains(r#"<a:srcRect l="0" t="25000" r="0" b="40000"/>"#),
        "the lynx-style block-inset clip composes with the cover fit:\n{xml}"
    );
    // The displayed extent is the clipped window, not the full image extent.
    assert!(
        xml.contains(r#"<wp:extent cx="1778000" cy="889000"/>"#)
            && xml.contains(r#"<wp:extent cx="2540000" cy="889000"/>"#),
        "the drawing extent is the visible clip window:\n{xml}"
    );
}

#[test]
fn image_fit_cover_derives_src_rect() {
    let xml = fixture_doc_xml("image_fit_cover");
    // A 1:1 image covering a 2:1 box centers vertically and crops 25% top
    // and bottom; covering a 1:2 box crops 25% left and right.
    assert!(
        xml.contains(r#"<a:srcRect l="0" t="25000" r="0" b="25000"/>"#),
        "a tall target crops top+bottom of the cover fit:\n{xml}"
    );
    assert!(
        xml.contains(r#"<a:srcRect l="25000" t="0" r="25000" b="0"/>"#),
        "a wide target crops left+right of the cover fit:\n{xml}"
    );
    assert!(
        xml.contains(r#"<wp:extent cx="1270000" cy="635000"/>"#)
            && xml.contains(r#"<wp:extent cx="635000" cy="1270000"/>"#),
        "the drawing extent is the declared box, not the fitted image:\n{xml}"
    );
}

#[test]
fn figure_with_lone_table_body_floats_as_tblppr() {
    let xml = fixture_doc_xml("figure_float_table");
    assert!(
        xml.contains("<w:tblpPr"),
        "a placement figure with a lone table body floats via w:tblpPr:\n{xml}"
    );
    let start = xml.find("<w:tblpPr").unwrap();
    let end = xml[start..].find("/>").map_or(xml.len(), |e| start + e);
    let tblp = &xml[start..end];
    assert!(
        tblp.contains(r#"w:vertAnchor="page""#)
            && tblp.contains(r#"w:horzAnchor="page""#)
            && tblp.contains("w:tblpX=")
            && tblp.contains("w:tblpY="),
        "the floating table anchors at the measured page position:\n{tblp}"
    );
    assert!(
        xml.contains("Floating results") && xml.contains(r#"<w:gridSpan w:val="3"/>"#),
        "the caption travels with the table as a grid-spanning row:\n{xml}"
    );
    assert!(
        xml.contains("<w:tbl>"),
        "the body is a real table, not a text box:\n{xml}"
    );
}

#[test]
fn place_with_lone_table_body_positions_as_tblppr() {
    let xml = fixture_doc_xml("place_lone_table");
    // dx 30pt + dy 40pt from the default 2.5cm (1417twip) margin origin.
    assert!(
        xml.contains(r#"w:tblpX="2017""#) && xml.contains(r#"w:tblpY="2217""#),
        "the placed table anchors at the measured position:\n{xml}"
    );
    assert!(
        xml.contains("alpha") && xml.contains("<w:tbl>"),
        "the placed table stays a real table:\n{xml}"
    );
}

#[test]
fn margin_note_anchors_relative_to_margin() {
    let xml = fixture_doc_xml("margin_note");
    assert!(
        xml.contains(r#"<wp:positionH relativeFrom="margin">"#),
        "a box fully inside the left margin anchors to the text-area edge:\n{xml}"
    );
    assert!(
        xml.contains("<wp:posOffset>-"),
        "a left-margin note carries a negative margin offset:\n{xml}"
    );
    let start = xml
        .find("<wp:anchor")
        .expect("the margin note is an anchored float");
    let anchor = &xml[start..];
    assert!(
        anchor.contains("<w:txbxContent>") && anchor.contains("Margin note text"),
        "the note is an editable text box:\n{anchor}"
    );
}

#[test]
fn figure_scope_parent_pins_current_behavior() {
    let xml = fixture_doc_xml("figure_scope_parent");
    // `scope: "parent"` floats at page level across the two-column body;
    // today that lowers to the same w:tblpPr mechanism — pinned, not real
    // column spanning.
    assert!(
        xml.contains("<w:tblpPr") && xml.contains("Wide results"),
        "a parent-scope float keeps the measured page position:\n{xml}"
    );
}

#[test]
fn figure_with_image_body_keeps_anchored_textbox() {
    let xml = fixture_doc_xml("figure_float_image");
    assert!(
        xml.contains("<wp:anchor") && xml.contains("<w:txbxContent>"),
        "a non-table placement figure stays an anchored text box:\n{xml}"
    );
    assert!(
        xml.contains("A floating image"),
        "the figure caption rides in the text box:\n{xml}"
    );
}
