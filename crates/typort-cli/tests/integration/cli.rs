//! End-to-end CLI behavior: spanned error output and atomic writes.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::common::fixture_path;

fn typort_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_typort"))
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("typort-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn run_typort(input: &Path, output: Option<&Path>) -> Output {
    let mut cmd = Command::new(typort_bin());
    cmd.arg(input);
    if let Some(output) = output {
        cmd.arg("-o").arg(output);
    }
    cmd.output().unwrap()
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
    let result = run_typort(&fixture_path("compile_error_span"), Some(&output));
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
    let result = run_typort(&fixture_path("hello"), Some(&output));
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
    let result = run_typort(&fixture_path("hello"), Some(&output_dir));
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
