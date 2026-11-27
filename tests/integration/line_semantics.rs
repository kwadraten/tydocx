use super::render_corpus::render_native_and_wps;
use crate::common::wps::run_tool;
use crate::common::{fixture_doc_xml, fixture_outcome};
use std::path::Path;
use std::process::Command;

fn between<'a>(xml: &'a str, start: &str, end: &str) -> &'a str {
    xml.split_once(start)
        .unwrap_or_else(|| panic!("missing {start:?}"))
        .1
        .split_once(end)
        .unwrap_or_else(|| panic!("missing {end:?}"))
        .0
}

#[test]
fn line_semantics_preserve_endpoints_and_stroke_properties() {
    let xml = fixture_doc_xml("line_semantics");
    let h = between(&xml, "Horizontal segment", "Vertical segment");
    assert!(h.contains(r#"cx="812800" cy="25400""#), "{h}");
    assert!(h.contains(r#"<a:pt x="0" y="12700"/>"#), "{h}");
    assert!(h.contains(r#"<a:pt x="812800" y="12700"/>"#), "{h}");
    assert!(h.contains(r#"<a:ln w="25400" cap="rnd">"#), "{h}");
    assert!(h.contains(r#"<a:srgbClr val="A03323""#), "{h}");
    assert!(h.contains(r#"<a:alpha val="50196"/>"#), "{h}");
    assert!(h.contains(r#"<a:ds d="150000" sp="100000"/>"#), "{h}");
    assert!(!h.contains("w:pBdr"), "{h}");
    let v = between(&xml, "Vertical segment", "Diagonal segment");
    assert!(v.contains(r#"cx="19050" cy="533400""#), "{v}");
    assert!(v.contains(r#"<a:pt x="9525" y="0"/>"#), "{v}");
    assert!(v.contains(r#"<a:pt x="9525" y="533400"/>"#), "{v}");
    assert!(v.contains(r#"<a:ln w="19050" cap="sq">"#), "{v}");
    let d = between(&xml, "Diagonal segment", "Decorated text");
    assert!(d.contains(r#"<a:prstGeom prst="line">"#), "{d}");
    assert!(d.contains(r#"cx="527929" cy="304800""#), "{d}");
    assert!(d.contains(r#"<a:ln w="6350" cap="flat">"#), "{d}");
}

#[test]
fn line_semantics_keep_native_text_border_shape_and_math_owners() {
    let xml = fixture_doc_xml("line_semantics");
    let text = between(&xml, "Decorated text", "Rounded shape");
    assert!(text.contains(r#"<w:u w:val="single"/>"#), "{text}");
    assert!(text.contains("<w:strike/>"), "{text}");
    assert!(!text.contains("<w:drawing>"), "{text}");
    let rect = between(&xml, "Rounded shape", "Native edges");
    assert!(rect.contains(r#"<a:prstGeom prst="roundRect">"#), "{rect}");
    assert!(rect.contains(r#"<a:ln w="15875" cap="flat">"#), "{rect}");
    assert!(rect.contains(r#"<a:ds d="240000" sp="160000"/>"#), "{rect}");
    assert!(rect.contains("<a:bevel/>"), "{rect}");
    assert!(rect.contains("<wps:txbx>"), "{rect}");
    let borders = between(&xml, "Native edges", "Math owners");
    assert_eq!(borders.matches("<w:tbl>").count(), 2, "{borders}");
    assert!(borders.contains("<w:tcBorders>"), "{borders}");
    assert!(!borders.contains("<w:drawing>"), "{borders}");
    let math = between(&xml, "Math owners", "Unmapped stroke aspects");
    assert!(math.contains("<m:f>") && math.contains("<m:rad>"), "{math}");
    assert!(!math.contains("<a:ln"), "{math}");
}

#[test]
fn line_semantics_report_unmapped_styling_without_changing_owners() {
    let outcome = fixture_outcome("line_semantics");
    for construct in [
        "underline-style",
        "strikethrough-style",
        "table-border-style",
    ] {
        assert!(
            outcome
                .degradations
                .iter()
                .any(|d| d.construct == construct),
            "{construct}: {:?}",
            outcome.degradations
        );
    }
    assert!(
        outcome
            .degradations
            .iter()
            .any(|d| d.construct == "shape" && d.message.contains("dash pattern phase")),
        "{:?}",
        outcome.degradations
    );
}

#[test]
fn line_semantics_preserve_explicit_parallel_geometry_without_assuming_compound_ratios() {
    let xml = fixture_doc_xml("line_compound_geometry");
    let h = between(&xml, "Aligned horizontal", "Aligned vertical");
    let v = between(&xml, "Aligned vertical", "Different length");
    let alternate = between(&xml, "Another exact gap", "Overlapping strokes");
    for (draw, extent, width, color, a, b) in [
        (
            h,
            r#"cx="1054100" cy="47625""#,
            "15875",
            "85144B",
            r#"x="0" y="7938""#,
            r#"x="0" y="39688""#,
        ),
        (
            v,
            r#"cx="28575" cy="558800""#,
            "9525",
            "0074D9",
            r#"x="4763" y="0""#,
            r#"x="23813" y="0""#,
        ),
        (
            alternate,
            r#"cx="1054100" cy="53975""#,
            "15875",
            "85144B",
            r#"x="0" y="7938""#,
            r#"x="0" y="46038""#,
        ),
    ] {
        assert_eq!(draw.matches("<wps:wsp").count(), 1, "{draw}");
        assert_eq!(draw.matches("<a:lnTo>").count(), 2, "{draw}");
        assert_eq!(draw.matches("<a:ln ").count(), 1, "{draw}");
        assert!(draw.contains(extent), "{draw}");
        assert!(
            draw.contains(&format!(r#"w="{width}" cap="flat""#)),
            "{draw}"
        );
        assert!(
            draw.contains(&format!(r#"<a:srgbClr val="{color}""#)),
            "{draw}"
        );
        assert!(!draw.contains("prstDash"), "{draw}");
        assert!(!draw.contains("cmpd="), "{draw}");
        assert!(draw.contains(a) && draw.contains(b), "{draw}");
    }
    for (start, end, count) in [
        ("Different length", "Different paint", 2),
        ("Different paint", "Another exact gap", 2),
        ("Overlapping strokes", "Nonflat caps", 2),
        ("Nonflat caps", "Longitudinal offset", 2),
        ("Longitudinal offset", "Triple group", 2),
        ("Triple group", "End of line examples", 3),
    ] {
        let draw = between(&xml, start, end);
        assert_eq!(draw.matches("<wps:wsp").count(), count, "{draw}");
        assert!(!draw.contains("cmpd="), "{draw}");
    }
    assert!(
        fixture_outcome("line_compound_geometry")
            .degradations
            .iter()
            .any(|degradation| degradation.construct == "stack-layout")
    );
}

#[test]
#[ignore = "needs Windows WPS Writer (Kwps.Application), Poppler pdftoppm, and Python with PyMuPDF"]
fn wps_compound_line_ink_matches_the_declared_thickness_and_gaps() {
    let (native, wps) = render_native_and_wps("line_compound_geometry");
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/check_compound_line_ink.py");
    run_tool(
        Command::new("python")
            .args(["-E", "-X", "utf8"])
            .arg(script)
            .arg(native)
            .arg(wps),
        "Python with PyMuPDF and Poppler pdftoppm (parallel stroke measurements)",
    );
}
