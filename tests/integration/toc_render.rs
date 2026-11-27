use crate::common::wps::{run_tool, temp_dir, wps_export};
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
#[ignore = "needs Windows WPS Writer (Kwps.Application), Poppler pdftoppm, Python with PyMuPDF, and a local Typst template and package cache"]
fn wps_modern_nju_toc_links_cover_every_character() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join(".");
    // Use the real template, while keeping ordinary CI on package-free fixtures
    // (including toc_row_navigation.typ). Never reuse a preconverted artifact.
    let source = std::env::var_os("TYDOCX_MODERN_NJU_SOURCE").map_or_else(
        || repo.join("tests/acceptance/sources/modern-nju-thesis/template/thesis.typ"),
        PathBuf::from,
    );
    assert!(
        source.is_file(),
        "install the modern-nju-thesis 0.4.1 template at {} or set TYDOCX_MODERN_NJU_SOURCE",
        source.display()
    );
    let dir = temp_dir("modern-nju-toc");
    let docx = dir.join("candidate.docx");
    let pdf = dir.join("wps.pdf");
    run_tool(
        Command::new(env!("CARGO_BIN_EXE_tydocx"))
            .env(
                "TYPST_PACKAGE_CACHE_PATH",
                repo.join("tests/acceptance/package-cache"),
            )
            .arg(source)
            .arg("-o")
            .arg(&docx),
        "tydocx conversion with the local Typst package cache and fonts",
    );
    // Export as opened so a manual TOC refresh cannot repair a broken cache.
    wps_export(&docx, &pdf, &dir, false);
    run_tool(
        Command::new("python")
            .args(["-E", "-X", "utf8"])
            .arg(repo.join("tests/check_modern_nju_toc.py"))
            .arg(&docx)
            .arg(&pdf),
        "Python with PyMuPDF and Poppler pdftoppm (modern-nju-thesis DOCX and WPS PDF checks)",
    );
}
