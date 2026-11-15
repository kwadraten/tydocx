//! Compile errors keep their source spans (upstream issue #33).

use crate::common::fixture_path;
use typst::World as _;

#[test]
fn compile_error_keeps_span() {
    let world = typort_core::TyportWorld::new(&fixture_path("compile_error_span")).unwrap();
    let errors = typort_core::convert(&world).expect_err("broken fixture should fail");
    assert!(!errors.is_empty(), "expected at least one diagnostic");
    let diagnostic = &errors[0];
    assert!(
        !diagnostic.span.is_detached(),
        "diagnostic should carry a span"
    );
    let id = diagnostic.span.id().expect("span should point into a file");
    let source = world.source(id).unwrap();
    let rendered =
        typort_core::diag::format_diagnostic(diagnostic, Some(("compile_error_span.typ", &source)));
    assert!(
        rendered.starts_with("compile_error_span.typ:3:10: "),
        "expected a located diagnostic, got: {rendered}"
    );
}
