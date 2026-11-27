//! Shared fixture-conversion and XML-scanning helpers for the area modules in
//! the `integration` test binary. This file does not itself match Cargo's test
//! entry-point patterns; `tests/integration/main.rs` pulls it in explicitly.

use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod fixture_cache;
pub mod ir;
pub(crate) mod wps;

pub use self::fixture_cache::fixture_ir_and_outcome;
use self::fixture_cache::{ConversionPath, converted_fixture};

/// The path to `tests/fixtures/<fixture>.typ`.
pub fn fixture_path(fixture: &str) -> PathBuf {
    PathBuf::from(format!("tests/fixtures/{fixture}.typ"))
}

/// Convert `tests/fixtures/<fixture>.typ` and return the public document model.
pub fn fixture_document(fixture: &str) -> tydocx::ooxml::Document {
    fixture_document_with_options(fixture, &tydocx::ConvertOptions::default())
}

/// Convert a fixture and return both its world and public document model.
pub fn fixture_document_with_world(
    fixture: &str,
) -> (tydocx::TydocxWorld, tydocx::ooxml::Document) {
    let world = tydocx::TydocxWorld::new(&fixture_path(fixture)).unwrap();
    let outcome = tydocx::convert::convert(&world, &tydocx::ConvertOptions::default()).unwrap();
    (world, outcome.document)
}

/// Convert `tests/fixtures/<fixture>.typ` and return the full conversion
/// outcome (document + degradation report).
pub fn fixture_outcome(fixture: &str) -> tydocx::ConvertOutcome {
    fixture_outcome_with_options(fixture, &tydocx::ConvertOptions::default())
}

/// Convert `tests/fixtures/<fixture>.typ` with explicit options and return
/// the full conversion outcome (document + degradation report).
pub fn fixture_outcome_with_options(
    fixture: &str,
    options: &tydocx::ConvertOptions,
) -> tydocx::ConvertOutcome {
    converted_fixture(fixture, &[], options, ConversionPath::Modern)
        .outcome
        .clone()
}

/// Convert `tests/fixtures/<fixture>.typ` with explicit options and return
/// the public document model.
pub fn fixture_document_with_options(
    fixture: &str,
    options: &tydocx::ConvertOptions,
) -> tydocx::ooxml::Document {
    fixture_document_with_font_dirs_and_options(fixture, &[], options)
}

/// Convert `tests/fixtures/<fixture>.typ` with extra font directories loaded and
/// return the public document model.
pub fn fixture_document_with_font_dirs(
    fixture: &str,
    font_dirs: &[PathBuf],
) -> tydocx::ooxml::Document {
    fixture_document_with_font_dirs_and_options(
        fixture,
        font_dirs,
        &tydocx::ConvertOptions::default(),
    )
}

/// Convert a fixture with extra font directories and explicit options.
pub fn fixture_document_with_font_dirs_and_options(
    fixture: &str,
    font_dirs: &[PathBuf],
    options: &tydocx::ConvertOptions,
) -> tydocx::ooxml::Document {
    converted_fixture(fixture, font_dirs, options, ConversionPath::Modern)
        .outcome
        .document
        .clone()
}

/// Every `tests/fixtures/**/*.typ` name, relative without the extension, in
/// sorted order (`conformance/conf-overline` for subdirectory fixtures).
pub fn all_fixtures() -> Vec<String> {
    fn walk(dir: &Path, prefix: &str, names: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).expect("fixture directory") {
            let entry = entry.expect("fixture entry");
            let name = entry.file_name().into_string().expect("utf-8 name");
            let path = entry.path();
            if path.is_dir() {
                walk(&path, &format!("{prefix}{name}/"), names);
            } else if let Some(stem) = name.strip_suffix(".typ") {
                names.push(format!("{prefix}{stem}"));
            }
        }
    }
    let mut names = Vec::new();
    walk(Path::new("tests/fixtures"), "", &mut names);
    names.sort();
    names
}

/// The parts produced by one fixture conversion.
#[derive(Clone)]
pub struct FixturePackage {
    parts: Arc<[(String, Vec<u8>)]>,
    byte_len: usize,
}

impl FixturePackage {
    /// Return the serialized package size in bytes.
    pub fn byte_len(&self) -> usize {
        self.byte_len
    }

    /// Return every part name in package order.
    pub fn part_names(&self) -> impl Iterator<Item = &str> {
        self.parts.iter().map(|(name, _)| name.as_str())
    }

    /// Return a package part as text.
    pub fn part_text(&self, name: &str) -> &str {
        let bytes = &self
            .parts
            .iter()
            .find(|(part_name, _)| part_name == name)
            .unwrap_or_else(|| panic!("package should contain {name:?}"))
            .1;
        std::str::from_utf8(bytes)
            .unwrap_or_else(|error| panic!("{name:?} should be text: {error}"))
    }
}

/// Convert `tests/fixtures/<fixture>.typ` and return all package parts.
pub fn fixture_package(fixture: &str) -> FixturePackage {
    fixture_package_with_options(fixture, &tydocx::ConvertOptions::default())
}

/// Convert `tests/fixtures/<fixture>.typ` through the LEGACY path and
/// return all package parts. Use this to pin legacy-only behavior registered
/// in `docs/fallback-registry.md`.
pub fn fixture_package_legacy(fixture: &str) -> FixturePackage {
    converted_fixture(
        fixture,
        &[],
        &tydocx::ConvertOptions::default(),
        ConversionPath::Legacy,
    )
    .package()
}

/// Convert a fixture with explicit options and return all package parts.
pub fn fixture_package_with_options(
    fixture: &str,
    options: &tydocx::ConvertOptions,
) -> FixturePackage {
    converted_fixture(fixture, &[], options, ConversionPath::Modern).package()
}

/// Package a converted fixture document and return all parts.
pub fn fixture_package_from_document(doc: &tydocx::ooxml::Document) -> FixturePackage {
    let mut buf = Vec::new();
    tydocx::ooxml::write_docx(doc, Cursor::new(&mut buf)).unwrap();

    let byte_len = buf.len();
    let mut archive = zip::ZipArchive::new(Cursor::new(buf)).unwrap();
    let mut parts = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let mut part = archive.by_index(index).unwrap();
        let mut bytes = Vec::new();
        part.read_to_end(&mut bytes).unwrap();
        parts.push((part.name().to_owned(), bytes));
    }
    FixturePackage {
        parts: parts.into(),
        byte_len,
    }
}

/// Convert `tests/fixtures/<fixture>.typ` and return the named docx part.
pub fn fixture_part(fixture: &str, part: &str) -> String {
    fixture_package(fixture).part_text(part).to_owned()
}

/// Convert `tests/fixtures/<fixture>.typ` with extra font directories loaded
/// (e.g. `tests/fonts` for a variable-font fixture) and return the named docx
/// part.
pub fn fixture_part_with_font_dirs(fixture: &str, part: &str, font_dirs: &[PathBuf]) -> String {
    converted_fixture(
        fixture,
        font_dirs,
        &tydocx::ConvertOptions::default(),
        ConversionPath::Modern,
    )
    .package()
    .part_text(part)
    .to_owned()
}

/// Convert `tests/fixtures/<fixture>.typ` and return `word/document.xml`.
pub fn fixture_doc_xml(fixture: &str) -> String {
    fixture_part(fixture, "word/document.xml")
}

/// Render with a fresh world for tests that check determinism or the live clock.
pub fn fixture_doc_xml_uncached(fixture: &str) -> String {
    let (_, document) = fixture_document_with_world(fixture);
    fixture_package_from_document(&document)
        .part_text("word/document.xml")
        .to_owned()
}

/// Convert `tests/fixtures/<fixture>.typ` through the LEGACY path and
/// return `word/document.xml`.
pub fn fixture_doc_xml_legacy(fixture: &str) -> String {
    fixture_package_legacy(fixture)
        .part_text("word/document.xml")
        .to_owned()
}

/// Convert `tests/fixtures/<fixture>.typ` and return `word/styles.xml`.
pub fn fixture_styles_xml(fixture: &str) -> String {
    fixture_part(fixture, "word/styles.xml")
}

/// Return the single `<w:p>...</w:p>` block that contains `needle`.
pub fn paragraph_containing<'a>(doc_xml: &'a str, needle: &str) -> &'a str {
    let pos = doc_xml
        .find(needle)
        .unwrap_or_else(|| panic!("document should contain {needle:?}"));
    let start = doc_xml[..pos]
        .rfind("<w:p>")
        .or_else(|| doc_xml[..pos].rfind("<w:p "))
        .expect("paragraph start");
    let end = doc_xml[pos..]
        .find("</w:p>")
        .map(|e| pos + e)
        .expect("paragraph end");
    &doc_xml[start..end]
}

/// Concatenated `<w:t>` text of every `<w:p>` in document order.
/// Matches both `<w:p>` and `<w:p ...>` (attribute-bearing) paragraphs.
pub fn paragraph_texts(doc_xml: &str) -> Vec<String> {
    doc_xml
        .match_indices("<w:p")
        .filter(|(i, _)| matches!(doc_xml.as_bytes().get(i + 4), Some(b'>' | b' ')))
        .map(|(start, _)| {
            let end = doc_xml[start..]
                .find("</w:p>")
                .map_or(doc_xml.len(), |e| start + e);
            let block = &doc_xml[start..end];
            let mut t = String::new();
            let mut rest = block;
            while let Some(o) = rest.find("<w:t") {
                let after = &rest[o..];
                if !matches!(after.as_bytes().get(4), Some(b'>' | b' ')) {
                    rest = &after[4..];
                    continue;
                }
                if let Some(gt) = after.find('>') {
                    let content = &after[gt + 1..];
                    if let Some(close) = content.find("</w:t>") {
                        t.push_str(&content[..close]);
                        rest = &content[close..];
                        continue;
                    }
                }
                break;
            }
            t
        })
        .collect()
}
