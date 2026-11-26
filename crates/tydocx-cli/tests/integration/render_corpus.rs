//! Package-free reductions of discrepancies from real Universe/WPS render trials.

#[path = "render_heading_flow.rs"]
mod heading_flow;

use super::render_references::active_world;
use super::structure::assert_navigation_targets;
use crate::common::wps::{run_tool, temp_dir, wps_export};
use crate::common::{
    FixturePackage, fixture_doc_xml, fixture_outcome, fixture_package,
    fixture_package_from_document, fixture_path, paragraph_containing, paragraph_texts,
};
use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::process::Command;
use tydocx_core::{ConvertOptions, DegradationAction, TydocxWorld};
use tydocx_ooxml::document::{BlockElement, InlineElement};
use typst::layout::{Frame, FrameItem};
use typst_layout::PagedDocument;

fn paged(world: &TydocxWorld) -> PagedDocument {
    typst::compile::<PagedDocument>(world)
        .output
        .expect("Typst's native Paged target must accept this reduction")
}

fn paged_text(world: &TydocxWorld) -> String {
    fn extract(frame: &Frame, out: &mut String) {
        for (_, item) in frame.items() {
            match item {
                FrameItem::Text(text) => out.push_str(&text.text),
                FrameItem::Group(group) => extract(&group.frame, out),
                _ => {}
            }
        }
    }

    let mut text = String::new();
    for page in paged(world).pages() {
        extract(&page.frame, &mut text);
    }
    text
}

fn reference_text(fixture: &str) -> String {
    paged_text(&TydocxWorld::new(&fixture_path(fixture)).expect("open fixture"))
}

fn assert_compile_gap(fixture: &str, reference: &str, diagnostic: &str) {
    let world = active_world(fixture);
    let native = paged_text(&world);
    assert!(
        native.contains(reference),
        "native output should contain {reference:?}, got {native:?}"
    );

    let errors = tydocx_core::convert::convert(&world, &ConvertOptions::default())
        .err()
        .expect("known DOCX compile gap; replace this test with a preservation assertion if fixed");
    assert!(
        errors
            .iter()
            .any(|error| error.message.contains(diagnostic)),
        "expected {diagnostic:?}, got {errors:?}"
    );
}

fn assert_reference_preserved(fixture: &str, reference: &str, word_markup: &[&str]) {
    let world = active_world(fixture);
    let native = paged_text(&world);
    assert!(native.contains(reference), "{native}");
    let output = tydocx_core::convert::convert(&world, &ConvertOptions::default())
        .expect("DOCX must preserve the reference accepted by native Paged");
    let package = fixture_package_from_document(&output.document);
    let xml = package.part_text("word/document.xml");
    assert!(xml.contains(reference), "{xml}");
    for markup in word_markup {
        assert!(xml.contains(markup), "missing {markup:?} in {xml}");
    }
}

#[test]
fn nested_columns_reference_is_preserved() {
    // clean-math-paper, modern-nju-thesis and ntnu-physics-report-replica
    // all rely on reference targets nested within columns.
    assert_reference_preserved(
        "render_nested_reference",
        "Nested chapter",
        &[" REF nested-chapter ", r#"w:name="nested-chapter""#],
    );
}

#[test]
fn reemitted_equations_keep_native_numbers_and_unique_ref_targets() {
    let xml = fixture_doc_xml("render_equation_show_numbering");
    assert_eq!(xml.matches("<m:oMath>").count(), 4);
    assert_eq!(xml.matches("<m:oMathPara>").count(), 1);
    let equations: Vec<_> = xml
        .split("<w:p>")
        .filter(|paragraph| paragraph.contains("<m:oMath>"))
        .collect();
    assert_eq!(equations.len(), 4);
    for (paragraph, number) in
        equations
            .iter()
            .zip([Some("(1.1)"), Some("(1.2)"), None, Some("(1.3)")])
    {
        if let Some(number) = number {
            assert!(
                paragraph.contains(r#"w:val="center" w:pos="#),
                "{paragraph}"
            );
            assert!(paragraph.contains(r#"w:val="right" w:pos="#), "{paragraph}");
            assert!(
                paragraph.contains(&format!(">{number}</w:t>")),
                "{paragraph}"
            );
        } else {
            assert!(paragraph.contains("<m:oMathPara>"), "{paragraph}");
            assert!(
                !paragraph.contains("<w:t ") && !paragraph.contains("<w:t>"),
                "{paragraph}"
            );
            assert!(
                !paragraph.contains(r#"w:val="right" w:pos="#),
                "{paragraph}"
            );
        }
    }
    let mut names = std::collections::HashSet::new();
    for start in xml.split("<w:bookmarkStart ").skip(1) {
        let (name, _) = start
            .split_once("w:name=\"")
            .unwrap()
            .1
            .split_once('"')
            .unwrap();
        assert!(names.insert(name), "duplicate bookmark {name}");
    }
    let anchor = equations[0]
        .split_once(r#"w:name="display:first"/>"#)
        .unwrap()
        .1;
    let anchor = anchor.split_once("<w:bookmarkEnd").unwrap().0;
    assert!(
        anchor.contains(">(1.1)</w:t>") && !anchor.contains("<m:oMath>"),
        "{anchor}"
    );
    for reference in ["Forward:", "Backward:"] {
        let p = paragraph_containing(&xml, reference);
        assert!(p.contains(" REF display:first "), "{p}");
        assert!(p.contains(">(1.1)</w:t>"), "{p}");
    }
}

#[test]
fn contextual_numbering_function_keeps_the_display_equation_number() {
    // This independently exercises a numbering function returning `context` content.
    // It does not reproduce Alberta's empty parentheses: the probe's REF cache had a value.
    let fixture = "render_math_numbering_context";
    let native = reference_text(fixture);
    assert_eq!(native.matches("(2.1)").count(), 2, "{native}");
    let xml = fixture_doc_xml(fixture);
    let reference = paragraph_containing(&xml, "Equation reference:");
    let cache = reference
        .split_once(" REF balance ")
        .and_then(|(_, tail)| tail.split_once(r#"<w:fldChar w:fldCharType="separate"/>"#))
        .and_then(|(_, tail)| tail.split_once(r#"<w:fldChar w:fldCharType="end"/>"#))
        .map(|(cache, _)| cache)
        .expect("cached REF to balance");
    assert!(
        paragraph_texts(&format!("<w:p>{cache}</w:p>"))[0].contains("(2.1)"),
        "{reference}"
    );

    let equation = paragraph_containing(&xml, "<m:oMath>");
    for token in ["x", "=", "1"] {
        assert!(
            equation.contains(&format!("<m:t>{token}</m:t>")),
            "{equation}"
        );
    }
    assert_eq!(
        paragraph_texts(equation),
        ["(2.1)"],
        "the editable display equation itself must carry the native number: {equation}"
    );
}

#[test]
fn floating_place_figure_reference_is_preserved() {
    // blind-cvpr wraps a labelled figure in a floating place.
    assert_reference_preserved(
        "render_floating_reference",
        "Float anchor",
        &[" REF float-target ", r#"w:name="float-target""#],
    );
}

#[test]
fn nested_bibliography_is_preserved_with_citation() {
    // An in-memory Hayagriva bibliography avoids a data-file dependency.
    assert_reference_preserved(
        "render_nested_bibliography",
        "Evidence title",
        &["<w:hyperlink w:anchor=", ">[1]</w:t>"],
    );
}

#[test]
fn state_value_in_a_figure_is_missing_in_fragment_evaluation() {
    // faithful-acmart and biz-report read a state-backed configuration.
    // The Paged target finds the update; the DOCX figure body sees `none`.
    assert_compile_gap(
        "render_figure_state",
        "Theorem context",
        "cannot access fields on type none",
    );
}

#[test]
fn figure_reference_show_queries_metadata_after_each_target() {
    // The block's displayed metadata must join the same introspector as its target.
    for (fixture, prefix, references) in [
        (
            "render_theorem_metadata_after_target",
            "Reference:",
            &[("proof", "Resolved proof")][..],
        ),
        (
            "render_metadata_after_targets",
            "References:",
            &[
                ("first-proof", "First resolved proof"),
                ("second-proof", "Second resolved proof"),
            ][..],
        ),
    ] {
        let world = active_world(fixture);
        let native = paged_text(&world);
        let output = tydocx_core::convert::convert(&world, &ConvertOptions::default())
            .unwrap_or_else(|errors| panic!("{fixture}: {errors:?}"));
        let package = fixture_package_from_document(&output.document);
        let xml = package.part_text("word/document.xml");
        let paragraph = paragraph_containing(&xml, prefix);
        for (target, value) in references {
            assert!(native.contains(value), "{fixture}: {native}");
            assert!(
                paragraph.contains(&format!(r#"w:hyperlink w:anchor="{target}""#)),
                "{fixture}: {paragraph}"
            );
            assert!(paragraph.contains(value), "{fixture}: {paragraph}");
            assert_eq!(
                xml.matches(&format!(r#"w:name="{target}""#)).count(),
                1,
                "{fixture}: {xml}"
            );
        }
        assert!(!paragraph.contains(" REF "), "{fixture}: {paragraph}");
        assert_navigation_targets(&xml);
    }
}

#[test]
fn heading_counter_steps_are_evaluated_at_each_location() {
    let fixture = "render_heading_counter";
    let native = reference_text(fixture);
    assert!(
        native.contains("Problem 1") && native.contains("Problem 2"),
        "{native}"
    );
    let xml = fixture_doc_xml(fixture);
    assert_eq!(xml.matches(">Problem 1</w:t>").count(), 1, "{xml}");
    assert_eq!(xml.matches(">Problem 2</w:t>").count(), 1, "{xml}");
    assert!(!xml.contains(">Problem 0</w:t>"), "{xml}");
}

#[test]
fn figure_counter_is_evaluated_at_each_location() {
    let fixture = "render_theorem_counter";
    let native = reference_text(fixture);
    assert!(
        native.contains("Theorem 1") && native.contains("Theorem 2"),
        "{native}"
    );
    let xml = fixture_doc_xml(fixture);
    assert_eq!(xml.matches(">Theorem 1</w:t>").count(), 1, "{xml}");
    assert_eq!(xml.matches(">Theorem 2</w:t>").count(), 1, "{xml}");
}

#[test]
fn a_layout_callback_drops_its_width_dependent_body() {
    let fixture = "render_layout_callback";
    let native = reference_text(fixture);
    assert!(
        native.contains("Wide panel") && !native.contains("Narrow panel"),
        "{native}"
    );
    let xml = fixture_doc_xml(fixture);
    assert!(xml.contains("Ordinary body."));
    assert!(!xml.contains("Wide panel"), "known callback loss:\n{xml}");
    assert!(
        fixture_outcome(fixture)
            .degradations
            .iter()
            .any(|d| d.construct == "layouter-body"),
        "losing the callback's body must at least be reported"
    );
}

#[test]
fn a_chapter_show_break_reaches_the_main_flow_once() {
    // modern-nju-thesis emits its chapter break inside a heading show rule.
    let fixture = "render_heading_show_pagebreak";
    let native = reference_text(fixture);
    assert!(
        native.contains("First chapter") && native.contains("Third chapter"),
        "{native}"
    );
    let xml = fixture_doc_xml(fixture);
    let (before_second, second_and_after) = xml.split_once("Second chapter").unwrap();
    let (before_third, third_and_after) = second_and_after.split_once("Third chapter").unwrap();
    let breaks = |part: &str| part.matches(r#"w:br w:type="page""#).count();
    assert_eq!(
        breaks(before_second),
        1,
        "the leading weak break adds no blank page: {xml}"
    );
    assert_eq!(
        breaks(before_third),
        1,
        "explicit break and the show rule share one boundary: {xml}"
    );
    assert_eq!(breaks(third_and_after), 0, "{xml}");
}

#[test]
fn a_stack_preserves_text_and_drawing_in_flow() {
    // The CJK résumé template renders its form fields in stacks.
    let fixture = "render_stack_body";
    assert!(reference_text(fixture).contains("Career details."));
    let xml = fixture_doc_xml(fixture);
    let before = xml.find("Ordinary body.").expect("body before stack");
    let panel = xml.find("<w:drawing>").expect("native drawing in stack");
    let details = xml.find("Career details.").expect("text in stack");
    let after = xml.find("Following body.").expect("body after stack");
    assert!(
        before < panel && panel < details && details < after,
        "{xml}"
    );
    assert!(xml.contains(r#"val="158447""#), "{xml}");
    assert!(xml.contains(r#"cx="1778000" cy="304800""#), "{xml}");
}

#[test]
fn a_context_stack_preserves_both_measured_lines_in_order() {
    // The two callbacks originate at the same span in `rule`. Their evaluated
    // width and distinct strokes must survive each child's realization.
    let fixture = "render_stack_measured_lines";
    let outcome = fixture_outcome(fixture);
    let package = fixture_package_from_document(&outcome.document);
    let xml = package.part_text("word/document.xml");
    let (before, rest) = xml.split_once("Measured title").expect("title in stack");
    assert!(!before.contains("<w:drawing>"), "{xml}");
    assert!(!rest.contains("Measured title"), "{xml}");
    let (between, after) = rest
        .split_once("Following body.")
        .expect("body after stack");
    assert!(!after.contains("<w:drawing>"), "{xml}");
    let drawings: Vec<_> = between
        .split("<w:drawing>")
        .skip(1)
        .map(|tail| tail.split_once("</w:drawing>").expect("closed drawing").0)
        .collect();
    assert_eq!(drawings.len(), 2, "{xml}");
    for (drawing, color) in drawings.into_iter().zip(["FF4136", "0074D9"]) {
        assert!(
            drawing.contains(r#"<wp:extent cx="914400" cy="12700"/>"#),
            "{drawing}"
        );
        assert!(drawing.contains("<a:lnTo>"), "{drawing}");
        assert!(
            drawing.contains(&format!(r#"<a:srgbClr val="{color}""#)),
            "{drawing}"
        );
    }
    assert!(
        outcome.degradations.iter().any(|d| {
            d.construct == "stack-layout" && d.action == DegradationAction::Approximated
        }),
        "{:?}",
        outcome.degradations
    );
    assert!(
        !outcome
            .degradations
            .iter()
            .any(|d| d.construct == "layouter-body"),
        "{:?}",
        outcome.degradations
    );
}

#[test]
fn a_horizontal_stack_preserves_native_tables_captions_and_forward_references() {
    // modern-nju-thesis pairs in-flow figure tables using a horizontal stack.
    // The second figure is reconstructed by a show rule, so its reference
    // must resolve to the realized alias rather than only the source label.
    let fixture = "render_stack_tables";
    let native = reference_text(fixture);
    let outcome = fixture_outcome(fixture);
    let package = fixture_package_from_document(&outcome.document);
    let xml = package.part_text("word/document.xml");
    assert_eq!(xml.matches("<w:tbl>").count(), 2, "{xml}");
    assert!(
        !xml.contains("wps:txbx") && !xml.contains("w:tblpPr"),
        "{xml}"
    );

    let mut rest = xml;
    for text in [
        "Before the stack:",
        "First stack caption",
        "First cell",
        "Second stack caption",
        "Second cell",
        "After the stack.",
    ] {
        assert!(native.contains(text), "{native}");
        assert_eq!(xml.matches(text).count(), 1, "{xml}");
        rest = rest
            .split_once(text)
            .unwrap_or_else(|| panic!("out of order: {text}: {xml}"))
            .1;
    }
    let reference = paragraph_containing(xml, "Before the stack:");
    assert_eq!(
        paragraph_texts(reference)[0].replace('\u{a0}', " "),
        "Before the stack: Table 1 and Table 3.",
        "{reference}"
    );
    for (caption, number, label) in [
        ("First stack caption", 1, "stack-first"),
        ("Second stack caption", 3, "stack-reemitted"),
    ] {
        let target = paragraph_containing(xml, caption);
        assert!(
            reference.contains(&format!(r#" REF {label} \h "#)),
            "{reference}"
        );
        assert!(
            target.split("<w:bookmarkStart ").skip(1).any(|tag| {
                tag.split_once('>')
                    .is_some_and(|(attrs, _)| attrs.contains(&format!(r#"w:name="{label}""#)))
            }),
            "{target}"
        );
        assert_eq!(
            paragraph_texts(target)[0].replace('\u{a0}', " "),
            format!("Table {number}: {caption}"),
            "{target}"
        );
    }
    let first = paragraph_containing(xml, "First cell");
    let second = paragraph_containing(xml, "Second cell");
    assert!(!first.contains(r#"<w:sz w:val="32"/>"#), "{first}");
    assert!(second.contains(r#"<w:sz w:val="32"/>"#), "{second}");
    let tables: Vec<_> = xml
        .split("<w:tbl>")
        .skip(1)
        .map(|tail| tail.split_once("</w:tbl>").expect("closed table").0)
        .collect();
    for table in &tables {
        assert!(table.contains(r#"<w:jc w:val="center"/>"#), "{table}");
    }
    assert!(
        tables[0].contains(r#"<w:insideV w:val="single""#),
        "{}",
        tables[0]
    );
    assert!(
        tables[1].contains(r#"<w:top w:val="single""#),
        "{}",
        tables[1]
    );
    assert!(
        tables[1].contains(r#"<w:insideV w:val="nil""#),
        "{}",
        tables[1]
    );
    assert!(!xml.contains(r#"<w:spacing w:val="1000"/>"#), "{xml}");
    assert!(
        outcome.degradations.iter().any(|d| {
            d.construct == "stack-layout"
                && d.action == DegradationAction::Approximated
                && d.span.is_some()
        }),
        "{:?}",
        outcome.degradations
    );
    assert!(
        !outcome
            .degradations
            .iter()
            .any(|d| d.construct == "layouter-body")
    );
}

#[test]
fn flexible_header_gap_is_lost_but_text_and_bold_survive() {
    // adaptable-pset joins the left and right running-head labels in WPS.
    let package = fixture_package("render_header_spread");
    let name = package
        .part_names()
        .find(|name| name.starts_with("word/header"))
        .expect("header part");
    let xml = package.part_text(name);
    for preserved in ["LEFT-TEXT", "RIGHT-TEXT", "<w:b/>"] {
        assert!(xml.contains(preserved), "{xml}");
    }
    assert!(
        !xml.contains("w:tab") && !xml.contains("<w:tbl>") && !xml.contains("<wp:anchor"),
        "the current header encodes no independent horizontal placement:\n{xml}"
    );
}

#[test]
fn context_outlines_cache_each_selector_and_logical_page() {
    let fixture = "render_outline_context_selector";
    let native = reference_text(fixture);
    assert!(native.contains("Zygote index marker") && native.contains("Quartz index marker"));
    let xml = fixture_doc_xml(fixture);
    let rows: Vec<_> = xml
        .split("</w:p>")
        .filter(|row| row.contains(r" TOC \h \z \c "))
        .map(|row| row.rsplit_once("<w:p>").expect("TOC paragraph").1)
        .collect();
    assert_eq!(rows.len(), 2, "{xml}");
    for (row, (title, other, page)) in rows.into_iter().zip([
        ("Zygote index marker", "Quartz index marker", "II"),
        ("Quartz index marker", "Zygote index marker", "III"),
    ]) {
        let (entry, page_ref) = row.split_once(" PAGEREF ").expect("cached page reference");
        assert!(entry.contains(title) && !entry.contains(other), "{row}");
        assert!(page_ref.contains(&format!(">{page}</w:t>")), "{row}");
    }
    for text in ["First specimen body", "Second specimen body"] {
        assert!(xml.contains(text), "{xml}");
    }
    assert_navigation_targets(&xml);
}

#[test]
fn outline_links_resolve_to_the_rendered_target_paragraphs() {
    let fixture = "render_outline_target_navigation";
    let native = reference_text(fixture);
    let xml = fixture_doc_xml(fixture);
    assert_navigation_targets(&xml);
    for (title, caption) in [
        ("First inventory", "First caption"),
        ("Second inventory", "Second caption"),
    ] {
        let row = xml
            .split_once(title)
            .expect("instance title")
            .1
            .split("</w:p>")
            .nth(1)
            .expect("instance's first cache row");
        assert!(row.contains(" TOC ") && row.contains(caption), "{row}");
    }
    for title in ["First section", "Second section", "Visible index"] {
        assert!(native.contains(title), "{native}");
        let paragraphs: Vec<_> = xml
            .split("</w:p>")
            .filter(|paragraph| paragraph.contains(title))
            .collect();
        let [row, target] = paragraphs.as_slice() else {
            panic!("expected a row and its actual title paragraph for {title}: {xml}");
        };
        let name = row
            .split_once(" PAGEREF ")
            .expect("cached page reference")
            .1
            .split_ascii_whitespace()
            .next()
            .expect("target name");
        assert!(row.contains(&format!(r#"w:anchor="{name}""#)), "{row}");
        assert!(target.contains(&format!(r#"w:name="{name}""#)), "{target}");
    }
}

#[test]
fn rewritten_heading_leaves_outline_fields_without_bookmarks() {
    // WPS renders the dangling PAGEREF fields in euler-math as bookmark errors.
    let fixture = "render_rewritten_outline";
    let native = reference_text(fixture);
    assert!(native.contains("First chapter") && native.contains("Second chapter"));
    let xml = fixture_doc_xml(fixture);
    for target in ["outline-target-0", "outline-target-1"] {
        assert!(xml.contains(&format!("PAGEREF {target} ")), "{xml}");
        assert!(!xml.contains(&format!("w:name=\"{target}\"")), "{xml}");
    }
}

fn normal_style(package: &FixturePackage) -> &str {
    package
        .part_text("word/styles.xml")
        .split("w:styleId=\"Normal\"")
        .nth(1)
        .and_then(|tail| tail.split("</w:style>").next())
        .expect("Normal style")
}

#[test]
fn root_text_size_keeps_title_formatting_local() {
    // Cobalt CV's 28pt title used to become the default for 10pt body text.
    let package = fixture_package("render_title_body_size");
    let normal = normal_style(&package);
    let xml = package.part_text("word/document.xml");
    let title = paragraph_containing(xml, "TITLE");
    let body = paragraph_containing(xml, "Ordinary body in ten point type.");
    assert!(normal.contains(r#"<w:sz w:val="20"/>"#), "{normal}");
    assert!(title.contains(r#"<w:sz w:val="56"/>"#), "{title}");
    assert!(!body.contains("<w:sz "), "{body}");
}

fn text_defaults_package(label: &str, source: &str) -> FixturePackage {
    let path = temp_dir(label).join("style.typ");
    fs::write(&path, source).expect("write text-defaults reduction");
    let world = TydocxWorld::new(&path).expect("open text-defaults reduction");
    let converted = tydocx_core::convert::convert(&world, &ConvertOptions::default())
        .expect("convert text-defaults reduction");
    fixture_package_from_document(&converted.document)
}

#[test]
fn root_text_font_and_explicit_default_size_beat_glyph_vote() {
    // Computed arguments cannot be recovered by matching literal set rules.
    // The explicit 11pt also equals Typst's built-in default.
    let package = text_defaults_package(
        "declared-text-defaults",
        r#"
#set page(margin: 30pt)
#let body-size = 11pt
#let body-font = "Libertinus Serif"
#set text(size: body-size, font: body-font)
#align(center, text(size: 28pt, font: "New Computer Modern")[Typography sample with more glyphs than the body])

Body marker.
"#,
    );
    let normal = normal_style(&package).to_ascii_lowercase();
    let xml = package.part_text("word/document.xml");
    let title = paragraph_containing(xml, "Typography sample").to_ascii_lowercase();
    let body = paragraph_containing(xml, "Body marker.");
    assert!(normal.contains(r#"w:ascii="libertinus serif""#), "{normal}");
    assert!(normal.contains(r#"<w:sz w:val="22"/>"#), "{normal}");
    assert!(title.contains(r#"<w:sz w:val="56"/>"#), "{title}");
    assert!(
        title.contains(r#"w:ascii="new computer modern""#),
        "{title}"
    );
    assert!(
        !body.contains("<w:sz ") && !body.contains("<w:rFonts"),
        "{body}"
    );
}

#[test]
fn no_root_text_defaults_keeps_glyph_vote() {
    // With no root set rules, the outweighing local face and size still
    // establish Normal, rather than forcing Typst's built-in defaults.
    let package = text_defaults_package(
        "inferred-text-defaults",
        r#"
#text(size: 14pt, font: "New Computer Modern")[This paragraph has more glyphs and sets the inferred baseline.]

Short body.
"#,
    );
    let normal = normal_style(&package).to_ascii_lowercase();
    assert!(
        normal.contains(r#"w:ascii="new computer modern""#),
        "{normal}"
    );
    assert!(normal.contains(r#"<w:sz w:val="28"/>"#), "{normal}");
    let xml = package.part_text("word/document.xml");
    let sampled = paragraph_containing(xml, "This paragraph");
    let short = paragraph_containing(xml, "Short body.").to_ascii_lowercase();
    assert!(!sampled.contains("<w:sz "), "{sampled}");
    assert!(short.contains(r#"<w:sz w:val="22"/>"#), "{short}");
    assert!(short.contains(r#"w:ascii="libertinus serif""#), "{short}");
}

#[test]
fn a_figure_rectangle_keeps_editable_raw_lines() {
    // This probes a block raw inside a rectangle, not loss of the entire WUT body.
    let xml = fixture_doc_xml("render_figure_rect_raw");
    let (textbox, after_textbox) = xml
        .split_once("<w:txbxContent>")
        .and_then(|(_, tail)| tail.split_once("</w:txbxContent>"))
        .expect("editable listing in the rectangle's text box");
    // Count both paragraph and soft-break boundaries as lines, not run boundaries.
    let lines = textbox
        .replace("<w:br/>", "</w:p><w:p>")
        .replace(r#"<w:br w:type="textWrapping"/>"#, "</w:p><w:p>")
        .replace("<w:cr/>", "</w:p><w:p>");
    assert_eq!(
        paragraph_texts(&lines),
        ["fn hidden_needle() {", "    let alphabet = 42;", "}"],
        "{textbox}"
    );

    let caption = "Procedure listing anchor";
    let caption_count: usize = paragraph_texts(&xml)
        .iter()
        .map(|text| text.matches(caption).count())
        .sum();
    assert_eq!(caption_count, 1, "{xml}");
    let mut following = paragraph_texts(after_textbox).into_iter();
    assert!(following.any(|text| text.contains(caption)), "{xml}");
    assert!(
        following.any(|text| text == "The ordinary paragraph after the listing."),
        "{xml}"
    );
}

#[test]
fn block_fill_is_lost_but_insets_and_text_survive() {
    // chef-cookbook and yuuko-report both lose decorated callout panels.
    let fixture = "render_block_fill";
    let xml = fixture_doc_xml(fixture);
    assert!(xml.contains("Section contents."));
    assert!(xml.contains(r#"w:left="100" w:right="300""#), "{xml}");
    assert!(
        !xml.contains("<w:shd") && !xml.contains("<w:drawing>"),
        "{xml}"
    );
    assert!(
        fixture_outcome(fixture)
            .degradations
            .iter()
            .any(|d| d.construct == "block-decoration")
    );
}

#[test]
fn large_poster_dimensions_are_present_in_the_docx() {
    // Peace of Posters' WPS PDF is clipped; the actual A0 dimensions make
    // it into OOXML, so the size discrepancy is downstream of the writer.
    let xml = fixture_doc_xml("render_poster_page_size");
    assert!(xml.contains("Poster body."), "{xml}");
    assert!(
        xml.contains(r#"<w:pgSz w:w="47679" w:h="67408"/>"#),
        "{xml}"
    );
}

fn ppm_first_page(pdf: &Path) -> (String, Vec<u8>) {
    let output = run_tool(
        Command::new("pdftoppm")
            .args(["-r", "72", "-f", "1", "-l", "1", "-singlefile"])
            .arg(pdf),
        "pdftoppm",
    );
    // pdftoppm writes a raw P6 PPM to stdout with three header lines.
    let mut parts = output.stdout.splitn(4, |&b| b == b'\n');
    assert_eq!(parts.next(), Some(b"P6".as_slice()), "expected raw PPM");
    let dimensions = String::from_utf8(parts.next().expect("PPM dimensions").to_vec())
        .expect("ASCII dimensions");
    assert_eq!(parts.next(), Some(b"255".as_slice()), "expected 8-bit RGB");
    (dimensions, parts.next().expect("PPM pixels").to_vec())
}

fn green_ink(pdf: &Path) -> usize {
    ppm_first_page(pdf)
        .1
        .chunks_exact(3)
        .filter(|rgb| rgb[1] > rgb[0].saturating_add(35) && rgb[1] > rgb[2].saturating_add(35))
        .count()
}

pub(super) fn render_native_and_wps(fixture: &str) -> (PathBuf, PathBuf) {
    render_native_and_wps_fields(fixture, false)
}

fn render_native_and_wps_fields(fixture: &str, refresh_fields: bool) -> (PathBuf, PathBuf) {
    let world = TydocxWorld::new(&fixture_path(fixture)).expect("open visual fixture");
    let dir = temp_dir(fixture);
    let typst_pdf = dir.join("native.pdf");
    let docx = dir.join("candidate.docx");
    let wps_pdf = dir.join("wps.pdf");
    let rendered = typst_pdf::pdf(&paged(&world), &typst_pdf::PdfOptions::default())
        .expect("render native PDF");
    fs::write(&typst_pdf, rendered).expect("write reference PDF");
    let mut converted = tydocx_core::convert::convert(&world, &ConvertOptions::default())
        .expect("convert visual fixture");
    if refresh_fields {
        // Make it impossible to pass by exporting an untouched cached REF.
        let mut seeded = false;
        for element in &mut converted.document.body.elements {
            if let BlockElement::Paragraph(paragraph) = element {
                for inline in &mut paragraph.inlines {
                    if let InlineElement::FieldRef { display_text, .. } = inline {
                        *display_text = "stale".to_owned();
                        seeded = true;
                    }
                }
            }
        }
        assert!(seeded, "refresh probe needs a REF field");
    }
    tydocx_ooxml::write_docx(
        &converted.document,
        BufWriter::new(fs::File::create(&docx).expect("create candidate DOCX")),
    )
    .expect("write candidate DOCX");
    wps_export(&docx, &wps_pdf, &dir, refresh_fields);
    (typst_pdf, wps_pdf)
}

#[test]
#[ignore = "needs Windows WPS Writer (Kwps.Application), pdftotext and pdftoppm"]
fn wps_visual_keeps_equation_centers_and_refreshes_number_refs() {
    let (native, wps) = render_native_and_wps_fields("render_equation_show_numbering", true);
    for pdf in [&native, &wps] {
        let output = run_tool(
            Command::new("pdftotext").args(["-bbox"]).arg(pdf).arg("-"),
            "pdftotext",
        );
        let bbox = String::from_utf8(output.stdout).expect("UTF-8 bbox");
        for number in ["(1.1)", "(1.2)", "(1.3)"] {
            let right_edges: Vec<f64> = bbox
                .lines()
                .filter(|line| line.ends_with(&format!(">{number}</word>")))
                .map(|line| {
                    line.split_once("xMax=\"")
                        .unwrap()
                        .1
                        .split_once('"')
                        .unwrap()
                        .0
                        .parse()
                        .unwrap()
                })
                .filter(|&edge| edge > 400.0)
                .collect();
            assert_eq!(right_edges.len(), 1, "{pdf:?}: {bbox}");
            assert!(
                (right_edges[0] - 506.28).abs() < 2.0,
                "{pdf:?}: {right_edges:?}"
            );
        }
        let (dimensions, ppm) = ppm_first_page(pdf);
        let width: usize = dimensions
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        // Only the equations reach this horizontal window. Measure painted ink
        // rather than relying on the PDF font's math-to-Unicode map.
        let mut bands = Vec::new();
        let mut bounds: Option<(usize, usize)> = None;
        for row in ppm.chunks_exact(width * 3) {
            for x in 240..350 {
                if row[x * 3] < 160 {
                    bounds =
                        Some(bounds.map_or((x, x), |(left, right)| (left.min(x), right.max(x))));
                }
            }
            if (240..350).all(|x| row[x * 3] >= 160)
                && let Some((left, right)) = bounds.take()
            {
                bands.push((left + right) as f64 / 2.0);
            }
        }
        assert_eq!(bands.len(), 4, "{pdf:?}: {bands:?}");
        for center in bands {
            assert!((center - 295.65).abs() < 2.0, "{pdf:?}: {center}");
        }
        let layout = run_tool(
            Command::new("pdftotext")
                .args(["-layout"])
                .arg(pdf)
                .arg("-"),
            "pdftotext",
        );
        let layout = String::from_utf8(layout.stdout).expect("UTF-8 PDF text");
        for reference in ["Forward:", "Backward:"] {
            assert!(
                layout
                    .lines()
                    .any(|line| line.contains(reference) && line.contains("(1.1).")),
                "{pdf:?}: {layout}"
            );
        }
    }
}

#[test]
#[ignore = "needs Windows WPS Writer (Kwps.Application) and pdftotext"]
fn wps_visual_keeps_numbered_and_unnumbered_source_blanks() {
    let (native, wps) = render_native_and_wps("section_parity_page_footer");
    for pdf in [&native, &wps] {
        let extracted = run_tool(
            Command::new("pdftotext")
                .args(["-layout"])
                .arg(pdf)
                .arg("-"),
            "pdftotext",
        );
        let text = String::from_utf8(extracted.stdout).expect("UTF-8 PDF text");
        let pages: Vec<_> = text.split_terminator('\u{c}').map(str::trim).collect();
        let locate = |text: &str| pages.iter().position(|p| p.contains(text)).unwrap();
        let declaration = locate("Declaration leaf.");
        let preface = locate("Preface leaf.");
        let contents = locate("Contents leaf.");
        let first_chapter = locate("First chapter");
        let second_chapter = locate("Second chapter");

        assert_eq!(declaration % 2, 0, "{pdf:?}: {pages:?}");
        assert_eq!(contents % 2, 0, "{pdf:?}: {pages:?}");
        assert_eq!(pages[declaration - 1], "", "{pdf:?}: {pages:?}");
        assert_eq!(
            &pages[declaration + 1..preface],
            &[""],
            "{pdf:?}: {pages:?}"
        );
        assert_eq!(&pages[preface + 1..contents], &["II"], "{pdf:?}: {pages:?}");
        assert_eq!(
            &pages[contents + 1..first_chapter],
            &["IV"],
            "{pdf:?}: {pages:?}"
        );
        assert!(
            pages[preface].ends_with('I') && pages[contents].ends_with("III"),
            "{pdf:?}: {pages:?}"
        );
        assert!(
            pages[first_chapter].ends_with('1') && pages[second_chapter].ends_with('2'),
            "{pdf:?}: {pages:?}"
        );
        assert!(
            pages[second_chapter].starts_with("Second chapter")
                && second_chapter == first_chapter + 1,
            "{pdf:?}: {pages:?}"
        );
    }
}

#[test]
#[ignore = "needs Windows WPS Writer (Kwps.Application) and pdftoppm"]
fn wps_visual_stack_keeps_green_panel() {
    let (typst_pdf, wps_pdf) = render_native_and_wps("render_stack_body");
    assert!(
        green_ink(&typst_pdf) > 2_000,
        "native stack paints a green panel"
    );
    assert!(
        green_ink(&wps_pdf) > 2_000,
        "WPS output must paint the preserved green panel"
    );
}

#[test]
#[ignore = "needs Windows WPS Writer (Kwps.Application) and pdftoppm"]
fn wps_visual_stack_paints_both_measured_lines() {
    let (native, wps) = render_native_and_wps("render_stack_measured_lines");
    for pdf in [&native, &wps] {
        let (dimensions, pixels) = ppm_first_page(pdf);
        let width: usize = dimensions
            .split_whitespace()
            .next()
            .expect("PPM width")
            .parse()
            .expect("numeric PPM width");
        let band = |dominant: usize, other: usize| {
            pixels.chunks_exact(3 * width).position(|row| {
                // At 72 dpi a 72pt line covers 72 pixels. Allow a few pixels
                // for antialiasing along the edges of the 1pt stroke.
                row.chunks_exact(3)
                    .filter(|rgb| rgb[dominant] > rgb[other].saturating_add(40))
                    .count()
                    >= 65
            })
        };
        let red = band(0, 2).unwrap_or_else(|| panic!("missing red line in {pdf:?}"));
        let blue = band(2, 0).unwrap_or_else(|| panic!("missing blue line in {pdf:?}"));
        assert!(red < blue, "lines out of order in {pdf:?}");
    }
}

#[test]
#[ignore = "needs Windows WPS Writer (Kwps.Application) and pdftoppm"]
fn wps_visual_extreme_poster_height_is_clipped() {
    fn raster_height(pdf: &Path) -> u32 {
        ppm_first_page(pdf)
            .0
            .split_ascii_whitespace()
            .nth(1)
            .expect("height in PPM header")
            .parse()
            .expect("numeric PPM height")
    }

    let (typst_pdf, wps_pdf) = render_native_and_wps("render_poster_page_size");
    assert!(
        raster_height(&typst_pdf) > 3_000,
        "native page should be A0"
    );
    assert!(
        raster_height(&wps_pdf) < 200,
        "known WPS 12.1 A0-height import discrepancy"
    );
}
