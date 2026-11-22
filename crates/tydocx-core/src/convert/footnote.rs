use std::collections::HashMap;

use tydocx_ooxml::document::{Document, FootnoteFormat, InlineElement, Paragraph};
use typst::introspection::Tag;
use typst_html::HtmlNode;
use typst_layout::PagedDocument;

use super::fmt::InlineFmt;
use super::image::ImagePlacement;
use super::{InlineOptions, collect_inlines, has_attr_value, tag_name};

/// Extract the footnote bodies from the HTML `doc-endnotes` section, add them to the
/// document, and refine the footnote text size from the Paged render. The size is
/// measured from the footnote runs themselves (`page::apply_footnote_text_size`)
/// because the global size histogram mistakes the superscript reference marker for
/// the footnote body text. `image_sizes` is the display-size map computed from
/// the paged render, for footnote images.
pub(super) fn extract_add_and_size_footnotes(
    doc: &mut Document,
    body_children: &[HtmlNode],
    paged: Option<&PagedDocument>,
    image_sizes: &HashMap<u64, ImagePlacement>,
) {
    let contents = extract_footnote_contents(body_children, image_sizes);
    for content in &contents {
        doc.add_footnote(vec![tydocx_ooxml::document::Paragraph {
            inlines: content.clone(),
            ..tydocx_ooxml::document::Paragraph::default()
        }]);
    }
    super::page::apply_footnote_text_size(doc, paged, &contents);
}

/// Find the footnote number from children starting at a TAG Start("footnote").
///
/// Locates the `role="doc-noteref"` element and reads its number. typst 0.15
/// changed the nesting: the marker is now `<sup id role="doc-noteref"><a>N</a></sup>`
/// (0.14 was `<a role="doc-noteref"><sup>N</sup></a>`), so rather than assume a
/// `<sup>` child we read the noteref element's own text content (which is `N`
/// either way) and only fall back to the nested-`<sup>` scan.
pub(super) fn find_footnote_id_in_range(children: &[HtmlNode]) -> Option<u32> {
    let text = find_footnote_marker_text(children)?;
    let trimmed = text.trim();
    if let Ok(n) = trimmed.parse() {
        return Some(n);
    }
    parse_circled_number(trimmed)
}

/// Read the rendered marker text of a footnote reference
/// (`role="doc-noteref"`) in the given range, e.g. `"1"`, `"①"`, or `"(1)"`.
/// The marker reflects the resolved numbering (including `#set
/// footnote(numbering: …)`), which the `FootnoteElem` field alone cannot
/// see. Returns `None` when no noteref element is found or its text is empty.
pub(super) fn find_footnote_marker_text(children: &[HtmlNode]) -> Option<String> {
    for child in children {
        match child {
            HtmlNode::Element(elem) => {
                if has_attr_value(elem, "role", "doc-noteref") {
                    let text = super::collect_deep_text(&elem.children);
                    let trimmed = text.trim().to_string();
                    if trimmed.is_empty() {
                        return None;
                    }
                    return Some(trimmed);
                }
                if let Some(text) = find_footnote_marker_text(&elem.children) {
                    return Some(text);
                }
            }
            HtmlNode::Tag(tag) => {
                if matches!(tag, Tag::End(..)) {
                    break;
                }
            }
            _ => {}
        }
    }
    None
}

/// Whether a rendered footnote marker is representable in Word: a plain
/// decimal (`"1"`) or a circled number (`"①"`, via `numFmt
/// decimalEnclosedCircle`). Anything else — `"(1)"`, `"i"`, `"a"` — has no
/// OOXML footnote numbering equivalent (U26).
#[must_use]
pub(super) fn footnote_marker_representable(marker: &str) -> bool {
    if marker.parse::<u32>().is_ok() {
        return true;
    }
    parse_circled_number(marker).is_some()
}

pub(super) fn parse_circled_number(s: &str) -> Option<u32> {
    let c = s.chars().next()?;
    let code = c as u32;
    match code {
        0x2460..=0x2473 => Some(code - 0x2460 + 1),
        0x3251..=0x325F => Some(code - 0x3251 + 21),
        0x32B1..=0x32BF => Some(code - 0x32B1 + 36),
        _ => None,
    }
}

/// Extract footnote content from the `<section role="doc-endnotes">` at the end of body.
pub(super) fn extract_footnote_contents(
    children: &[HtmlNode],
    image_sizes: &HashMap<u64, ImagePlacement>,
) -> Vec<Vec<InlineElement>> {
    let mut footnotes = Vec::new();

    for child in children {
        if let HtmlNode::Element(elem) = child
            && tag_name(elem) == "section"
            && has_attr_value(elem, "role", "doc-endnotes")
        {
            for ol_child in &elem.children {
                if let HtmlNode::Element(ol) = ol_child
                    && tag_name(ol) == "ol"
                {
                    extract_footnotes_from_ol(&ol.children, &mut footnotes, image_sizes);
                }
            }
        }
    }

    footnotes
}

fn extract_footnotes_from_ol(
    children: &[HtmlNode],
    footnotes: &mut Vec<Vec<InlineElement>>,
    image_sizes: &HashMap<u64, ImagePlacement>,
) {
    for child in children {
        if let HtmlNode::Element(li) = child
            && tag_name(li) == "li"
        {
            let mut para = Paragraph::new();
            collect_inlines(
                &li.children,
                &mut para,
                None,
                InlineOptions::footnote(InlineFmt::default(), image_sizes),
            );
            footnotes.push(para.inlines);
        }
    }
}

/// Detect whether footnotes use circled number format (①②③).
pub(super) fn detect_footnote_format(children: &[HtmlNode], doc: &mut Document) {
    for child in children {
        if let HtmlNode::Element(elem) = child {
            if has_attr_value(elem, "role", "doc-noteref") {
                // typst 0.15 puts the `doc-noteref` role ON the `<sup>` (whose child
                // is `<a>N`), not on an `<a>` with a `<sup>` child as in 0.14, so a
                // `<sup>`-child scan never matches. Read all descendant text (the
                // number, regardless of `<sup>`/`<a>` order) and test it — the same
                // approach `find_footnote_marker_text` uses.
                let text = super::collect_deep_text(&elem.children);
                if parse_circled_number(text.trim()).is_some() {
                    doc.style.footnote_format = FootnoteFormat::CircledNumber;
                    return;
                }
            }
            detect_footnote_format(&elem.children, doc);
            if doc.style.footnote_format == FootnoteFormat::CircledNumber {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footnote_marker_representable_decimal() {
        assert!(footnote_marker_representable("1"));
        assert!(footnote_marker_representable("42"));
    }

    #[test]
    fn footnote_marker_representable_circled() {
        assert!(footnote_marker_representable("①"));
        assert!(footnote_marker_representable("⑳"));
    }

    #[test]
    fn footnote_marker_not_representable() {
        // Parenthesized (the manuscript's "(n)"), roman, and letter
        // numberings have no OOXML footnote numbering equivalent.
        assert!(!footnote_marker_representable("(1)"));
        assert!(!footnote_marker_representable("i"));
        assert!(!footnote_marker_representable("a"));
        assert!(!footnote_marker_representable(""));
    }
}
