//! End-to-end CLI behavior: spanned error output and atomic writes.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::common::{fixture_path, paragraph_texts};

fn tydocx_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_tydocx"))
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tydocx-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn run_tydocx(input: &Path, output: Option<&Path>) -> Output {
    run_tydocx_extra(input, output, &[])
}

fn run_tydocx_extra(input: &Path, output: Option<&Path>, extra: &[&str]) -> Output {
    let mut cmd = Command::new(tydocx_bin());
    cmd.arg(input);
    if let Some(output) = output {
        cmd.arg("-o").arg(output);
    }
    for arg in extra {
        cmd.arg(arg);
    }
    cmd.output().unwrap()
}

/// The `word/document.xml` part of a written .docx file.
fn document_xml(docx: &Path) -> String {
    let file = fs::File::open(docx).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let mut part = archive.by_name("word/document.xml").unwrap();
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut part, &mut xml).unwrap();
    xml
}

fn staging_leftovers(dir: &Path) -> Vec<String> {
    fs::read_dir(dir)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".tmp-"))
        .collect()
}

#[test]
fn cli_reports_compile_error_with_span() {
    let dir = temp_dir("error-span");
    let output = dir.join("out.docx");
    let result = run_tydocx(&fixture_path("compile_error_span"), Some(&output));
    assert!(!result.status.success(), "broken fixture should fail");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("error: Typst compilation failed:"),
        "stderr was: {stderr}"
    );
    assert!(
        stderr.contains("compile_error_span.typ:3:10:"),
        "stderr was: {stderr}"
    );
    assert!(!output.exists(), "failed conversion must not leave output");
    assert!(
        staging_leftovers(&dir).is_empty(),
        "staging files left behind: {:?}",
        staging_leftovers(&dir)
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn cli_writes_output_atomically() {
    let dir = temp_dir("atomic-ok");
    let output = dir.join("out.docx");
    let result = run_tydocx(&fixture_path("hello"), Some(&output));
    assert!(
        result.status.success(),
        "stderr was: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(output.exists(), "output was not written");
    assert!(!fs::read(&output).unwrap().is_empty(), "output is empty");
    assert!(
        staging_leftovers(&dir).is_empty(),
        "staging files left behind: {:?}",
        staging_leftovers(&dir)
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn cli_failed_write_leaves_no_partial_output() {
    let dir = temp_dir("atomic-fail");
    let output_dir = dir.join("outdir");
    fs::create_dir(&output_dir).unwrap();
    let result = run_tydocx(&fixture_path("hello"), Some(&output_dir));
    assert!(
        !result.status.success(),
        "writing onto a directory should fail"
    );
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("failed to write"), "stderr was: {stderr}");
    assert!(output_dir.is_dir(), "output directory must be untouched");
    assert!(
        staging_leftovers(&dir).is_empty(),
        "staging files left behind: {:?}",
        staging_leftovers(&dir)
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn cli_compat_modes_produce_identical_output() {
    let dir = temp_dir("compat-modes");
    let mut xmls = Vec::new();
    for mode in ["c1", "c2", "c3"] {
        let output = dir.join(format!("out-{mode}.docx"));
        let result = run_tydocx_extra(
            &fixture_path("complex_paper"),
            Some(&output),
            &["--compat", mode],
        );
        assert!(
            result.status.success(),
            "--compat {mode} should succeed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(
            result.stderr.is_empty(),
            "--compat {mode} should print no warnings on this fixture, stderr was: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(
            String::from_utf8_lossy(&result.stdout).contains("wrote "),
            "--compat {mode} should still report the written file"
        );
        xmls.push(document_xml(&output));
    }
    // No --compat flag means C2.
    let default_output = dir.join("out-default.docx");
    let result = run_tydocx(&fixture_path("complex_paper"), Some(&default_output));
    assert!(
        result.status.success(),
        "default mode should succeed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    xmls.push(document_xml(&default_output));
    // complex_paper carries no C1-gated constructs; C3 wraps its dynamic
    // values in Word fields whose cached results are the C2 literal text.
    assert_eq!(xmls[0], xmls[1], "c1 must match c2 on this fixture");
    assert_eq!(xmls[1], xmls[3], "default must match c2");
    assert_eq!(
        paragraph_texts(&xmls[1]),
        paragraph_texts(&xmls[2]),
        "c3 field cached results must equal the c2 literal text"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn cli_degradation_report_sidecar() {
    let dir = temp_dir("degradation-report");
    let output = dir.join("out.docx");
    let report = dir.join("report.json");
    let result = run_tydocx_extra(
        &fixture_path("issue_c1_misc"),
        Some(&output),
        &[
            "--compat",
            "c1",
            "--degradation-report",
            report.to_str().unwrap(),
        ],
    );
    assert!(
        result.status.success(),
        "c1 conversion should succeed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(output.exists(), "output was not written");
    let json = fs::read_to_string(&report).unwrap();
    assert!(
        json.starts_with("{\"degradations\": ["),
        "report should be a degradations JSON object, got: {json}"
    );
    for construct in [
        "figure-placement",
        "outline-target",
        "outline-indent",
        "list-marker",
        "enum-reversed",
        "image-fit",
        "image-alt",
    ] {
        assert!(
            json.contains(&format!("\"construct\": \"{construct}\"")),
            "report should list {construct}, got: {json}"
        );
    }
    // A C2 run reports nothing: the sidecar is an empty list.
    let report_c2 = dir.join("report-c2.json");
    let result = run_tydocx_extra(
        &fixture_path("issue_c1_misc"),
        Some(&output),
        &[
            "--compat",
            "c2",
            "--degradation-report",
            report_c2.to_str().unwrap(),
        ],
    );
    assert!(
        result.status.success(),
        "c2 conversion should succeed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        fs::read_to_string(&report_c2).unwrap(),
        "{\"degradations\": []}",
        "c2 sidecar should be empty"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn cli_compat_rejects_unknown_mode() {
    let dir = temp_dir("compat-invalid");
    let output = dir.join("out.docx");
    let result = run_tydocx_extra(
        &fixture_path("hello"),
        Some(&output),
        &["--compat", "minimal"],
    );
    assert!(
        !result.status.success(),
        "unknown --compat value should fail"
    );
    assert!(!output.exists(), "failed conversion must not leave output");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("invalid value"), "stderr was: {stderr}");
    let _ = fs::remove_dir_all(&dir);
}
