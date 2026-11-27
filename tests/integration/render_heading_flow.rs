use super::paged;
use crate::common::{fixture_doc_xml, fixture_path, paragraph_containing, paragraph_texts};
use tydocx::TydocxWorld;

const PAGE_BREAK: &str = r#"<w:br w:type="page"/>"#;
const COLUMN_BREAK: &str = r#"<w:br w:type="column"/>"#;

fn native_pages(fixture: &str) -> usize {
    let world = TydocxWorld::new(&fixture_path(fixture)).expect("open fixture");
    paged(&world).pages().len()
}

#[test]
fn replacement_heading_keeps_all_blocks_after_its_weak_break() {
    let fixture = "render_heading_colbreak_show";
    assert_eq!(native_pages(fixture), 3);

    let xml = fixture_doc_xml(fixture);
    let (outline, body) = xml
        .split_once(PAGE_BREAK)
        .expect("first heading opens a new page");
    let (first, second) = body
        .split_once(PAGE_BREAK)
        .expect("second heading opens a new page");
    assert!(!second.contains(PAGE_BREAK), "{xml}");
    assert!(!xml.contains(COLUMN_BREAK), "{xml}");
    assert!(
        !body.contains("<w:numPr>") && !body.contains(r#"w:val="Heading"#),
        "{body}"
    );

    for (chapter, number, marker, title, prose) in [
        (
            first,
            "1",
            "Part 1",
            "Eider precise heading",
            "One content paragraph.",
        ),
        (
            second,
            "2",
            "Part 2",
            "Quartz precise heading",
            "A second content paragraph.",
        ),
    ] {
        let texts: Vec<_> = paragraph_texts(chapter)
            .into_iter()
            .filter(|text| !text.is_empty())
            .collect();
        assert_eq!(texts, [marker, title, prose]);
        let entry = paragraph_containing(outline, title);
        assert!(entry.contains(&format!(">{number} </w:t>")), "{entry}");
        let target = entry.split_once("w:anchor=\"").unwrap().1;
        let (target, _) = target.split_once('"').unwrap();
        let name = format!(r#"w:name="{target}""#);
        let bookmarked = paragraph_containing(chapter, &name);
        assert_eq!(paragraph_texts(bookmarked).concat(), marker, "{chapter}");
        assert_eq!(xml.matches(&name).count(), 1, "{xml}");
        assert_eq!(xml.matches(title).count(), 2, "{xml}");
    }
}

#[test]
fn reemitted_heading_owns_its_numbering_and_bookmark() {
    let fixture = "render_heading_colbreak_reemit";
    assert_eq!(native_pages(fixture), 2);

    let xml = fixture_doc_xml(fixture);
    let (outline, body) = xml
        .split_once(PAGE_BREAK)
        .expect("heading opens a new page");
    let title = "Preserved native heading";
    assert!(paragraph_containing(outline, title).contains(r#"w:anchor="native-heading""#));
    let texts: Vec<_> = paragraph_texts(body)
        .into_iter()
        .filter(|text| !text.is_empty())
        .collect();
    assert_eq!(
        texts,
        [
            "Introductory marker",
            title,
            "Closing marker",
            "Following content."
        ]
    );
    let heading = paragraph_containing(body, title);
    assert!(
        heading.contains(r#"w:val="Heading1""#) && heading.contains("<w:numPr>"),
        "{heading}"
    );
    assert!(heading.contains(r#"w:name="native-heading""#), "{heading}");
    assert_eq!(
        xml.matches(r#"w:name="native-heading""#).count(),
        1,
        "{xml}"
    );
    assert_eq!(xml.matches("<w:numPr>").count(), 1, "{xml}");
    assert_eq!(xml.matches(PAGE_BREAK).count(), 1, "{xml}");
    assert!(!xml.contains(COLUMN_BREAK), "{xml}");
}

#[test]
fn single_column_weak_breaks_share_strong_and_parity_boundaries() {
    let fixture = "render_colbreak_weak_page_flow";
    assert_eq!(native_pages(fixture), 5);

    let xml = fixture_doc_xml(fixture);
    let pages: Vec<_> = xml.split(PAGE_BREAK).collect();
    assert_eq!(pages.len(), 3, "{xml}");
    assert!(pages[0].contains("Opening body."), "{}", pages[0]);
    assert!(
        pages[1].contains("After paired weak breaks."),
        "{}",
        pages[1]
    );
    assert!(pages[2].contains("After the strong break."), "{}", pages[2]);
    assert!(pages[2].contains("After the odd boundary."), "{}", pages[2]);
    assert_eq!(xml.matches(r#"w:val="oddPage""#).count(), 1, "{xml}");
    assert_eq!(xml.matches("<w:sectPr>").count(), 2, "{xml}");
    assert!(!xml.contains(COLUMN_BREAK), "{xml}");
}

#[test]
fn multiple_page_columns_keep_the_column_break() {
    let xml = fixture_doc_xml("issue_column_break");
    assert_eq!(xml.matches(COLUMN_BREAK).count(), 1, "{xml}");
    assert!(!xml.contains(PAGE_BREAK), "{xml}");
}
