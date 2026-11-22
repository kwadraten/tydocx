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
    /// Total-page reference: `counter(page).final()`, possibly chained into
    /// `.first()`/`.at(..)`/`.display(...)` — the document's last page
    /// number. Word recomputes it as a NUMPAGES field (C3).
    PageTotal,
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
    if is_page_final_ref(node.clone()) {
        return CounterUse::PageTotal;
    }
    if subtree_has_counter(&node, "page") {
        return CounterUse::PageCounterUnsupported;
    }
    if subtree_has_counter(&node, "footnote") {
        return CounterUse::FootnoteCounter;
    }
    CounterUse::Other
}

/// Resolve the label of a `counter(footnote).at(<label>)` reference from the
/// source expression a `context` Tag's span points at — the footnote the
/// reference counts to, for the C3 NOTEREF field. Same callee-chain walk as
/// [`counter_page_label`].
#[must_use]
pub fn counter_footnote_label(source: &Source, span: Span) -> Option<String> {
    let mut node = source.find(span)?;
    loop {
        if let Some(call) = node.cast::<ast::FuncCall>() {
            if let Some(label) = counter_at_label(call, "footnote") {
                return Some(label);
            }
            node = node.children().next()?;
        } else if node.cast::<ast::FieldAccess>().is_some() {
            node = node.children().next()?;
        } else {
            return None;
        }
    }
}

/// Which `datetime` value a `context` expression evaluates, for the C3
/// `DATE`/`TIME` fields. `datetime.today()` is a date; `datetime.now()`
/// (should Typst expose it) is the time-of-day reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateTimeUse {
    /// `datetime.today(...)` → ` DATE `.
    Date,
    /// `datetime.now(...)` → ` TIME `.
    Time,
}

/// Classify the `datetime` reading of the source expression a `context`
/// Tag's span points at. Only a bare `datetime.today()`/`datetime.now()`
/// call chain (possibly wrapped in `.display(...)`, `.year()`, ...) counts —
/// the value must be "today/now at render time", which is what the Word
/// field recomputes.
#[must_use]
pub fn datetime_use(source: &Source, span: Span) -> Option<DateTimeUse> {
    let mut node = source.find(span)?;
    loop {
        if let Some(call) = node.cast::<ast::FuncCall>() {
            if let Some(use_) = datetime_call(call) {
                return Some(use_);
            }
            node = node.children().next()?;
        } else {
            let access = node.cast::<ast::FieldAccess>()?;
            if is_datetime_access(access) {
                return Some(DateTimeUse::Date);
            }
            node = node.children().next()?;
        }
    }
}

/// Whether this call is `datetime.today(...)`/`datetime.now(...)`.
fn datetime_call(call: ast::FuncCall<'_>) -> Option<DateTimeUse> {
    let ast::Expr::FieldAccess(access) = call.callee() else {
        return None;
    };
    let field = access.field().get();
    let ast::Expr::Ident(target) = access.target() else {
        return None;
    };
    if target.get().as_str() != "datetime" {
        return None;
    }
    match field.as_str() {
        "today" => Some(DateTimeUse::Date),
        "now" => Some(DateTimeUse::Time),
        _ => None,
    }
}

/// Whether this field access is `datetime.today` — the no-parens form.
fn is_datetime_access(access: ast::FieldAccess<'_>) -> bool {
    access.field().get().as_str() == "today"
        && matches!(access.target(), ast::Expr::Ident(ident) if ident.get().as_str() == "datetime")
}

/// Which document-metadata field a `context` expression reads, for the C3
/// `DOCPROPERTY` field. Walks the callee chain to a `document.<field>` field
/// access; only the fields tydocx writes into `docProps/core.xml` map.
#[must_use]
pub fn docproperty_use(source: &Source, span: Span) -> Option<&'static str> {
    let mut node = source.find(span)?;
    loop {
        if node.cast::<ast::FuncCall>().is_some() {
            // `.display()`/`.join(...)`/etc. on the field read — descend into
            // the callee to reach the field access.
            node = node.children().next()?;
        } else {
            let access = node.cast::<ast::FieldAccess>()?;
            if let name @ Some(_) = document_field(access) {
                return name;
            }
            node = node.children().next()?;
        }
    }
}

/// The Word `DOCPROPERTY` name of a `document.<field>` field access — the
/// names tydocx writes into `docProps/core.xml`.
fn document_field(access: ast::FieldAccess<'_>) -> Option<&'static str> {
    if !matches!(access.target(), ast::Expr::Ident(ident) if ident.get().as_str() == "document") {
        return None;
    }
    match access.field().get().as_str() {
        "title" => Some("Title"),
        "author" => Some("Author"),
        "keywords" => Some("Keywords"),
        "description" => Some("Comments"),
        "date" => Some("CreateDate"),
        _ => None,
    }
}

/// Whether the expression (walking down its callee chain) reaches
/// `counter(page).final()` — the document's total page count.
fn is_page_final_ref(mut node: LinkedNode<'_>) -> bool {
    loop {
        if let Some(call) = node.cast::<ast::FuncCall>() {
            if is_page_final_call(call) {
                return true;
            }
            // Descend into the callee (`.first()` on the final value, ...).
            let Some(next) = node.children().next() else {
                return false;
            };
            node = next;
        } else if let Some(access) = node.cast::<ast::FieldAccess>() {
            if access.field().get().as_str() == "final"
                && matches!(access.target(), ast::Expr::FuncCall(inner) if is_counter_call(inner, "page"))
            {
                return true;
            }
            // Descend into the target (`counter(page).final` itself).
            let Some(next) = node.children().next() else {
                return false;
            };
            node = next;
        } else {
            return false;
        }
    }
}

/// Whether this call is `.final(...)` on a `counter(page)` call.
fn is_page_final_call(call: ast::FuncCall<'_>) -> bool {
    let ast::Expr::FieldAccess(access) = call.callee() else {
        return false;
    };
    if access.field().get().as_str() != "final" {
        return false;
    }
    let ast::Expr::FuncCall(inner) = access.target() else {
        return false;
    };
    is_counter_call(inner, "page")
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
    counter_at_label(call, "page")
}

/// The label argument of a `counter(<name>).at(<label>)` call — the
/// `<label>` positional arg or `None` for any other shape.
fn counter_at_label(call: ast::FuncCall<'_>, name: &str) -> Option<String> {
    // The callee must be a `.at` field access on a `counter(<name>)` call.
    let ast::Expr::FieldAccess(access) = call.callee() else {
        return None;
    };
    if access.field().get().as_str() != "at" {
        return None;
    }
    let ast::Expr::FuncCall(inner) = access.target() else {
        return None;
    };
    if !is_counter_call(inner, name) {
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
