use crate::common::{
    FixturePackage, fixture_outcome, fixture_package_from_document, fixture_path, paragraph_texts,
};
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use tydocx::{ConvertOptions, TydocxWorld};

pub(super) fn active_world(fixture: &str) -> TydocxWorld {
    // Keep the reductions inert for the all-fixture scan, just as in render_corpus.
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let source = fs::read_to_string(fixture_path(fixture)).expect("read reference reduction");
    let base = std::env::temp_dir();
    let opencode = base.join("opencode");
    let root = if opencode.is_dir() { opencode } else { base };
    let dir = root.join(format!("tydocx-render-references-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("create reference-reduction temp directory");
    let path = dir.join(format!(
        "{fixture}-{}.typ",
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&path, format!("{source}\n#render-case\n")).expect("activate reference reduction");
    TydocxWorld::new(&path).expect("open activated fixture")
}

fn active_package(fixture: &str) -> FixturePackage {
    let world = active_world(fixture);
    let outcome = tydocx::convert(&world, &ConvertOptions::default())
        .unwrap_or_else(|errors| panic!("{fixture} must convert: {errors:?}"));
    fixture_package_from_document(&outcome.document)
}

fn assert_reference(fixture: &str, label: &str, target_text: &str, reference_text: &str) {
    let package = active_package(fixture);
    let xml = package.part_text("word/document.xml");
    assert!(xml.contains(target_text), "missing reference target: {xml}");
    assert!(
        xml.split("<w:bookmarkStart ")
            .skip(1)
            .filter_map(|tail| tail.split('>').next())
            .any(|attrs| attrs.contains(&format!(r#"w:name="{label}""#))),
        "missing target bookmark {label:?}: {xml}"
    );
    let cache = xml
        .split_once(&format!(r#" REF {label} \h </w:instrText>"#))
        .and_then(|(_, tail)| tail.split_once(r#"<w:fldChar w:fldCharType="separate"/>"#))
        .and_then(|(_, tail)| tail.split_once(r#"<w:fldChar w:fldCharType="end"/>"#))
        .map(|(body, _)| body)
        .unwrap_or_else(|| panic!("missing REF field for {label:?}: {xml}"));
    assert_eq!(
        paragraph_texts(&format!("<w:p>{cache}</w:p>"))[0].replace('\u{a0}', " "),
        reference_text,
        "wrong cached REF text for {label:?}: {cache}"
    );
}

#[test]
fn columns_keep_the_heading_reference_and_bookmark() {
    // The column layouter hides the heading tag from the root realization.
    assert_reference(
        "render_nested_reference",
        "nested-chapter",
        "Nested chapter",
        "Section 1",
    );
}

#[test]
fn floating_place_keeps_the_figure_reference_and_bookmark() {
    // The source refers forward to a target realized only inside the float.
    assert_reference(
        "render_floating_reference",
        "float-target",
        "Float anchor",
        "Figure 1",
    );
}

#[test]
fn columns_keep_the_citation_and_bibliography() {
    // Resolving the key at compile time is insufficient: the cached marker must
    // link to an actual entry in the Word document.
    let world = active_world("render_nested_bibliography");
    let outcome = tydocx::convert(&world, &ConvertOptions::default()).unwrap();
    assert!(
        !outcome
            .degradations
            .iter()
            .any(|d| d.construct == "bibliography-display"),
        "{:?}",
        outcome.degradations
    );
    let package = fixture_package_from_document(&outcome.document);
    let xml = package.part_text("word/document.xml");
    assert_eq!(xml.matches("Evidence title").count(), 1, "{xml}");
    assert!(xml.contains("Nested sources"), "{xml}");
    assert!(
        crate::common::paragraph_containing(xml, "Evidence title").contains("224422"),
        "{xml}"
    );
    let (anchor, marker) = xml
        .split_once(r#"<w:hyperlink w:anchor=""#)
        .and_then(|(_, tail)| tail.split_once('"'))
        .and_then(|(anchor, tail)| {
            tail.split_once("</w:hyperlink>")
                .map(|(body, _)| (anchor, body))
        })
        .unwrap_or_else(|| panic!("missing citation hyperlink: {xml}"));
    assert!(!anchor.is_empty() && xml.contains(&format!(r#"w:name="{anchor}""#)));
    assert_eq!(
        paragraph_texts(&format!("<w:p>{marker}</w:p>"))[0],
        "[1]",
        "citation must link to the rendered entry"
    );
    assert!(
        package
            .part_text("customXml/item1.xml")
            .contains("Evidence title")
    );
}

#[test]
fn separate_bibliographies_emit_distinct_entries_and_bookmarks() {
    let outcome = fixture_outcome("bibliography_multiple");
    assert!(
        !outcome
            .degradations
            .iter()
            .any(|d| d.construct == "bibliography-display"),
        "{:?}",
        outcome.degradations
    );
    let package = fixture_package_from_document(&outcome.document);
    let xml = package.part_text("word/document.xml");
    for title in ["First Library Article", "Second Library Book"] {
        assert_eq!(xml.matches(title).count(), 1, "{xml}");
    }
    let anchors: Vec<_> = xml
        .split(r#"<w:hyperlink w:anchor=""#)
        .skip(1)
        .filter_map(|tail| tail.split('"').next())
        .filter(|anchor| anchor.starts_with("bib-"))
        .collect();
    assert_eq!(anchors.len(), 2, "{xml}");
    assert_ne!(anchors[0], anchors[1], "{xml}");
    for anchor in anchors {
        assert!(xml.contains(&format!(r#"w:name="{anchor}""#)), "{xml}");
    }
    let mut heading_targets = Vec::new();
    for title in ["References A", "References B"] {
        let heading = xml
            .split("<w:p>")
            .find(|p| p.contains("Heading1") && p.contains(title))
            .unwrap();
        let target = heading
            .split_once(r#"w:name=""#)
            .and_then(|(_, tail)| tail.split('"').next())
            .expect("bibliography headings emitted by one function need distinct targets");
        assert_eq!(
            xml.matches(&format!(r#"w:name="{target}""#)).count(),
            1,
            "{xml}"
        );
        assert!(xml.contains(&format!("PAGEREF {target} ")), "{xml}");
        heading_targets.push(target);
    }
    assert_ne!(heading_targets[0], heading_targets[1], "{xml}");
}
