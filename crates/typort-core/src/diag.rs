//! Rendering Typst diagnostics with their source location.
//!
//! Pure functions only: every input is an explicit argument, the same inputs
//! always produce the same output, and nothing here touches the filesystem,
//! the world, or any global state. Resolving a diagnostic's span to a file
//! (which needs the world) is the caller's job, at the boundary; these
//! functions only format what they are given.

use typst::diag::SourceDiagnostic;
use typst::syntax::{DiagSpanKind, Source, Span, SpanKind};

/// Byte offset in `source_text` to a 1-based `(line, column)` pair.
///
/// The column counts characters, not bytes, so multibyte text reports the
/// caret position a human sees. A byte offset past the end clamps to the end
/// of the text; an offset inside a multibyte character counts that character.
/// Lines are counted on `\n` (Typst sources use `\n` line endings).
#[must_use]
pub fn line_col(source_text: &str, byte_offset: usize) -> (usize, usize) {
    let offset = byte_offset.min(source_text.len());
    let mut line = 1;
    let mut column = 1;
    for (index, ch) in source_text.char_indices() {
        if index >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

/// Resolve a bare `Span` to a 1-based `(line, column)` in `source`.
///
/// Same span handling as [`format_diagnostic`], except a bare `Span` carries
/// no sub-range, so a numbered span resolves to its whole node range (a
/// `SourceDiagnostic` can narrow that with its sub-range). Returns `None`
/// when the span is detached, points into another file, or cannot be
/// resolved in this source.
///
/// Caller contract: `source` must be the source the span points into — the
/// one whose id the span carries. Span numbers are unique only within their
/// own file, so both span kinds are checked against that id.
#[must_use]
pub fn span_line_col(source: &Source, span: Span) -> Option<(usize, usize)> {
    let range = match span.get() {
        SpanKind::Detached => return None,
        SpanKind::Number { id, num } => {
            if id != source.id() {
                return None;
            }
            source.range(num, None)?
        }
        SpanKind::Range { id, range } => {
            if id != source.id() {
                return None;
            }
            range
        }
    };
    Some(line_col(source.text(), range.start))
}

/// Render one Typst diagnostic as `file:line:col: message`.
///
/// `file` carries the display name and source of the file the diagnostic's
/// span points into. When the span is detached, the file is missing, or the
/// span cannot be resolved in it, the bare message is returned, so the
/// diagnostic itself is never lost.
///
/// Caller contract: `file` must be the source the span points into — the one
/// whose id the span carries. Both span kinds are checked against that id: a
/// span for another file degrades to the bare message instead of reporting a
/// location in the wrong file. The check is needed on both arms: span numbers
/// are unique only within their own file, so the `Number` branch cannot rely
/// on the number alone, and the `Range` branch is a raw byte range with
/// nothing to check against at all.
#[must_use]
pub fn format_diagnostic(diagnostic: &SourceDiagnostic, file: Option<(&str, &Source)>) -> String {
    let message = diagnostic.message.as_str();
    let Some((name, source)) = file else {
        return message.to_owned();
    };
    let range = match diagnostic.span.get() {
        DiagSpanKind::Detached => return message.to_owned(),
        DiagSpanKind::Number { id, num, sub_range } => {
            if id != source.id() {
                return message.to_owned();
            }
            source.range(num, sub_range)
        }
        DiagSpanKind::Range { id, range } => {
            if id != source.id() {
                return message.to_owned();
            }
            Some(range)
        }
    };
    let Some(range) = range else {
        return message.to_owned();
    };
    let (line, column) = line_col(source.text(), range.start);
    format!("{name}:{line}:{column}: {message}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use typst::syntax::{DiagSpan, FileId, LinkedNode, RootedPath, Side, VirtualPath, VirtualRoot};

    fn test_source(text: &str) -> Source {
        let vpath = VirtualPath::new("/diag-test.typ").unwrap();
        let id = RootedPath::new(VirtualRoot::Project, vpath).intern();
        Source::new(id, text.to_string())
    }

    fn test_file_id() -> FileId {
        let vpath = VirtualPath::new("/diag-test.typ").unwrap();
        RootedPath::new(VirtualRoot::Project, vpath).intern()
    }

    #[test]
    fn line_col_empty_and_start() {
        assert_eq!(line_col("", 0), (1, 1));
        assert_eq!(line_col("", 99), (1, 1));
        assert_eq!(line_col("hello", 0), (1, 1));
    }

    #[test]
    fn line_col_single_line() {
        assert_eq!(line_col("hello", 1), (1, 2));
        assert_eq!(line_col("hello", 5), (1, 6));
        assert_eq!(line_col("hello", 99), (1, 6));
    }

    #[test]
    fn line_col_multiple_lines() {
        assert_eq!(line_col("a\nb\nc", 0), (1, 1));
        assert_eq!(line_col("a\nb\nc", 1), (1, 2));
        assert_eq!(line_col("a\nb\nc", 2), (2, 1));
        assert_eq!(line_col("a\nb\nc", 3), (2, 2));
        assert_eq!(line_col("a\nb\nc", 4), (3, 1));
        assert_eq!(line_col("a\n", 2), (2, 1));
    }

    #[test]
    fn line_col_counts_characters_not_bytes() {
        // "é" is two bytes; the column counts it as one character.
        assert_eq!(line_col("aé\nb", 3), (1, 3));
        assert_eq!(line_col("aé\nb", 4), (2, 1));
        // Offset 2 lands inside "é"; the character is counted.
        assert_eq!(line_col("aé", 2), (1, 3));
    }

    #[test]
    fn format_diagnostic_range_span() {
        let source = test_source("= Title\n\nBody text\n");
        let diagnostic = SourceDiagnostic::error(
            DiagSpan::from_range(test_file_id(), 9..13),
            "something broke",
        );
        assert_eq!(
            format_diagnostic(&diagnostic, Some(("diag-test.typ", &source))),
            "diag-test.typ:3:1: something broke"
        );
    }

    #[test]
    fn format_diagnostic_range_span_mid_line() {
        let source = test_source("= Title\n\nBody text\n");
        let diagnostic = SourceDiagnostic::error(
            DiagSpan::from_range(test_file_id(), 10..14),
            "something broke",
        );
        assert_eq!(
            format_diagnostic(&diagnostic, Some(("diag-test.typ", &source))),
            "diag-test.typ:3:2: something broke"
        );
    }

    #[test]
    fn format_diagnostic_numbered_span() {
        let source = test_source("= Title\n\nBody text\n");
        let span = LinkedNode::new(source.root())
            .leaf_at(9, Side::After)
            .unwrap()
            .span();
        let diagnostic = SourceDiagnostic::error(span, "unknown variable");
        assert_eq!(
            format_diagnostic(&diagnostic, Some(("diag-test.typ", &source))),
            "diag-test.typ:3:1: unknown variable"
        );
    }

    #[test]
    fn format_diagnostic_detached_span_has_no_location() {
        let source = test_source("= Title\n");
        let diagnostic = SourceDiagnostic::error(DiagSpan::detached(), "boom");
        assert_eq!(
            format_diagnostic(&diagnostic, Some(("diag-test.typ", &source))),
            "boom"
        );
    }

    #[test]
    fn format_diagnostic_missing_file_has_no_location() {
        let diagnostic =
            SourceDiagnostic::error(DiagSpan::from_range(test_file_id(), 0..1), "boom");
        assert_eq!(format_diagnostic(&diagnostic, None), "boom");
    }

    #[test]
    fn format_diagnostic_range_span_for_other_file_has_no_location() {
        let source = test_source("= Title\n\nBody text\n");
        let other_id = {
            let vpath = VirtualPath::new("/other.typ").unwrap();
            RootedPath::new(VirtualRoot::Project, vpath).intern()
        };
        let diagnostic =
            SourceDiagnostic::error(DiagSpan::from_range(other_id, 9..13), "something broke");
        assert_eq!(
            format_diagnostic(&diagnostic, Some(("diag-test.typ", &source))),
            "something broke"
        );
    }

    #[test]
    fn span_line_col_numbered_span() {
        let source = test_source("= Title\n\nBody text\n");
        let span = LinkedNode::new(source.root())
            .leaf_at(9, Side::After)
            .unwrap()
            .span();
        assert_eq!(span_line_col(&source, span), Some((3, 1)));
    }

    #[test]
    fn span_line_col_detached_span_has_no_location() {
        let source = test_source("= Title\n");
        assert_eq!(
            span_line_col(&source, typst::syntax::Span::detached()),
            None
        );
    }

    #[test]
    fn span_line_col_span_for_other_file_has_no_location() {
        let source = test_source("= Title\n\nBody text\n");
        let other_source = {
            let vpath = VirtualPath::new("/other.typ").unwrap();
            let id = RootedPath::new(VirtualRoot::Project, vpath).intern();
            Source::new(id, "= Title\n\nBody text\n".to_string())
        };
        let span = LinkedNode::new(other_source.root())
            .leaf_at(9, Side::After)
            .unwrap()
            .span();
        assert_eq!(span_line_col(&source, span), None);
    }

    #[test]
    fn format_diagnostic_numbered_span_for_other_file_has_no_location() {
        // Same text, different file: without the id check the number would
        // resolve in this source's tree and report a wrong location.
        let source = test_source("= Title\n\nBody text\n");
        let other_source = {
            let vpath = VirtualPath::new("/other.typ").unwrap();
            let id = RootedPath::new(VirtualRoot::Project, vpath).intern();
            Source::new(id, "= Title\n\nBody text\n".to_string())
        };
        let span = LinkedNode::new(other_source.root())
            .leaf_at(9, Side::After)
            .unwrap()
            .span();
        let diagnostic = SourceDiagnostic::error(span, "unknown variable");
        assert_eq!(
            format_diagnostic(&diagnostic, Some(("diag-test.typ", &source))),
            "unknown variable"
        );
    }
}
