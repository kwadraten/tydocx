use std::ops::ControlFlow;
use tydocx_math::equation_to_omml;
use typst::diag::{FileError, FileResult, SourceResult};
use typst::engine::Engine;
use typst::foundations::{Bytes, Content, Datetime, Duration, Output, StyleChain, Target};
use typst::introspection::{EmptyIntrospector, Introspector};
use typst::math::EquationElem;
use typst::syntax::{FileId, Source};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};

struct FixtureWorld {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    source: Source,
}

impl World for FixtureWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.book
    }

    fn main(&self) -> FileId {
        self.source.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main() {
            Ok(self.source.clone())
        } else {
            Err(FileError::AccessDenied)
        }
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        Ok(Bytes::from_string(self.source(id)?.text().to_owned()))
    }

    fn font(&self, _: usize) -> Option<Font> {
        None
    }

    fn today(&self, _: Option<Duration>) -> Option<Datetime> {
        None
    }
}

struct EquationXml(Vec<String>);

impl Output for EquationXml {
    fn target() -> Target {
        Target::Paged
    }

    fn create(_: &mut Engine, content: &Content, _: StyleChain) -> SourceResult<Self> {
        let mut equations = Vec::new();
        // The backend consumes evaluated Content, so the fixture needs no layout or fonts.
        let _ = content.traverse(&mut |element| -> ControlFlow<()> {
            if element.to_packed::<EquationElem>().is_some() {
                equations.push(equation_to_omml(&element));
            }
            ControlFlow::Continue(())
        });
        Ok(Self(equations))
    }

    fn introspector(&self) -> &dyn Introspector {
        &EmptyIntrospector
    }
}

fn fixture_equations(source: &str) -> Vec<String> {
    let world = FixtureWorld {
        library: LazyHash::new(Library::default()),
        book: LazyHash::new(FontBook::new()),
        source: Source::detached(source),
    };
    let EquationXml(equations) = typst::compile::<EquationXml>(&world).output.unwrap();
    equations
}

#[test]
fn cases_keep_only_the_delimiter_on_the_selected_side() {
    let equations = fixture_equations(include_str!("fixtures/cases_delimiters.typ"));
    // A reversed case array keeps the authored right delimiter, not a flipped left one.
    // In particular, an explicit `none` on the selected side must remain invisible.
    let expected = [("{", ""), ("", "}"), ("", ")"), ("", "")].map(|(open, close)| {
        format!(
            concat!(
                "<m:oMath><m:d><m:dPr>",
                "<m:begChr m:val=\"{0}\"/><m:endChr m:val=\"{1}\"/>",
                "</m:dPr><m:e><m:eqArr>",
                "<m:e><m:r><m:t>1</m:t></m:r></m:e>",
                "<m:e><m:r><m:t>2</m:t></m:r></m:e>",
                "</m:eqArr></m:e></m:d></m:oMath>",
            ),
            open, close
        )
    });
    assert_eq!(equations, expected);
}

#[test]
fn empty_cases_has_one_empty_equation_array_row() {
    let equations = fixture_equations(include_str!("fixtures/empty_cases.typ"));
    assert_eq!(
        equations,
        [concat!(
            "<m:oMath><m:d><m:dPr>",
            "<m:begChr m:val=\"{\"/><m:endChr m:val=\"\"/>",
            "</m:dPr><m:e><m:eqArr><m:e/></m:eqArr></m:e></m:d></m:oMath>",
        )],
    );
}
