//! Process-local memoization for checked-in fixtures and their read-only inputs.
//! Tests that need a fresh render, a custom world, or diagnostics convert directly.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

use tydocx::ir::IrDocument;
use tydocx::{ConvertOptions, ConvertOutcome, TydocxWorld, convert};

use super::{FixturePackage, fixture_package_from_document, fixture_path};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum ConversionPath {
    Modern,
    Legacy,
}

#[derive(Clone, PartialEq, Eq)]
struct FixtureKey {
    path: PathBuf,
    options: ConvertOptions,
    font_dirs: Vec<PathBuf>,
    conversion_path: ConversionPath,
}

impl FixtureKey {
    fn new(
        fixture: &str,
        font_dirs: &[PathBuf],
        options: &ConvertOptions,
        conversion_path: ConversionPath,
    ) -> Self {
        Self {
            path: normalized_fixture_path(fixture),
            options: options.clone(),
            // Keep the original search paths and their order: the world appends
            // each directory's fonts in that order.
            font_dirs: font_dirs.to_vec(),
            conversion_path,
        }
    }

    fn convert(&self) -> ConvertedFixture {
        let world = TydocxWorld::with_font_dirs(&self.path, &self.font_dirs).unwrap();
        let outcome = match self.conversion_path {
            ConversionPath::Modern => convert::convert(&world, &self.options),
            ConversionPath::Legacy => convert::convert_legacy(&world, &self.options),
        }
        .unwrap();
        ConvertedFixture {
            outcome,
            package: OnceLock::new(),
        }
    }
}

impl Hash for FixtureKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Exhaustive destructuring makes new ConvertOptions fields a compile
        // error here rather than silently omitting them from the cache key.
        let Self {
            path,
            options:
                ConvertOptions {
                    compat,
                    bibliography_style,
                },
            font_dirs,
            conversion_path,
        } = self;
        (path, compat, bibliography_style, font_dirs, conversion_path).hash(state);
    }
}

fn normalized_fixture_path(fixture: &str) -> PathBuf {
    fixture_path(fixture)
        .canonicalize()
        .unwrap_or_else(|error| panic!("cannot open fixture {fixture:?}: {error}"))
}

type Slot<T> = Arc<OnceLock<Arc<T>>>;

/// The map lock only finds/creates a slot. Each slot separately ensures that
/// one successful initialization serves all callers; a panic leaves it empty.
struct FixtureCache<K, T>(Mutex<HashMap<K, Slot<T>>>);

impl<K: Clone + Eq + Hash, T> FixtureCache<K, T> {
    fn new() -> Self {
        Self(Mutex::new(HashMap::new()))
    }

    fn get_or_init(&self, key: K, load: impl FnOnce(&K) -> T) -> Arc<T> {
        let slot = Arc::clone(self.0.lock().unwrap().entry(key.clone()).or_default());
        Arc::clone(slot.get_or_init(|| Arc::new(load(&key))))
    }
}

pub(super) struct ConvertedFixture {
    pub(super) outcome: ConvertOutcome,
    package: OnceLock<FixturePackage>,
}

impl ConvertedFixture {
    pub(super) fn package(&self) -> FixturePackage {
        self.package
            .get_or_init(|| fixture_package_from_document(&self.outcome.document))
            .clone()
    }
}

pub(super) fn converted_fixture(
    fixture: &str,
    font_dirs: &[PathBuf],
    options: &ConvertOptions,
    conversion_path: ConversionPath,
) -> Arc<ConvertedFixture> {
    static CACHE: LazyLock<FixtureCache<FixtureKey, ConvertedFixture>> =
        LazyLock::new(FixtureCache::new);
    CACHE.get_or_init(
        FixtureKey::new(fixture, font_dirs, options, conversion_path),
        FixtureKey::convert,
    )
}

/// Return the patched IR (what lowering actually consumed) and the outcome
/// from the same compile. Keep this pair separate from ordinary conversion,
/// which requires the companion Paged compile to succeed.
pub fn fixture_ir_and_outcome(fixture: &str) -> (IrDocument, ConvertOutcome) {
    type PatchedFixture = (IrDocument, ConvertOutcome);
    static CACHE: LazyLock<FixtureCache<PathBuf, PatchedFixture>> =
        LazyLock::new(FixtureCache::new);
    CACHE
        .get_or_init(normalized_fixture_path(fixture), |path| {
            compile_patched(path)
        })
        .as_ref()
        .clone()
}

fn compile_patched(path: &Path) -> (IrDocument, ConvertOutcome) {
    let world = TydocxWorld::new(path).unwrap();
    let docx_doc = typst::compile::<convert::DocxDocument>(&world)
        .output
        .unwrap();
    let paged = typst::compile::<typst_layout::PagedDocument>(&world)
        .output
        .ok();
    let (outcome, ir) = convert::export_ir_with_ir(
        &docx_doc,
        paged.as_ref(),
        &world,
        &ConvertOptions::default(),
    );
    (ir, outcome)
}

#[cfg(test)]
mod tests {
    use std::sync::Barrier;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    use tydocx::CompatMode;

    use super::*;
    use crate::common::{fixture_document, fixture_outcome, fixture_package};

    #[test]
    fn concurrent_callers_load_one_value_for_the_same_key() {
        let cache = FixtureCache::new();
        let calls = AtomicUsize::new(0);
        let start = Barrier::new(8);
        thread::scope(|scope| {
            let workers: Vec<_> = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        start.wait();
                        cache.get_or_init("hello", |_| calls.fetch_add(1, Ordering::Relaxed))
                    })
                })
                .collect();
            for worker in workers {
                assert_eq!(*worker.join().unwrap(), 0);
            }
        });
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_eq!(*cache.get_or_init("hello", |_| panic!("already cached")), 0);
    }

    #[test]
    fn options_fonts_fixture_and_path_have_separate_entries() {
        let cache = FixtureCache::new();
        let defaults = ConvertOptions::default();
        let modern = ConversionPath::Modern;
        let key = FixtureKey::new;
        let fonts = [PathBuf::from("font-dir-a"), PathBuf::from("font-dir-b")];
        let reversed = [fonts[1].clone(), fonts[0].clone()];
        let style = ConvertOptions {
            bibliography_style: Some("test_style.xsl".into()),
            ..defaults.clone()
        };
        let c3 = ConvertOptions::new(CompatMode::C3);
        let keys = [
            key("hello", &[], &defaults, modern),
            key("hello", &[], &c3, modern),
            key("hello", &[], &style, modern),
            key("hello", &fonts, &defaults, modern),
            key("hello", &reversed, &defaults, modern),
            key("hello", &[], &defaults, ConversionPath::Legacy),
            key("doc_title", &[], &defaults, modern),
        ];
        for (index, key) in keys.iter().enumerate() {
            assert_eq!(*cache.get_or_init(key.clone(), |_| index), index);
        }
        for (index, key) in keys.into_iter().enumerate() {
            assert_eq!(*cache.get_or_init(key, |_| panic!("already cached")), index);
        }
    }

    #[test]
    fn document_is_owned_but_package_parts_are_shared_through_the_fixture_alias() {
        let mut document = fixture_document("hello");
        assert!(!document.body.elements.is_empty());
        document.body.elements.clear();

        let first = fixture_package("hello");
        let second = fixture_package("./hello");
        assert!(Arc::ptr_eq(&first.parts, &second.parts));
        let cached_document = fixture_outcome("./hello").document;
        assert!(!cached_document.body.elements.is_empty());
    }

    #[test]
    fn callers_can_edit_their_patched_ir_and_outcome() {
        let (mut ir, mut outcome) = fixture_ir_and_outcome("hello");
        assert!(!ir.body.is_empty());
        assert!(!outcome.document.body.elements.is_empty());
        ir.body.clear();
        outcome.document.body.elements.clear();

        let (ir, outcome) = fixture_ir_and_outcome("./hello");
        assert!(!ir.body.is_empty());
        assert!(!outcome.document.body.elements.is_empty());
    }
}
