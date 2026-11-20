use super::smallcaps::collect_markup_text;
use super::{BlockElement, Document, InlineElement, Run, TyportWorld};

/// `text(baseline:)` is a `#[ghost]` property — like `smallcaps`, it is folded
/// into glyph shifts during realization and never reaches the pairs or the
/// `StyleChain`. Recover it from the source AST: find every
/// `#text(baseline: N)[body]` call, extract the body text and the half-point
/// shift, and mark the matching runs.
pub(super) fn apply_baseline_shifts_from_source(world: &TyportWorld, doc: &mut Document) {
    let source_text = world.main_source().text();
    let shifts = extract_baseline_texts_from_ast(source_text);
    if shifts.is_empty() {
        return;
    }
    for element in &mut doc.body.elements {
        if let BlockElement::Paragraph(p) = element {
            apply_shifts_to_inlines(&mut p.inlines, &shifts);
        }
    }
}

/// Split text runs around each recovered `#text(baseline:)` body so the shift
/// applies to exactly the shifted text — runs are coalesced across formatting
/// boundaries, so the shifted fragment usually sits inside a longer run.
fn apply_shifts_to_inlines(inlines: &mut Vec<InlineElement>, shifts: &[(i32, String)]) {
    let mut out = Vec::with_capacity(inlines.len());
    for inline in inlines.drain(..) {
        match inline {
            InlineElement::Text(run) => split_run(run, shifts, &mut out),
            other => out.push(other),
        }
    }
    *inlines = out;
}

fn split_run(run: Run, shifts: &[(i32, String)], out: &mut Vec<InlineElement>) {
    let found = shifts
        .iter()
        .filter_map(|(half_pt, text)| {
            run.text
                .find(text.as_str())
                .map(|pos| (pos, text, *half_pt))
        })
        .min_by_key(|(pos, _, _)| *pos);
    let Some((pos, text, half_pt)) = found else {
        out.push(InlineElement::Text(run));
        return;
    };
    if pos > 0 {
        let mut before = run.clone();
        before.text = run.text[..pos].to_string();
        out.push(InlineElement::Text(before));
    }
    let mut shifted = run.clone();
    shifted.text.clone_from(text);
    shifted.position_half_pt = Some(half_pt);
    out.push(InlineElement::Text(shifted));
    let rest = run.text[pos + text.len()..].to_string();
    if !rest.is_empty() {
        let mut after = run;
        after.text = rest;
        split_run(after, shifts, out);
    }
}

/// `(half-point shift, body text)` for each `#text(baseline: N)[body]` call in
/// source order. Only `pt` and `em` lengths map to `w:position`.
fn extract_baseline_texts_from_ast(source: &str) -> Vec<(i32, String)> {
    use typst_syntax::SyntaxKind;
    use typst_syntax::ast;
    fn collect(node: &typst_syntax::SyntaxNode, out: &mut Vec<(i32, String)>) {
        if node.kind() == SyntaxKind::FuncCall
            && let Some(call) = node.cast::<ast::FuncCall<'_>>()
            && matches!(call.callee(), ast::Expr::Ident(ident) if ident.as_str() == "text")
        {
            let mut half_pt = None;
            let mut texts = Vec::new();
            for arg in call.args().items() {
                match arg {
                    ast::Arg::Named(named) if named.name().get().as_str() == "baseline" => {
                        if let Some(shift) = numeric_half_pt(&named.expr()) {
                            half_pt = Some(shift);
                        }
                    }
                    ast::Arg::Pos(ast::Expr::ContentBlock(block)) => {
                        texts.push(collect_markup_text(block.body()).trim().to_string());
                    }
                    _ => {}
                }
            }
            if let Some(half_pt) = half_pt {
                out.extend(
                    texts
                        .into_iter()
                        .filter(|text| !text.is_empty())
                        .map(|text| (half_pt, text)),
                );
            }
        }
        for child in node.children() {
            collect(child, out);
        }
    }

    let root = typst_syntax::parse(source);
    let mut out = Vec::new();
    collect(&root, &mut out);
    out
}

/// A `baseline: N` numeric argument → signed half-points. `em` resolves
/// against the default 11pt body size — source-declared em baselines are rare
/// enough that the imprecision is acceptable for the C1 form.
fn numeric_half_pt(expr: &typst_syntax::ast::Expr<'_>) -> Option<i32> {
    use typst_syntax::ast;
    let ast::Expr::Numeric(numeric) = expr else {
        return None;
    };
    let (value, unit) = numeric.get();
    let pt = match unit {
        typst_syntax::ast::Unit::Pt => value,
        typst_syntax::ast::Unit::Em => value * 11.0,
        _ => return None,
    };
    #[allow(
        clippy::cast_possible_truncation,
        reason = "baseline shifts stay far below i32 range"
    )]
    Some((pt * 2.0).round() as i32)
}
