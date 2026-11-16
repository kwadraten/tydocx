//! Resolve `counter(page).at(<label>)` page references from source text.
//!
//! A `context` Tag in the HTML tree carries only the rendered page number as
//! text — the label it refers to is not recoverable from the compiled output.
//! The Tag's span points at the source expression, so the label is recovered
//! from the source AST: walk down the callee chain of the expression until a
//! `counter(page).at(<label>)` call is found.

use typst_syntax::Source;
use typst_syntax::Span;
use typst_syntax::ast::{self};

/// Resolve the label of a `counter(page).at(<label>)` page reference from
/// the source expression a `context` Tag's span points at.
///
/// Returns `None` when the span is detached, the expression is not a page
/// counter reference (a different counter, a `here()` selector, arithmetic
/// around the reference, ...), or the label can't be read — in all those
/// cases the caller keeps today's plain-text rendering.
#[must_use]
pub fn counter_page_label(source: &Source, span: Span) -> Option<String> {
    let mut node = source.find(span)?;
    loop {
        if let Some(call) = node.cast::<ast::FuncCall>() {
            if let Some(label) = counter_page_at_label(call) {
                return Some(label);
            }
            // Descend into the callee (`.first()` on a page reference, ...).
            node = node.children().next()?;
        } else if node.cast::<ast::FieldAccess>().is_some() {
            // Descend into the target (`.at(...)` on `counter(page)`).
            node = node.children().next()?;
        } else {
            // Anything else — arithmetic, a bare ident, an enclosing block —
            // is not a page reference.
            return None;
        }
    }
}

/// Read the label out of a `counter(page).at(<label>)` call, or `None` when
/// this call is anything else.
fn counter_page_at_label(call: ast::FuncCall<'_>) -> Option<String> {
    // The callee must be a `.at` field access on a `counter(page)` call.
    let ast::Expr::FieldAccess(access) = call.callee() else {
        return None;
    };
    if access.field().get().as_str() != "at" {
        return None;
    }
    let ast::Expr::FuncCall(inner) = access.target() else {
        return None;
    };
    if !matches!(inner.callee(), ast::Expr::Ident(ident) if ident.get().as_str() == "counter") {
        return None;
    }
    // `counter(page)`: one positional argument, the `page` ident.
    let mut counter_args = inner.args().items();
    if !matches!(counter_args.next(), Some(ast::Arg::Pos(ast::Expr::Ident(ident)))
        if ident.get().as_str() == "page")
    {
        return None;
    }
    // The `.at(...)` argument must be a label.
    let mut at_args = call.args().items();
    match at_args.next() {
        Some(ast::Arg::Pos(ast::Expr::Label(label))) => Some(label.get().to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use typst_syntax::{SyntaxKind, SyntaxNode};

    /// Find the first node of `kind` in pre-order (the outermost one for
    /// nested calls).
    fn find_node(node: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxNode> {
        if node.kind() == kind {
            return Some(node.clone());
        }
        node.children().find_map(|child| find_node(child, kind))
    }

    /// Resolve the label for a bare source expression. The test text is a
    /// `#`-prefixed code expression, as the context Tag's span covers it;
    /// the span passed is the outermost `FuncCall`'s, matching the real
    /// Tag span.
    fn label_of(text: &str) -> Option<String> {
        let source = Source::detached(text);
        let call = find_node(source.root(), SyntaxKind::FuncCall)?;
        counter_page_label(&source, call.span())
    }

    #[test]
    fn page_counter_at_label_resolves() {
        assert_eq!(
            label_of("#counter(page).at(<intro>).first()"),
            Some("intro".to_string())
        );
        assert_eq!(
            label_of("#counter(page).at(<intro>)"),
            Some("intro".to_string())
        );
        assert_eq!(
            label_of("#counter(page).at(<sec:intro>).first()"),
            Some("sec:intro".to_string())
        );
    }

    #[test]
    fn other_counters_do_not_resolve() {
        assert_eq!(label_of("#counter(heading).at(<intro>).first()"), None);
        // Arithmetic around the reference: the Tag span covers the whole
        // binary expression, which is not a page reference.
        let source = Source::detached("#(counter(page).at(<intro>).first() + 1)");
        let binary = find_node(source.root(), SyntaxKind::Binary).unwrap();
        assert_eq!(counter_page_label(&source, binary.span()), None);
    }

    #[test]
    fn here_selector_does_not_resolve() {
        assert_eq!(label_of("#counter(page).at(here()).first()"), None);
    }
}
