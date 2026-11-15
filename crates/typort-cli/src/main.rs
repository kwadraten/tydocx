use std::error::Error;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process;

use clap::Parser;
use typst::World as _;
use typst::diag::SourceDiagnostic;
use typst::syntax::{FileId, VirtualRoot};

#[derive(Parser)]
#[command(
    name = "typort",
    version,
    about = "Convert Typst documents to Word (.docx)"
)]
struct Cli {
    /// Input .typ file
    input: PathBuf,

    /// Output .docx file
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Preset name: loads `<name>.toml` from the standard preset search path
    /// (none are bundled; supply your own)
    #[arg(long)]
    preset: Option<String>,
}

fn main() {
    let cli = Cli::parse();

    let output_path = cli
        .output
        .unwrap_or_else(|| cli.input.with_extension("docx"));

    let world = typort_core::TyportWorld::new(&cli.input).unwrap_or_else(|e| {
        eprintln!("error: failed to read input: {e}");
        process::exit(1);
    });

    let mut doc = typort_core::convert(&world).unwrap_or_else(|errors| {
        eprintln!("error: Typst compilation failed:");
        for diagnostic in &errors {
            eprintln!("  {}", render_diagnostic(&world, diagnostic));
        }
        process::exit(1);
    });

    // Apply preset if specified
    if let Some(preset_name) = &cli.preset {
        let preset =
            typort_presets::load_preset_from_search_path(preset_name).unwrap_or_else(|e| {
                eprintln!("error: {e}");
                process::exit(1);
            });
        preset.apply(&mut doc);
    }

    write_docx_atomically(&doc, &output_path).unwrap_or_else(|e| {
        eprintln!("error: failed to write .docx: {e}");
        process::exit(1);
    });

    println!("wrote {}", output_path.display());
}

/// Render one compile diagnostic for the CLI: `file:line:col: message`, or
/// the bare message when the span points nowhere.
///
/// The world lookup (file id to source) is the only impure part; the
/// formatting itself is the pure `typort_core::diag::format_diagnostic`.
fn render_diagnostic(world: &typort_core::TyportWorld, diagnostic: &SourceDiagnostic) -> String {
    let file = diagnostic.span.id().and_then(|id| {
        let source = world.source(id).ok()?;
        Some((display_path(world, id), source))
    });
    let file = file.as_ref().map(|(name, source)| (name.as_str(), source));
    typort_core::diag::format_diagnostic(diagnostic, file)
}

/// Display path for a diagnostic's file: the path under the world root for
/// project files, `spec/path` for package files.
fn display_path(world: &typort_core::TyportWorld, id: FileId) -> String {
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
    doc: &typort_ooxml::Document,
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
    doc: &typort_ooxml::Document,
    output: &Path,
    staging: &Path,
) -> Result<(), Box<dyn Error>> {
    // `write_docx` takes ownership of the file, so it is closed before the
    // rename below (renaming an open file fails on some platforms).
    let file = File::create(staging)?;
    typort_ooxml::write_docx(doc, file)?;
    fs::rename(staging, output)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
