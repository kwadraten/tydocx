//! Conformance suite (step 5): canonical fingerprint, paragraph census
//! (REACH), semantic digest round-trips, preservation pins, and strict
//! package validation. Method mirrors EigenPal's `core` oracles
//! (canonical tree fingerprint + save-and-reopen digest), reimplemented
//! in `tydocx::ooxml::canonical`.
//!
//! Tests that need LibreOffice or python-docx are `#[ignore]`d — those
//! tools are not in CI — and panic loudly when a tool is missing, the
//! same convention the visual-regression tests use.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use crate::common;

/// Whitespace-insensitive comparison text: the writer inserts spaces at run
/// boundaries (between a footnote marker and following punctuation, between
/// differently-styled CJK runs), so layout whitespace is not content.
fn normalize(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

fn require_tool(name: &str) {
    let found = std::process::Command::new(name)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    assert!(found, "needs {name}; install it to run this opt-in test");
}

fn write_fixture_docx(fixture: &str, dir: &Path) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(format!("{fixture}.docx"));
    let file = std::fs::File::create(&path).unwrap();
    tydocx::ooxml::write_docx(
        &common::fixture_document(fixture),
        std::io::BufWriter::new(file),
    )
    .unwrap();
    path
}

/// A private LibreOffice profile per output dir: parallel opt-in tests share
/// the default profile otherwise, and concurrent `soffice` calls fail.
fn lo_profile_arg(out_dir: &Path) -> String {
    format!(
        "-env:UserInstallation=file://{}",
        out_dir.join("lo-profile").display()
    )
}

fn libreoffice_convert(docx: &Path, out_dir: &Path) -> PathBuf {
    std::fs::create_dir_all(out_dir).unwrap();
    let status = std::process::Command::new("soffice")
        .arg(lo_profile_arg(out_dir))
        .args(["--headless", "--convert-to", "docx", "--outdir"])
        .arg(out_dir)
        .arg(docx)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("spawn soffice");
    assert!(status.success(), "LibreOffice failed on {docx:?}");
    let out = out_dir.join(docx.file_name().unwrap());
    assert!(out.exists(), "LibreOffice produced no output for {docx:?}");
    out
}

fn read_part(docx: &Path, part: &str) -> Option<String> {
    let file = std::fs::File::open(docx).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let mut entry = archive.by_name(part).ok()?;
    let mut text = String::new();
    std::io::Read::read_to_string(&mut entry, &mut text).ok()?;
    Some(text)
}

fn python_docx_opens(docx: &Path) -> bool {
    let script = r#"
import sys
from docx import Document
doc = Document(sys.argv[1])
sum(1 for _ in doc.paragraphs)
"#;
    std::process::Command::new("python3")
        .args(["-c", script])
        .arg(docx)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[test]
fn conformance_fingerprint_is_deterministic_and_id_agnostic() {
    for fixture in ["general_elements", "issue_nested_table_alignment"] {
        let fingerprint = || {
            tydocx::ooxml::canonical::canonical_fingerprint(&common::fixture_doc_xml_uncached(
                fixture,
            ))
            .unwrap()
        };
        let a = fingerprint();
        let b = fingerprint();
        assert_eq!(a, b, "{fixture}: fingerprint must be deterministic");
    }
}

/// Entries the corpus walk finds but cannot convert as a root document:
/// `compile_error_span` fails Typst compilation on purpose, `paged_compile_failure`
/// and `fields_time_error` fail it as their tests' subject (the surfaced
/// diagnostic is the assertion), and `issue_import_parent_dir/d` is an import
/// module that escapes the sandbox root when compiled standalone — each
/// excluded by name, not silently.
const EXPECTED_COMPILE_ERRORS: &[&str] = &[
    "compile_error_span",
    "fields_time_error",
    "issue_import_parent_dir/d",
    "paged_compile_failure",
];

/// REACH census over the fixture corpus: every IR paragraph's text appears
/// in the output, and every output paragraph's text traces to IR content or
/// to a named generated/degraded construct.
#[test]
fn conformance_paragraph_census() {
    let failures = common::all_fixtures()
        .par_iter()
        .map(|fixture| {
            let mut failures = Vec::new();
            if EXPECTED_COMPILE_ERRORS.contains(&fixture.as_str()) {
                return failures;
            }
            let fixture = fixture.as_str();
            let (ir, outcome) = common::fixture_ir_and_outcome(fixture);
            let package = common::fixture_package_from_document(&outcome.document);
            // `document.xml` paragraphs are the strict half of the census: they
            // must trace to the IR. IR texts may legitimately land in other
            // parts — footnote bodies (sourced from the companion
            // `PagedDocument`) go to `word/footnotes.xml` — so the IR -> output
            // check accepts any word part as a landing zone. Paragraphs in
            // non-document parts are the converse named source
            // (`paged-footnote-body`, `page-furniture`) and skip tracing.
            let mut body_paras: Vec<String> = Vec::new();
            let mut all_part_paras: Vec<String> = Vec::new();
            for part in package.part_names() {
                if !part.starts_with("word/") || !part.ends_with(".xml") {
                    continue;
                }
                let texts = tydocx::ooxml::canonical::paragraph_texts(package.part_text(part))
                    .unwrap_or_default();
                if part == "word/document.xml" {
                    body_paras = texts;
                } else {
                    all_part_paras.extend(texts);
                }
            }
            let texts = common::ir::ir_texts(&ir);
            let out_joined = normalize(&format!(
                "{}\n{}",
                body_paras.join("\n"),
                all_part_paras.join("\n")
            ));
            let out_paras = &body_paras;
            // Bibliography entry bodies are IR content too: they used to render
            // only out-of-band (Word's bibliography field), but the visible
            // entry cache now also emits them as document paragraphs.
            let ir_joined = normalize(&format!(
                "{}\n{}",
                texts.paragraphs.join("\n"),
                texts.out_of_band.join("\n")
            ));

            // IR -> output: every non-empty IR paragraph reaches the package.
            for para in &texts.paragraphs {
                let text = normalize(para);
                if text.is_empty() || out_joined.contains(&text) {
                    continue;
                }
                // The paragraph's text may be a strict subset of an output
                // paragraph (a generated prefix was prepended).
                if out_paras.iter().any(|out| {
                    let out = normalize(out);
                    out.contains(&text) && out.len() >= text.len()
                }) {
                    continue;
                }
                failures.push(format!("{fixture}: IR paragraph dropped: {text:?}"));
            }
            // `out_of_band` texts (bibliography bodies) are excluded above: they
            // render from `customXml/sources.xml` inside Word — a named
            // degradation of this backend, not a drop.

            // output -> IR: every non-empty output paragraph traces to IR text,
            // possibly after stripping generated fragments: a TOC entry is its
            // heading title (IR text) plus a generated page number, so one
            // generated prefix and/or suffix may be stripped.
            for out in out_paras {
                let text = normalize(out);
                if text.is_empty() || ir_joined.contains(&text) {
                    continue;
                }
                let mut candidates = vec![text.as_str()];
                for generated in &texts.generated {
                    let frag = normalize(generated);
                    if frag.is_empty() {
                        continue;
                    }
                    if let Some(rest) = text.strip_prefix(frag.as_str()) {
                        candidates.push(rest);
                    }
                    if let Some(rest) = text.strip_suffix(frag.as_str()) {
                        candidates.push(rest);
                    }
                }
                let mut paired = candidates.clone();
                for base in candidates {
                    for generated in &texts.generated {
                        let frag = normalize(generated);
                        if frag.is_empty() {
                            continue;
                        }
                        if let Some(rest) = base.strip_prefix(frag.as_str()) {
                            paired.push(rest);
                        }
                        if let Some(rest) = base.strip_suffix(frag.as_str()) {
                            paired.push(rest);
                        }
                    }
                }
                if paired.iter().any(|c| c.is_empty() || ir_joined.contains(c)) {
                    continue;
                }
                failures.push(format!("{fixture}: untraceable output paragraph: {text:?}"));
            }
            failures
        })
        .collect::<Vec<_>>()
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    assert!(
        failures.is_empty(),
        "census failures:\n{}",
        failures.join("\n")
    );
}

/// Every `IrBlock` / `IrInline` variant must be exercised by at least one
/// fixture in the corpus — the digest audit is only meaningful if the corpus
/// reaches the whole inventory.
#[test]
fn conformance_ir_variant_coverage() {
    const BLOCK_ALL: &[&str] = &[
        "Paragraph",
        "Heading",
        "Table",
        "Grid",
        "Figure",
        "List",
        "Enum",
        "Equation",
        "Quote",
        "Raw",
        "Outline",
        "Bibliography",
        "Place",
        "PageBreak",
        "Shape",
        "Columns",
    ];
    const INLINE_ALL: &[&str] = &[
        "Text",
        "Link",
        "Ref",
        "Cite",
        "FootnoteMarker",
        "LineBreak",
        "Space",
        "Image",
        "Math",
        "PageBreak",
        "ColumnBreak",
        "Tab",
        "PageField",
        "Shape",
        "Overline",
    ];
    // Variants the realize pipeline can never produce: `PagebreakElem` is
    // collected as `IrBlock::PageBreak` (a paragraph boundary), and no
    // construct lowers to an inline `w:tab` run — tab stops are paragraph
    // properties. Both stay in the `IrInline` vocabulary because `lower`
    // still handles them for the shared IR model.
    const INLINE_UNPRODUCED: &[&str] = &["PageBreak", "Tab"];
    let (blocks, inlines) = common::all_fixtures()
        .par_iter()
        .map(|fixture| {
            if EXPECTED_COMPILE_ERRORS.contains(&fixture.as_str()) {
                return (BTreeSet::new(), BTreeSet::new());
            }
            let (ir, _) = common::fixture_ir_and_outcome(fixture);
            (
                common::ir::block_variants(&ir),
                common::ir::inline_variants(&ir),
            )
        })
        .reduce(
            || (BTreeSet::new(), BTreeSet::new()),
            |(mut blocks, mut inlines), (next_blocks, next_inlines)| {
                blocks.extend(next_blocks);
                inlines.extend(next_inlines);
                (blocks, inlines)
            },
        );
    let missing_blocks: Vec<&&str> = BLOCK_ALL.iter().filter(|b| !blocks.contains(**b)).collect();
    let missing_inlines: Vec<&&str> = INLINE_ALL
        .iter()
        .filter(|b| !inlines.contains(**b) && !INLINE_UNPRODUCED.contains(b))
        .collect();
    assert!(
        missing_blocks.is_empty() && missing_inlines.is_empty(),
        "corpus misses IrBlock {missing_blocks:?} and IrInline {missing_inlines:?}"
    );
}

#[test]
fn conformance_preservation_constructs_emitted() {
    let doc_xml = common::fixture_doc_xml("preserve_inert_constructs");
    for marker in [
        r#"<w:fldSimple w:instr="HYPERLINK"#,
        r#"<w:bookmarkStart w:id="0" w:name="targets"/>"#,
        r#" REF targets \h "#,
        "<w:footnoteReference",
    ] {
        assert!(
            doc_xml.contains(marker),
            "missing preserved construct {marker}"
        );
    }
}

/// Element classes LibreOffice provably rewrites rather than drops, with
/// the evidence the reopened file must still carry. `w:fldSimple` becomes
/// complex `w:fldChar` fields; table-level `w:tblBorders` is normalized to
/// per-cell `w:tcBorders` (`w:left`/`w:right` become `w:start`/`w:end`);
/// an empty `w:footnotePr` is elided. `w:left`/`w:right` in `w:ind` and
/// `w:tblBorders` become `w:start`/`w:end`. `m:*` needs no evidence: this
/// environment's LibreOffice has no `libreoffice-math` component, so it
/// imports OMML as nothing — an oracle limitation, not a drop tydocx can
/// observe.
fn lo_rewrite_evidence(class: &str) -> Option<&'static [&'static str]> {
    match class {
        "w:fldSimple" => Some(&["w:fldChar", "w:hyperlink"]),
        "w:tblBorders" | "w:insideH" | "w:insideV" => Some(&["w:tcBorders"]),
        // `w:left`/`w:right` appear in `w:tblBorders` sides and `w:ind`
        // indents; LibreOffice rewrites both to `w:start`/`w:end`.
        "w:left" => Some(&["w:start", "w:tcBorders"]),
        "w:right" => Some(&["w:end", "w:tcBorders"]),
        _ => None,
    }
}

/// Save-and-reopen: convert, write the docx, let LibreOffice rewrite it, and
/// require (i) every element class in the original survives or is a named
/// LibreOffice rewrite and (ii) the `w:t` paragraph-text multiset is
/// unchanged.
#[test]
#[ignore = "needs libreoffice; run with --ignored"]
fn conformance_roundtrip_libreoffice() {
    require_tool("soffice");
    let tmp = std::env::temp_dir().join("tydocx_roundtrip");
    let mut failures = Vec::new();
    for fixture in [
        "general_elements",
        "preserve_inert_constructs",
        "issue_nested_table_alignment",
        "math_test",
        "edge_landscape_pages",
        "style_footnotes",
    ] {
        let original = write_fixture_docx(fixture, &tmp.join("in"));
        let reopened = libreoffice_convert(&original, &tmp.join("out"));
        let original_xml = read_part(&original, "word/document.xml").unwrap();
        let reopened_xml = read_part(&reopened, "word/document.xml").unwrap();

        let original_classes = tydocx::ooxml::canonical::element_classes(&original_xml).unwrap();
        let reopened_classes = tydocx::ooxml::canonical::element_classes(&reopened_xml).unwrap();
        for class in &original_classes {
            if reopened_classes.contains(class) {
                continue;
            }
            if let Some(evidence) = lo_rewrite_evidence(class) {
                if evidence.iter().any(|e| reopened_classes.contains(*e)) {
                    continue;
                }
                failures.push(format!(
                    "{fixture}: {class} dropped and its LibreOffice rewrite \
                     {evidence:?} is absent too"
                ));
                continue;
            }
            if class.starts_with("m:") {
                continue;
            }
            if class == "w:footnotePr" && !original_xml.contains("<w:numFmt") {
                continue;
            }
            failures.push(format!(
                "{fixture}: element class {class} dropped on reopen"
            ));
        }

        // The paragraph `w:t` texts are the save-and-reopen oracle:
        // LibreOffice may rewrite markup and split runs, but the text the
        // document carries must be identical. `m:t` math runs are excluded —
        // the oracle cannot import OMML at all here.
        let mut original_texts: Vec<String> =
            tydocx::ooxml::canonical::paragraph_texts(&original_xml)
                .unwrap()
                .iter()
                .map(|t| normalize(t))
                .filter(|t| !t.is_empty())
                .collect();
        let mut reopened_texts: Vec<String> =
            tydocx::ooxml::canonical::paragraph_texts(&reopened_xml)
                .unwrap()
                .iter()
                .map(|t| normalize(t))
                .filter(|t| !t.is_empty())
                .collect();
        original_texts.sort();
        reopened_texts.sort();
        if original_texts != reopened_texts {
            failures.push(format!(
                "{fixture}: paragraph text differs after reopen:\n  original: {original_texts:?}\n  reopened: {reopened_texts:?}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "round-trip failures:\n{}",
        failures.join("\n")
    );
}

/// Preservation through save + reopen: the inert constructs emitted for
/// `preserve_inert_constructs` must still be present after LibreOffice
/// rewrites the package.
#[test]
#[ignore = "needs libreoffice; run with --ignored"]
fn conformance_preservation_survives_reopen() {
    require_tool("soffice");
    let tmp = std::env::temp_dir().join("tydocx_preserve");
    let original = write_fixture_docx("preserve_inert_constructs", &tmp.join("in"));
    let reopened = libreoffice_convert(&original, &tmp.join("out"));
    let xml = read_part(&reopened, "word/document.xml").unwrap();
    // LibreOffice rewrites `w:fldSimple` HYPERLINK into a `w:hyperlink`
    // element plus an external relationship, and REF fields into `w:fldChar`
    // runs — the constructs survive in the equivalent reopened form.
    for marker in [
        "<w:hyperlink",
        "targets",
        "footnoteReference",
        "hyperlink body",
    ] {
        assert!(xml.contains(marker), "reopened package lost {marker}");
    }
    let rels = read_part(&reopened, "word/_rels/document.xml.rels").unwrap_or_default();
    let footnotes = read_part(&reopened, "word/footnotes.xml").unwrap_or_default();
    assert!(
        rels.contains("example.com/target"),
        "reopened package lost the hyperlink relationship"
    );
    assert!(
        footnotes.contains("Footnote body text."),
        "reopened package lost the footnote body"
    );
}

/// Strict validation harness: every fixture docx is a package python-docx
/// opens, survives a LibreOffice conversion without error, and carries a
/// degradation report with a construct and message per entry.
#[test]
#[ignore = "needs libreoffice + python-docx; run with --ignored"]
fn conformance_strict_package_validation() {
    require_tool("soffice");
    require_tool("python3");
    let tmp = std::env::temp_dir().join("tydocx_strict");
    let mut failures = Vec::new();
    for fixture in common::all_fixtures() {
        if EXPECTED_COMPILE_ERRORS.contains(&fixture.as_str()) {
            continue;
        }
        let (_, outcome) = common::fixture_ir_and_outcome(&fixture);
        let path = tmp.join("in").join(format!("{fixture}.docx"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = std::fs::File::create(&path).unwrap();
        tydocx::ooxml::write_docx(&outcome.document, std::io::BufWriter::new(file)).unwrap();

        if !python_docx_opens(&path) {
            failures.push(format!("{fixture}: python-docx cannot open the package"));
        }
        let status = std::process::Command::new("soffice")
            .arg(lo_profile_arg(&tmp.join("out")))
            .args(["--headless", "--convert-to", "docx", "--outdir"])
            .arg(tmp.join("out"))
            .arg(&path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        // soffice writes the output under the input's basename, not the
        // fixture's subdirectory path.
        let basename = format!("{}.docx", fixture.rsplit('/').next().unwrap());
        match status {
            Ok(s) if s.success() && tmp.join("out").join(&basename).exists() => {}
            _ => failures.push(format!("{fixture}: LibreOffice conversion failed")),
        }
        for degradation in &outcome.degradations {
            if degradation.construct.is_empty() || degradation.message.is_empty() {
                failures.push(format!(
                    "{fixture}: degradation report entry lacks a construct or message"
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "strict validation failures:\n{}",
        failures.join("\n")
    );
}
