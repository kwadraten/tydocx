//! Shared helpers for the opt-in WPS rendering tests.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn temp_dir(label: &str) -> PathBuf {
    let base = std::env::temp_dir();
    let opencode = base.join("opencode");
    let root = if opencode.is_dir() { opencode } else { base };
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after Unix epoch")
        .as_nanos();
    let dir = root.join(format!("tydocx-{}-{label}-{nonce}", std::process::id()));
    fs::create_dir(&dir).expect("create test temp directory");
    dir
}

pub(crate) fn run_tool(command: &mut Command, tool: &str) -> Output {
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("{tool} is required for this visual test: {error}"));
    assert!(
        output.status.success(),
        "{tool} failed: {command:?}\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

pub(crate) fn wps_export(docx: &Path, pdf: &Path, dir: &Path, update: bool) {
    static WPS: Mutex<()> = Mutex::new(());
    let _session = WPS
        .lock()
        .expect("serialize WPS COM sessions within the test runner");
    // Use the installed WPS Writer, not the similarly named Word COM class.
    let powershell =
        PathBuf::from(std::env::var_os("WINDIR").expect("WPS visual test needs Windows"))
            .join(r"SysWOW64\WindowsPowerShell\v1.0\powershell.exe");
    let script = dir.join("export.ps1");
    fs::write(
        &script,
        r"param($InputDocx, $OutputPdf, [switch]$RefreshFields)
$ErrorActionPreference = 'Stop'
$app = New-Object -ComObject Kwps.Application
try {
    $app.Visible = $false
    $app.DisplayAlerts = 0
    $doc = $app.Documents.Open($InputDocx, $false, $true, $false)
    try {
        if ($RefreshFields) { [void]$doc.Fields.Update() }
        $doc.ExportAsFixedFormat($OutputPdf, 17, $false, 0, 0)
    } finally { $doc.Close(0) }
} finally { $app.Quit() }
",
    )
    .expect("write WPS COM automation script");
    let mut command = Command::new(powershell);
    command
        .args(["-NoProfile", "-NonInteractive", "-STA", "-File"])
        .arg(script)
        .arg(docx)
        .arg(pdf);
    if update {
        command.arg("-RefreshFields");
    }
    run_tool(
        &mut command,
        "32-bit Windows PowerShell with WPS Writer (Kwps.Application)",
    );
    assert!(pdf.is_file(), "WPS did not create {}", pdf.display());
}
