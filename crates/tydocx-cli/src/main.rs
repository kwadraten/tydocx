use std::error::Error;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process;

use clap::Parser;
use tydocx_core::{CompatMode, Degradation};
use typst::World as _;
use typst::diag::SourceDiagnostic;
use typst::syntax::{FileId, VirtualRoot};

#[derive(Parser)]
#[command(
    name = "tydocx",
    version,
    about = "Convert Typst documents to Word (.docx)"
)]
struct Cli {
    /// Input .typ file
    input: PathBuf,

    /// Output .docx file
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Compat mode: c1 (minimal: intersection only), c2 (graphical,
    /// default), c3 (dynamic: field codes on top of c2)
    #[arg(long, value_parser = parse_compat, default_value = "c2")]
    compat: CompatMode,

    /// Word bibliography style file (.xsl). Sets Sources.xml's
    /// SelectedStyle/StyleName so Word re-renders citations and the
    /// bibliography with that style on field update
    #[arg(long, value_name = "STYLE.xsl")]
    bibliography_style: Option<PathBuf>,

    /// Write the full deduplicated degradation list as JSON to this path
    /// (no 50-line cap — the cap is a display concern, the report is the
    /// complete record)
    #[arg(long)]
    degradation_report: Option<PathBuf>,
}

/// Parse a `--compat` value: only the three short names are accepted.
fn parse_compat(value: &str) -> Result<CompatMode, String> {
    CompatMode::from_short_name(value)
        .ok_or_else(|| format!("invalid compat mode {value:?}: expected c1, c2 or c3"))
}

fn main() {
    let cli = Cli::parse();

    let output_path = cli
        .output
        .unwrap_or_else(|| cli.input.with_extension("docx"));

    let world = tydocx_core::TydocxWorld::new(&cli.input).unwrap_or_else(|e| {
        eprintln!("error: failed to read input: {e}");
        process::exit(1);
    });

    let mut options = tydocx_core::ConvertOptions::new(cli.compat);
    options.bibliography_style = cli
        .bibliography_style
        .map(|p| p.to_string_lossy().into_owned());
    let outcome = tydocx_core::convert(&world, &options).unwrap_or_else(|errors| {
        eprintln!("error: Typst compilation failed:");
        for diagnostic in &errors {
            eprintln!("  {}", render_diagnostic(&world, diagnostic));
        }
        process::exit(1);
    });
    let doc = outcome.document;

    write_docx_atomically(&doc, &output_path).unwrap_or_else(|e| {
        eprintln!("error: failed to write .docx: {e}");
        process::exit(1);
    });

    print_degradations(&world, &outcome.degradations);

    if let Some(report_path) = &cli.degradation_report {
        write_degradation_report(&world, &outcome.degradations, report_path).unwrap_or_else(|e| {
            eprintln!("error: failed to write degradation report: {e}");
            process::exit(1);
        });
    }

    println!("wrote {}", output_path.display());
}

/// Print conversion degradations to stderr (§4.1), after the write so a
/// failed conversion prints no warnings. Deduplicated and capped; the span
/// of each warning is resolved to `file:line:col` here, at the boundary —
/// the formatting itself is the pure
/// `tydocx_core::compat::format_degradation`.
fn print_degradations(world: &tydocx_core::TydocxWorld, degradations: &[Degradation]) {
    let deduped = tydocx_core::compat::deduplicate_degradations(degradations.to_vec());
    let lines: Vec<String> = deduped
        .iter()
        .map(|degradation| render_degradation(world, degradation))
        .collect();
    let (printed, summary) = tydocx_core::compat::cap_warning_lines(&lines);
    for line in printed {
        eprintln!("{line}");
    }
    if let Some(summary) = summary {
        eprintln!("{summary}");
    }
}

/// Render one degradation warning for the CLI: `warning: file:line:col:
/// <construct>: <message>`, or without a location when the span points
/// nowhere.
///
/// The world lookup (file id to source) is the only impure part; the
/// formatting itself is the pure `tydocx_core::compat::format_degradation`.
fn render_degradation(world: &tydocx_core::TydocxWorld, degradation: &Degradation) -> String {
    let location = resolve_degradation_location(world, degradation);
    tydocx_core::compat::format_degradation(degradation, location.as_deref())
}

/// Resolve a degradation's span to a `file:line:col` string, or `None` when
/// the span points nowhere. Shared by the stderr printer and the sidecar
/// report writer.
fn resolve_degradation_location(
    world: &tydocx_core::TydocxWorld,
    degradation: &Degradation,
) -> Option<String> {
    let span = degradation.span?;
    let id = span.id()?;
    let source = world.source(id).ok()?;
    let (line, column) = tydocx_core::diag::span_line_col(&source, span)?;
    Some(format!("{}:{line}:{column}", display_path(world, id)))
}

/// Write the full deduplicated degradation list as JSON to `path` (§4.1/§8).
/// Unlike the stderr print, the sidecar is NOT capped: the cap is a display
/// concern, the report is the complete record. A failed write reports to
/// stderr and exits 1 — after the .docx itself was written successfully.
fn write_degradation_report(
    world: &tydocx_core::TydocxWorld,
    degradations: &[Degradation],
    path: &Path,
) -> Result<(), Box<dyn Error>> {
    let deduped = tydocx_core::compat::deduplicate_degradations(degradations.to_vec());
    let entries: Vec<_> = deduped
        .iter()
        .map(|degradation| {
            let location = resolve_degradation_location(world, degradation);
            tydocx_core::compat::DegradationReportEntry::from_resolved(location, degradation)
        })
        .collect();
    let json = tydocx_core::compat::degradation_report_json(&entries);
    fs::write(path, json)?;
    Ok(())
}

/// Render one compile diagnostic for the CLI: `file:line:col: message`, or
/// the bare message when the span points nowhere.
///
/// The world lookup (file id to source) is the only impure part; the
/// formatting itself is the pure `tydocx_core::diag::format_diagnostic`.
fn render_diagnostic(world: &tydocx_core::TydocxWorld, diagnostic: &SourceDiagnostic) -> String {
    let file = diagnostic.span.id().and_then(|id| {
        let source = world.source(id).ok()?;
        Some((display_path(world, id), source))
    });
    let file = file.as_ref().map(|(name, source)| (name.as_str(), source));
    tydocx_core::diag::format_diagnostic(diagnostic, file)
}

/// Display path for a diagnostic's file: the path under the world root for
/// project files, `spec/path` for package files.
fn display_path(world: &tydocx_core::TydocxWorld, id: FileId) -> String {
    match id.root() {
        VirtualRoot::Project => world
            .root()
            .join(id.vpath().get_without_slash())
            .display()
            .to_string(),
        VirtualRoot::Package(spec) => {
            format!("{spec}/{}", id.vpath().get_without_slash())
        }
    }
}

/// Derive the staging path used for an atomic write of `output`: same
/// directory, same file name with `.tmp-{unique}` appended.
///
/// Pure: the caller supplies `unique` (the CLI passes its process id), so the
/// same inputs always give the same path. A path with no file name (for
/// example one ending in `..`) falls back to a `tmp-{unique}` entry inside it.
fn staging_path(output: &Path, unique: &str) -> PathBuf {
    match output.file_name() {
        Some(name) => {
            let mut name = name.to_os_string();
            name.push(format!(".tmp-{unique}"));
            output.with_file_name(name)
        }
        None => output.join(format!("tmp-{unique}")),
    }
}

/// Write the `.docx` to a staging file in the same directory, then rename it
/// over `output`. A failed write or rename removes the staging file, so no
/// partial output is ever left behind.
fn write_docx_atomically(
    doc: &tydocx_ooxml::Document,
    output: &Path,
) -> Result<(), Box<dyn Error>> {
    let staging = staging_path(output, &process::id().to_string());
    let result = write_then_rename(doc, output, &staging);
    if result.is_err() {
        // Best effort: never leave a partial output behind.
        let _ = fs::remove_file(&staging);
    }
    result
}

fn write_then_rename(
    doc: &tydocx_ooxml::Document,
    output: &Path,
    staging: &Path,
) -> Result<(), Box<dyn Error>> {
    // `write_docx` takes ownership of the file, so it is closed before the
    // rename below (renaming an open file fails on some platforms).
    let file = File::create(staging)?;
    tydocx_ooxml::write_docx(doc, file)?;
    fs::rename(staging, output)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_compat_accepts_short_names_only() {
        assert_eq!(parse_compat("c1"), Ok(CompatMode::C1));
        assert_eq!(parse_compat("c2"), Ok(CompatMode::C2));
        assert_eq!(parse_compat("c3"), Ok(CompatMode::C3));
        assert!(parse_compat("minimal").is_err());
        assert!(parse_compat("C2").is_err());
        assert!(parse_compat("").is_err());
    }

    #[test]
    fn staging_path_appends_tmp_suffix() {
        assert_eq!(
            staging_path(Path::new("out.docx"), "123"),
            PathBuf::from("out.docx.tmp-123")
        );
        assert_eq!(
            staging_path(Path::new("out"), "7"),
            PathBuf::from("out.tmp-7")
        );
        assert_eq!(
            staging_path(Path::new("/a/b/report.docx"), "9"),
            PathBuf::from("/a/b/report.docx.tmp-9")
        );
        assert_eq!(
            staging_path(Path::new(".hidden"), "1"),
            PathBuf::from(".hidden.tmp-1")
        );
    }

    #[test]
    fn staging_path_without_file_name_uses_entry_inside() {
        assert_eq!(
            staging_path(Path::new("a/b/.."), "1"),
            PathBuf::from("a/b/../tmp-1")
        );
    }
}
