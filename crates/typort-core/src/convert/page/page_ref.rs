//! Resolve `counter(page).at(<label>)` page references from source text.
//!
//! A `context` Tag in the HTML tree carries only the rendered page number as
//! text — the label it refers to is not recoverable from the compiled output.
//! The Tag's span points at the source expression, so the label is recovered
//! from the source AST: walk down the callee chain of the expression until a
//! `counter(page).at(<label>)` call is found.

use typst_syntax::LinkedNode;
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

/// What a `context` expression does with a counter, for the PAGE-field
/// decision in `handle_inline_context_tag`. Callers try `counter_page_label`
/// first (PAGEREF); this classifies the remaining shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CounterUse {
    /// Current-page reference: `counter(page).display(...)` or
    /// `counter(page).at(here())`, possibly with further chaining
    /// (`.display().first()`, ...). Word recomputes it as a PAGE field.
    CurrentPage,
    /// A page-counter use Word cannot recompute: arithmetic around it, or
    /// `.at(...)` with a non-label, non-`here()` argument. Stays literal text
    /// and is reported as a degradation.
    PageCounterUnsupported,
    /// A footnote-counter reference (`counter(footnote)...`); NOTEREF is a
    /// C3 field, so it stays literal text here and is reported.
    FootnoteCounter,
    /// No page/footnote counter use (ordinary context content).
    Other,
}

/// Classify the counter use of the source expression a `context` Tag's span
/// points at.
#[must_use]
pub fn classify_counter_use(source: &Source, span: Span) -> CounterUse {
    let Some(node) = source.find(span) else {
        return CounterUse::Other;
    };
    if is_current_page_ref(node.clone()) {
        return CounterUse::CurrentPage;
    }
    if subtree_has_counter(&node, "page") {
        return CounterUse::PageCounterUnsupported;
    }
    if subtree_has_counter(&node, "footnote") {
        return CounterUse::FootnoteCounter;
    }
    CounterUse::Other
}

/// Whether the expression (walking down its callee chain) is a current-page
/// reference: `.display(...)` or `.at(here())` on `counter(page)`.
fn is_current_page_ref(mut node: LinkedNode<'_>) -> bool {
    loop {
        if let Some(call) = node.cast::<ast::FuncCall>() {
            if is_page_display_call(call) || is_page_at_here_call(call) {
                return true;
            }
            // Descend into the callee (`.first()` on a display, ...).
            let Some(next) = node.children().next() else {
                return false;
            };
            node = next;
        } else if node.cast::<ast::FieldAccess>().is_some() {
            // Descend into the target (`.display(...)` on `counter(page)`).
            let Some(next) = node.children().next() else {
                return false;
            };
            node = next;
        } else {
            return false;
        }
    }
}

/// Whether this call is `.display(...)` on a `counter(page)` call. The
/// display pattern argument (if any) does not matter: the cached text carries
/// whatever Typst rendered.
fn is_page_display_call(call: ast::FuncCall<'_>) -> bool {
    let ast::Expr::FieldAccess(access) = call.callee() else {
        return false;
    };
    if access.field().get().as_str() != "display" {
        return false;
    }
    let ast::Expr::FuncCall(inner) = access.target() else {
        return false;
    };
    is_counter_call(inner, "page")
}

/// Whether this call is `.at(here())` on a `counter(page)` call.
fn is_page_at_here_call(call: ast::FuncCall<'_>) -> bool {
    let ast::Expr::FieldAccess(access) = call.callee() else {
        return false;
    };
    if access.field().get().as_str() != "at" {
        return false;
    }
    let ast::Expr::FuncCall(inner) = access.target() else {
        return false;
    };
    if !is_counter_call(inner, "page") {
        return false;
    }
    // The `.at(...)` argument must be a bare `here()` call.
    let mut at_args = call.args().items();
    let Some(ast::Arg::Pos(ast::Expr::FuncCall(here))) = at_args.next() else {
        return false;
    };
    if !matches!(here.callee(), ast::Expr::Ident(ident) if ident.get().as_str() == "here") {
        return false;
    }
    here.args().items().next().is_none()
}

/// Whether this call is `counter(<name>)` with one positional ident argument.
fn is_counter_call(call: ast::FuncCall<'_>, name: &str) -> bool {
    if !matches!(call.callee(), ast::Expr::Ident(ident) if ident.get().as_str() == "counter") {
        return false;
    }
    let mut args = call.args().items();
    matches!(args.next(), Some(ast::Arg::Pos(ast::Expr::Ident(ident)))
        if ident.get().as_str() == name)
}

/// Whether any `counter(<name>)` call occurs anywhere under `node`.
fn subtree_has_counter(node: &LinkedNode<'_>, name: &str) -> bool {
    if let Some(call) = node.cast::<ast::FuncCall>()
        && is_counter_call(call, name)
    {
        return true;
    }
    node.children()
        .any(|child| subtree_has_counter(&child, name))
}
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

    /// Classify the counter use for a bare source expression, as the context
    /// Tag's span covers it; the span passed is the outermost node's,
    /// matching the real Tag span.
    fn classify_of(text: &str) -> CounterUse {
        let source = Source::detached(text);
        let call = find_node(source.root(), SyntaxKind::FuncCall).unwrap();
        classify_counter_use(&source, call.span())
    }

    #[test]
    fn display_and_at_here_classify_as_current_page() {
        assert_eq!(
            classify_of("#counter(page).display()"),
            CounterUse::CurrentPage
        );
        assert_eq!(
            classify_of("#counter(page).display(\"1\")"),
            CounterUse::CurrentPage
        );
        assert_eq!(
            classify_of("#counter(page).at(here())"),
            CounterUse::CurrentPage
        );
        assert_eq!(
            classify_of("#counter(page).at(here()).display()"),
            CounterUse::CurrentPage
        );
        assert_eq!(
            classify_of("#counter(page).display().first()"),
            CounterUse::CurrentPage
        );
    }

    #[test]
    fn arithmetic_footnote_and_other_counters_classify() {
        // Arithmetic around the reference: the Tag span covers the whole
        // binary expression.
        let source = Source::detached("#(counter(page).display() + 1)");
        let binary = find_node(source.root(), SyntaxKind::Binary).unwrap();
        assert_eq!(
            classify_counter_use(&source, binary.span()),
            CounterUse::PageCounterUnsupported
        );
        assert_eq!(
            classify_of("#counter(footnote).display()"),
            CounterUse::FootnoteCounter
        );
        assert_eq!(
            classify_of("#counter(footnote).at(<fn>)"),
            CounterUse::FootnoteCounter
        );
        assert_eq!(
            classify_of("#counter(heading).display()"),
            CounterUse::Other
        );
        // A label reference is PAGEREF territory (the caller tries the label
        // first); classify alone reports it as an unsupported page-counter
        // use, which is why the caller order matters.
        assert_eq!(
            classify_of("#counter(page).at(<intro>)"),
            CounterUse::PageCounterUnsupported
        );
    }
}
