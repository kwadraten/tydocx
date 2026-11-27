use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn docs_keep_references_not_reports_or_plans() {
    // Render and validation behavior belongs in executable tests. Keep docs for
    // architecture, standards, and contributor references, not transient artifacts.
    fn forbidden_files(dir: &Path, found: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap_or_else(|error| panic!("{dir:?}: {error}")) {
            let entry = entry.expect("read docs entry");
            if entry.file_type().expect("read docs entry type").is_dir() {
                forbidden_files(&entry.path(), found);
            } else {
                let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                if ["report", "plan", "verification", "audit"]
                    .into_iter()
                    .any(|term| name.contains(term))
                {
                    found.push(entry.path());
                }
            }
        }
    }

    let docs = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs");
    let mut found = Vec::new();
    forbidden_files(&docs, &mut found);
    found.sort();
    assert!(
        found.is_empty(),
        "non-reference files under docs: {found:#?}"
    );
}
