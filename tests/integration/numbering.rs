//! Numbering-semantics checklist over the L2 writer, derived from the
//! word-layout numbering model (see `docs/fallback-registry.md` for the
//! survey citation). Each item pins one OOXML rule:
//! counters are abstract-scoped so every list instance needs a `startOverride`;
//! `startOverride` lives on the `w:num` instance, not the abstract;
//! `lvlRestart` is intentionally absent (OOXML default = restart on any
//! higher level, which is Typst's enum semantics);
//! every level carries an explicit `w:start` so an item first appearing at
//! a deeper level has a defined ancestor start;
//! markers hang via `w:hanging`, never `w:firstLine`.

use crate::common::{fixture_doc_xml, fixture_part, paragraph_containing};

/// Split a numbering part into its `<w:abstractNum ..>...</w:abstractNum>` and
/// `<w:num ..>...</w:num>` sections.
fn numbering_sections(part: &str) -> (Vec<String>, Vec<String>) {
    let mut abstracts = Vec::new();
    let mut nums = Vec::new();
    for (tag, out) in [("w:abstractNum", &mut abstracts), ("w:num", &mut nums)] {
        let open = format!("<{tag} ");
        let close = format!("</{tag}>");
        let mut rest = part;
        while let Some(start) = rest.find(&open) {
            let end = rest[start..]
                .find(&close)
                .map(|i| start + i + close.len())
                .unwrap_or(part.len());
            out.push(rest[start..end].to_owned());
            rest = &rest[end.min(rest.len())..];
        }
    }
    (abstracts, nums)
}

#[test]
fn numbering_counters_are_abstract_scoped_so_each_list_restarts() {
    // Two ordered lists share abstractNum 1; each gets its own `w:num` whose
    // level-0 `startOverride` resets the shared counter. Without the override
    // the second list would continue 4,5,6 — this is the
    // abstractId-scoped-counter rule.
    let numbering = fixture_part("two_ordered_lists", "word/numbering.xml");
    let (_abstracts, nums) = numbering_sections(&numbering);
    let dynamic: Vec<&String> = nums
        .iter()
        .filter(|num| num.contains("w:startOverride"))
        .collect();
    assert!(
        dynamic.len() >= 2,
        "each list instance must carry its own startOverride, got {dynamic:?}"
    );
    for num in &dynamic {
        assert!(
            num.contains(r#"<w:abstractNumId w:val="1"/>"#),
            "ordered lists share abstract 1: {num}"
        );
    }
    let doc_xml = fixture_doc_xml("two_ordered_lists");
    let mut num_ids: Vec<&str> = Vec::new();
    let mut rest = doc_xml.as_str();
    let needle = "<w:numId w:val=\"";
    while let Some(i) = rest.find(needle) {
        let tail = &rest[i + needle.len()..];
        let end = tail.find('"').unwrap();
        num_ids.push(&tail[..end]);
        rest = &tail[end..];
    }
    let distinct: std::collections::BTreeSet<&&str> = num_ids.iter().collect();
    assert!(
        distinct.len() >= 2,
        "the two lists must reference different numIds, got {num_ids:?}"
    );
}

#[test]
fn numbering_start_override_is_scoped_to_the_num_instance() {
    // `startOverride` applies to the `w:num` instance (inside its
    // `w:lvlOverride`), never to the shared `w:abstractNum`; `enum(start: 4)`
    // must not leak into the abstract or into other lists.
    let numbering = fixture_part("enum_custom_start", "word/numbering.xml");
    let (abstracts, nums) = numbering_sections(&numbering);
    for abs in &abstracts {
        assert!(
            !abs.contains("startOverride"),
            "abstractNum must never carry startOverride: {abs}"
        );
    }
    let custom: Vec<&String> = nums
        .iter()
        .filter(|num| num.contains(r#"<w:startOverride w:val="4"/>"#))
        .collect();
    assert_eq!(custom.len(), 1, "exactly one instance starts at 4");
    assert!(
        custom[0].contains(r#"<w:lvlOverride w:ilvl="0">"#),
        "startOverride is scoped to level 0 of that num"
    );
    let plain: Vec<&String> = nums
        .iter()
        .filter(|num| num.contains(r#"<w:startOverride w:val="1"/>"#))
        .collect();
    assert!(
        !plain.is_empty(),
        "the plain enum restarts at 1 on its own instance"
    );
}

#[test]
fn numbering_levels_restart_on_any_higher_level_by_default() {
    // OOXML's `w:lvlRestart` default (when the element is absent) restarts a
    // level whenever ANY higher level increments — exactly Typst's enum
    // semantics for nested lists. Emitting the element would only narrow that.
    let numbering = fixture_part("nested_list", "word/numbering.xml");
    assert!(
        !numbering.contains("lvlRestart"),
        "lvlRestart must stay absent so levels restart on any higher level"
    );
    let doc_xml = fixture_doc_xml("nested_list");
    assert!(
        doc_xml.contains(r#"<w:ilvl w:val="1"/>"#),
        "nested items reference the deeper level"
    );
}

#[test]
fn numbering_every_level_declares_an_ancestor_start() {
    // An item first appearing at a deeper ilvl has no counters above it;
    // OOXML falls back to the level's own `w:start`. Every emitted `w:lvl`
    // — abstract and override alike — must declare one.
    for fixture in [
        "nested_list",
        "two_ordered_lists",
        "issue_nested_enum_reset",
    ] {
        let numbering = fixture_part(fixture, "word/numbering.xml");
        let levels = numbering.matches("<w:lvl ").count();
        let starts = numbering.matches("<w:start ").count();
        assert_eq!(
            levels, starts,
            "{fixture}: every w:lvl must declare w:start for the ancestor fallback"
        );
    }
}

#[test]
fn numbering_markers_hang_left_of_the_text_not_first_line() {
    // Marker placement uses a hanging indent: body text sits at w:left, the
    // marker hangs w:hanging twips to its left. A firstLine indent would push
    // the FIRST line right — the opposite of list layout.
    for (fixture, needle) in [
        ("two_ordered_lists", "alpha"),
        ("nested_list", "Item A"),
        ("edge_hanging_indent_list", "list item with"),
    ] {
        let doc_xml = fixture_doc_xml(fixture);
        let para = paragraph_containing(&doc_xml, needle);
        assert!(
            para.contains("<w:numPr>"),
            "{fixture}: {needle:?} should be a numbered paragraph"
        );
        let ind = para
            .find("<w:ind ")
            .map(|i| &para[i..para[i..].find("/>").map_or(para.len(), |e| i + e + 2)])
            .unwrap_or("");
        assert!(
            ind.contains("w:hanging"),
            "{fixture}: list marker placement must use w:hanging, got {ind}"
        );
        assert!(
            !ind.contains("w:firstLine"),
            "{fixture}: list marker placement must not use w:firstLine, got {ind}"
        );
    }
}
