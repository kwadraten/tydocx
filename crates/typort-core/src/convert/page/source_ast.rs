use super::units::{numeric_to_half_pt, numeric_to_twips};

/// All style overrides extracted from `#set` rules in the source AST.
/// Each field is `None` if the source doesn't set it (use heuristic fallback).
#[derive(Default)]
pub struct SourceStyleOverrides {
    // #set page(margin: ...)
    pub margin_top: Option<u32>,
    pub margin_bottom: Option<u32>,
    pub margin_left: Option<u32>,
    pub margin_right: Option<u32>,
    // #set page(margin: ...) in em — resolved later against the body size,
    // the same deferred-em pattern as the `par` em fields below. At most one
    // of the twips / em field per side is set.
    pub margin_top_em: Option<f64>,
    pub margin_bottom_em: Option<f64>,
    pub margin_left_em: Option<f64>,
    pub margin_right_em: Option<f64>,
    // #set page(columns: N)
    pub columns: Option<u32>,
    // #set page(numbering: "1"/"i"/...) — a plain pattern, or a declared but
    // unmappable value (a function, a computed value). `numbering: none`
    // leaves this `None` (numbering disabled, nothing to consume).
    pub page_numbering: Option<PageNumberingDecl>,
    // #set text(font: ..., size: ...)
    pub text_font: Option<Vec<String>>,
    pub text_size_half_pt: Option<u32>,
    // #set text(lang: "zh", region: "cn") — ISO 639 lang + optional ISO 3166 region.
    pub text_lang: Option<String>,
    pub text_region: Option<String>,
    // #set par(first-line-indent: ..., leading: ..., justify: ..., spacing: ...)
    // Values in twips for absolute units, or as em*1000 (milliem) for em units.
    pub first_line_indent_twips: Option<u32>,
    pub first_line_indent_em: Option<f64>,
    // #set par(first-line-indent: (amount: ..., all: true)) — indent every
    // paragraph, including the first after a heading.
    pub first_line_indent_all: Option<bool>,
    pub par_leading_twips: Option<u32>,
    pub par_leading_em: Option<f64>,
    pub par_spacing_twips: Option<u32>,
    pub par_spacing_em: Option<f64>,
    pub justify: Option<bool>,
}

/// How `#set page(numbering:)` was declared.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PageNumberingDecl {
    /// A plain numbering pattern (`"1"`, `"i"`, `"a"`, …).
    Pattern(String),
    /// Declared, but not as a plain pattern — e.g. a function
    /// (`n => "(" + str(n) + ")"`). Falls back to glyph detection.
    Unmappable,
}

impl SourceStyleOverrides {
    /// Fill any `None` fields from `other` (used to merge imported file overrides).
    pub fn merge_from(&mut self, other: &SourceStyleOverrides) {
        macro_rules! fill {
            ($field:ident) => {
                if self.$field.is_none() {
                    self.$field = other.$field.clone();
                }
            };
        }
        fill!(margin_top);
        fill!(margin_bottom);
        fill!(margin_left);
        fill!(margin_right);
        fill!(margin_top_em);
        fill!(margin_bottom_em);
        fill!(margin_left_em);
        fill!(margin_right_em);
        fill!(columns);
        fill!(page_numbering);
        fill!(text_font);
        fill!(text_size_half_pt);
        fill!(text_lang);
        fill!(text_region);
        fill!(first_line_indent_twips);
        fill!(first_line_indent_em);
        fill!(first_line_indent_all);
        fill!(par_leading_twips);
        fill!(par_leading_em);
        fill!(par_spacing_twips);
        fill!(par_spacing_em);
        fill!(justify);
    }
}

/// Extract style overrides from Typst source AST in a single walk.
///
/// Reads `#set page(...)`, `#set text(...)`, `#set par(...)` rules, but only
/// those in *document-global* scope: top-level of the file, or inside the
/// closure named by a `#show:` template (whose names are passed in
/// `template_names`). A `set text(size:)` buried in a `#block[…]` or a
/// non-template helper closure is element-/locally-scoped and must not clobber
/// the real global body size.
#[must_use]
pub fn extract_source_style_overrides(
    source: &str,
    template_names: &[String],
) -> SourceStyleOverrides {
    let root = typst_syntax::parse(source);
    let mut ovr = SourceStyleOverrides::default();
    collect_global_set_rules(&root, template_names, true, &mut ovr);
    ovr
}

/// Whether the source resets the footnote counter inside a page header
/// (`#set page(header: …)` containing `counter(footnote).update(N)` with an
/// integer literal `N`, or the `#page(header: …)` call form).
///
/// Per typst-library's `FootnoteElem` docs, that is the opt-in for per-page
/// footnote numbering; the default is continuous numbering across the whole
/// document. A function-valued update (`update(n => n + 1)`) is not a reset,
/// and neither is an update anywhere but a page header — a mid-document
/// `counter(footnote).update(0)` restarts numbering once, it does not make it
/// per-page.
///
/// Only document-global rules count (top level, or inside a `#show:`
/// template closure) — the same scoping as [`extract_source_style_overrides`].
#[must_use]
pub fn footnote_counter_reset_in_page_header(source: &str, template_names: &[String]) -> bool {
    let root = typst_syntax::parse(source);
    header_resets_footnote_counter(&root, template_names, true)
}

/// Mirror of [`collect_global_set_rules`]' scoping, answering whether any
/// document-global page-header rule resets the footnote counter.
fn header_resets_footnote_counter(
    node: &typst_syntax::SyntaxNode,
    template_names: &[String],
    in_global_scope: bool,
) -> bool {
    use typst_syntax::SyntaxKind;
    use typst_syntax::ast;

    match node.kind() {
        // A show rule's recipe applies to specific elements, not globally.
        SyntaxKind::ShowRule => return false,
        // A content block (`[...]`) introduces a local scope.
        SyntaxKind::ContentBlock => {
            return node
                .children()
                .any(|child| header_resets_footnote_counter(child, template_names, false));
        }
        // A `#let NAME(...) = …` closure. The show-template closure holds the
        // real document globals; any other closure is a local helper.
        SyntaxKind::LetBinding => {
            if let Some(binding) = node.cast::<ast::LetBinding<'_>>()
                && let ast::LetBindingKind::Closure(name) = binding.kind()
            {
                let child_global = template_names.iter().any(|t| t == name.as_str());
                return node.children().any(|child| {
                    header_resets_footnote_counter(child, template_names, child_global)
                });
            }
        }
        _ => {}
    }

    if in_global_scope {
        if node.kind() == SyntaxKind::SetRule
            && let Some(set) = node.cast::<ast::SetRule<'_>>()
            && let ast::Expr::Ident(ident) = set.target()
            && ident.as_str() == "page"
            && page_header_resets_footnote_counter(set.args())
        {
            return true;
        }

        // Also honor the `#page(header: …)` function-call form.
        if node.kind() == SyntaxKind::FuncCall
            && let Some(call) = node.cast::<ast::FuncCall<'_>>()
            && matches!(call.callee(), ast::Expr::Ident(i) if i.as_str() == "page")
            && page_header_resets_footnote_counter(call.args())
        {
            return true;
        }
    }

    node.children()
        .any(|child| header_resets_footnote_counter(child, template_names, in_global_scope))
}

/// Whether the `header:` argument of a page rule resets the footnote counter.
fn page_header_resets_footnote_counter(args: typst_syntax::ast::Args<'_>) -> bool {
    use typst_syntax::ast::{self, AstNode};
    for arg in args.items() {
        if let ast::Arg::Named(named) = arg
            && named.name().as_str() == "header"
            && subtree_resets_footnote_counter(named.expr().to_untyped())
        {
            return true;
        }
    }
    false
}

/// Whether any `counter(footnote).update(<int literal>)` call occurs under
/// `node`.
fn subtree_resets_footnote_counter(node: &typst_syntax::SyntaxNode) -> bool {
    use typst_syntax::ast;
    if let Some(call) = node.cast::<ast::FuncCall<'_>>()
        && is_footnote_counter_update(call)
    {
        return true;
    }
    node.children().any(subtree_resets_footnote_counter)
}

/// Whether this call is `counter(footnote).update(<int literal>)`.
fn is_footnote_counter_update(call: typst_syntax::ast::FuncCall<'_>) -> bool {
    use typst_syntax::ast;
    let ast::Expr::FieldAccess(access) = call.callee() else {
        return false;
    };
    if access.field().get().as_str() != "update" {
        return false;
    }
    let ast::Expr::FuncCall(inner) = access.target() else {
        return false;
    };
    if !matches!(inner.callee(), ast::Expr::Ident(ident) if ident.get().as_str() == "counter") {
        return false;
    }
    let mut counter_args = inner.args().items();
    if !matches!(counter_args.next(), Some(ast::Arg::Pos(ast::Expr::Ident(ident)))
        if ident.get().as_str() == "footnote")
    {
        return false;
    }
    let mut update_args = call.args().items();
    matches!(update_args.next(), Some(ast::Arg::Pos(ast::Expr::Int(_))))
}
#[must_use]
pub fn extract_show_template_names_from_source(source: &str) -> Vec<String> {
    let root = typst_syntax::parse(source);
    extract_show_template_names(&root)
}

/// Collect the closure names referenced by document-wide `#show: NAME` /
/// `#show: NAME.with(...)` rules (`selector()` is `None`). The named closure
/// holds the document's real global `set` rules, so its body must be treated as
/// global scope during [`collect_global_set_rules`].
fn extract_show_template_names(root: &typst_syntax::SyntaxNode) -> Vec<String> {
    use typst_syntax::SyntaxKind;
    use typst_syntax::ast;

    let mut names = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(node) = stack.pop() {
        if node.kind() == SyntaxKind::ShowRule
            && let Some(show) = node.cast::<ast::ShowRule<'_>>()
            && show.selector().is_none()
        {
            match show.transform() {
                // `#show: tmpl.with(...)` => FuncCall whose callee is a
                // FieldAccess `tmpl.with`; the template is the FieldAccess target.
                ast::Expr::FuncCall(call) => {
                    if let ast::Expr::FieldAccess(access) = call.callee()
                        && access.field().as_str() == "with"
                        && let ast::Expr::Ident(ident) = access.target()
                    {
                        names.push(ident.as_str().to_string());
                    } else if let ast::Expr::Ident(ident) = call.callee() {
                        // `#show: tmpl()` => bare ident callee.
                        names.push(ident.as_str().to_string());
                    }
                }
                // `#show: tmpl` => the transform is the ident itself.
                ast::Expr::Ident(ident) => names.push(ident.as_str().to_string()),
                _ => {}
            }
        }
        for child in node.children() {
            stack.push(child.clone());
        }
    }
    names
}

/// A `#set par(hanging-indent: …)` rule recovered from the source, including its
/// resolved literal/constant length and lexical scope.
fn collect_global_set_rules(
    node: &typst_syntax::SyntaxNode,
    template_names: &[String],
    in_global_scope: bool,
    ovr: &mut SourceStyleOverrides,
) {
    use typst_syntax::SyntaxKind;
    use typst_syntax::ast;

    match node.kind() {
        // A show rule's recipe applies to specific elements, not globally — and
        // its own `set` rules are element-scoped. Don't descend.
        SyntaxKind::ShowRule => return,
        // A content block (`[...]` / `#block[...]`) introduces a local scope.
        SyntaxKind::ContentBlock => {
            for child in node.children() {
                collect_global_set_rules(child, template_names, false, ovr);
            }
            return;
        }
        // A `#let NAME(...) = …` closure. The show-template closure holds the
        // real document globals; any other closure is a local helper.
        SyntaxKind::LetBinding => {
            if let Some(binding) = node.cast::<ast::LetBinding<'_>>()
                && let ast::LetBindingKind::Closure(name) = binding.kind()
            {
                let child_global = template_names.iter().any(|t| t == name.as_str());
                for child in node.children() {
                    collect_global_set_rules(child, template_names, child_global, ovr);
                }
                return;
            }
        }
        _ => {}
    }

    if in_global_scope {
        if node.kind() == SyntaxKind::SetRule
            && let Some(set) = node.cast::<ast::SetRule<'_>>()
            && let ast::Expr::Ident(ident) = set.target()
        {
            match ident.as_str() {
                "page" => parse_page_args(set.args(), ovr),
                "text" => parse_text_args(set.args(), ovr),
                "par" => parse_par_args(set.args(), ovr),
                _ => {}
            }
        }

        // Also honor the `#page(...)` function-call form (e.g.
        // `#page(columns: 2)[…]`), not just `#set page(...)`. Its named args
        // carry the same page settings.
        if node.kind() == SyntaxKind::FuncCall
            && let Some(call) = node.cast::<ast::FuncCall<'_>>()
            && matches!(call.callee(), ast::Expr::Ident(i) if i.as_str() == "page")
        {
            parse_page_args(call.args(), ovr);
        }
    }

    for child in node.children() {
        collect_global_set_rules(child, template_names, in_global_scope, ovr);
    }
}

fn parse_page_args(args: typst_syntax::ast::Args<'_>, ovr: &mut SourceStyleOverrides) {
    for arg in args.items() {
        let typst_syntax::ast::Arg::Named(named) = arg else {
            continue;
        };
        match named.name().as_str() {
            "margin" => parse_margin_value(named.expr(), ovr),
            "columns" => {
                if let typst_syntax::ast::Expr::Int(i) = named.expr() {
                    ovr.columns = u32::try_from(i.get()).ok();
                }
            }
            "numbering" => match named.expr() {
                typst_syntax::ast::Expr::Str(s) => {
                    ovr.page_numbering = Some(PageNumberingDecl::Pattern(s.get().to_string()));
                }
                // `numbering: none` disables numbering — no declaration.
                typst_syntax::ast::Expr::None(_) => {}
                // A function, a variable, … — declared but not mappable.
                _ => {
                    ovr.page_numbering = Some(PageNumberingDecl::Unmappable);
                }
            },
            _ => {}
        }
    }
}

/// A `#set page(margin: …)` value as declared: absolute twips, or em to
/// resolve later against the body size. Zero converts to neither (a zero
/// margin is treated as unset, like the absolute path below).
fn margin_amount(n: typst_syntax::ast::Numeric<'_>) -> (Option<u32>, Option<f64>) {
    let (value, unit) = n.get();
    if unit == typst_syntax::ast::Unit::Em {
        (None, if value == 0.0 { None } else { Some(value) })
    } else {
        let twips = numeric_to_twips(n);
        (if twips > 0 { Some(twips) } else { None }, None)
    }
}

/// Pick the winning margin value from a priority chain (first = lowest
/// priority): the last entry carrying a value on either track wins.
fn pick_margin_amount(chain: &[(Option<u32>, Option<f64>)]) -> (Option<u32>, Option<f64>) {
    chain
        .iter()
        .rev()
        .find(|(twips, em)| twips.is_some() || em.is_some())
        .copied()
        .unwrap_or((None, None))
}

fn parse_margin_value(expr: typst_syntax::ast::Expr<'_>, ovr: &mut SourceStyleOverrides) {
    match expr {
        typst_syntax::ast::Expr::Numeric(n) => {
            let (twips, em) = margin_amount(n);
            if twips.is_some() || em.is_some() {
                ovr.margin_top = twips;
                ovr.margin_bottom = twips;
                ovr.margin_left = twips;
                ovr.margin_right = twips;
                ovr.margin_top_em = em;
                ovr.margin_bottom_em = em;
                ovr.margin_left_em = em;
                ovr.margin_right_em = em;
            }
        }
        typst_syntax::ast::Expr::Dict(dict) => {
            // Collect with Typst priority: rest < x/y < individual sides
            let mut rest = (None, None);
            let mut x = (None, None);
            let mut y = (None, None);
            let mut top = (None, None);
            let mut bottom = (None, None);
            let mut left = (None, None);
            let mut right = (None, None);

            for item in dict.items() {
                let typst_syntax::ast::DictItem::Named(entry) = item else {
                    continue;
                };
                if let typst_syntax::ast::Expr::Numeric(n) = entry.expr() {
                    let amount = margin_amount(n);
                    match entry.name().as_str() {
                        "rest" => rest = amount,
                        "x" => x = amount,
                        "y" => y = amount,
                        "top" => top = amount,
                        "bottom" => bottom = amount,
                        "left" => left = amount,
                        "right" => right = amount,
                        _ => {}
                    }
                }
            }

            // Resolve in priority order, each track separately.
            let (top_twips, top_em) = pick_margin_amount(&[rest, y, top]);
            let (bottom_twips, bottom_em) = pick_margin_amount(&[rest, y, bottom]);
            let (left_twips, left_em) = pick_margin_amount(&[rest, x, left]);
            let (right_twips, right_em) = pick_margin_amount(&[rest, x, right]);
            if top_twips.is_some() || top_em.is_some() {
                ovr.margin_top = top_twips;
                ovr.margin_top_em = top_em;
            }
            if bottom_twips.is_some() || bottom_em.is_some() {
                ovr.margin_bottom = bottom_twips;
                ovr.margin_bottom_em = bottom_em;
            }
            if left_twips.is_some() || left_em.is_some() {
                ovr.margin_left = left_twips;
                ovr.margin_left_em = left_em;
            }
            if right_twips.is_some() || right_em.is_some() {
                ovr.margin_right = right_twips;
                ovr.margin_right_em = right_em;
            }
        }
        _ => {}
    }
}

fn parse_text_args(args: typst_syntax::ast::Args<'_>, ovr: &mut SourceStyleOverrides) {
    for arg in args.items() {
        match arg {
            typst_syntax::ast::Arg::Named(named) => match named.name().as_str() {
                "font" => {
                    if ovr.text_font.is_none() {
                        ovr.text_font = extract_font_list(named.expr());
                    }
                }
                "size" => {
                    if ovr.text_size_half_pt.is_none()
                        && let typst_syntax::ast::Expr::Numeric(n) = named.expr()
                    {
                        let half_pt = numeric_to_half_pt(n);
                        if half_pt > 0 {
                            ovr.text_size_half_pt = Some(half_pt);
                        }
                    }
                }
                "lang" => {
                    if ovr.text_lang.is_none()
                        && let typst_syntax::ast::Expr::Str(s) = named.expr()
                    {
                        ovr.text_lang = Some(s.get().to_string());
                    }
                }
                "region" => {
                    if ovr.text_region.is_none()
                        && let typst_syntax::ast::Expr::Str(s) = named.expr()
                    {
                        ovr.text_region = Some(s.get().to_string());
                    }
                }
                _ => {}
            },
            typst_syntax::ast::Arg::Pos(typst_syntax::ast::Expr::Numeric(n)) => {
                let half_pt = numeric_to_half_pt(n);
                if half_pt > 0 {
                    ovr.text_size_half_pt = Some(half_pt);
                }
            }
            _ => {}
        }
    }
}

/// Record a first-line-indent amount (em or absolute) onto the overrides.
fn set_first_line_indent_amount(n: typst_syntax::ast::Numeric<'_>, ovr: &mut SourceStyleOverrides) {
    let (value, unit) = n.get();
    if unit == typst_syntax::ast::Unit::Em {
        ovr.first_line_indent_em = Some(value);
    } else {
        ovr.first_line_indent_twips = Some(numeric_to_twips(n));
    }
}

fn parse_par_args(args: typst_syntax::ast::Args<'_>, ovr: &mut SourceStyleOverrides) {
    for arg in args.items() {
        let typst_syntax::ast::Arg::Named(named) = arg else {
            continue;
        };
        match named.name().as_str() {
            "first-line-indent" => {
                if ovr.first_line_indent_twips.is_none() && ovr.first_line_indent_em.is_none() {
                    match named.expr() {
                        typst_syntax::ast::Expr::Numeric(n) => set_first_line_indent_amount(n, ovr),
                        // `(amount: 2em, all: true)`: indent every paragraph,
                        // including the first one after a heading.
                        typst_syntax::ast::Expr::Dict(dict) => {
                            for item in dict.items() {
                                let typst_syntax::ast::DictItem::Named(entry) = item else {
                                    continue;
                                };
                                match entry.name().as_str() {
                                    "amount" => {
                                        if let typst_syntax::ast::Expr::Numeric(n) = entry.expr() {
                                            set_first_line_indent_amount(n, ovr);
                                        }
                                    }
                                    "all" => {
                                        if let typst_syntax::ast::Expr::Bool(b) = entry.expr() {
                                            ovr.first_line_indent_all = Some(b.get());
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            "leading" => {
                if ovr.par_leading_twips.is_none()
                    && ovr.par_leading_em.is_none()
                    && let typst_syntax::ast::Expr::Numeric(n) = named.expr()
                {
                    let (value, unit) = n.get();
                    if unit == typst_syntax::ast::Unit::Em {
                        ovr.par_leading_em = Some(value);
                    } else {
                        ovr.par_leading_twips = Some(numeric_to_twips(n));
                    }
                }
            }
            "spacing" => {
                if ovr.par_spacing_twips.is_none()
                    && ovr.par_spacing_em.is_none()
                    && let typst_syntax::ast::Expr::Numeric(n) = named.expr()
                {
                    let (value, unit) = n.get();
                    if unit == typst_syntax::ast::Unit::Em {
                        ovr.par_spacing_em = Some(value);
                    } else {
                        ovr.par_spacing_twips = Some(numeric_to_twips(n));
                    }
                }
            }
            "justify" => {
                if ovr.justify.is_none()
                    && let typst_syntax::ast::Expr::Bool(b) = named.expr()
                {
                    ovr.justify = Some(b.get());
                }
            }
            _ => {}
        }
    }
}

fn extract_font_list(expr: typst_syntax::ast::Expr<'_>) -> Option<Vec<String>> {
    match expr {
        typst_syntax::ast::Expr::Str(s) => Some(vec![s.get().to_string()]),
        typst_syntax::ast::Expr::Array(arr) => {
            let fonts: Vec<String> = arr
                .items()
                .filter_map(|item| {
                    if let typst_syntax::ast::ArrayItem::Pos(typst_syntax::ast::Expr::Str(s)) = item
                    {
                        Some(s.get().to_string())
                    } else {
                        None
                    }
                })
                .collect();
            if fonts.is_empty() { None } else { Some(fonts) }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::units::pt_to_twips;
    use super::*;

    #[test]
    fn source_ast_extracts_text_size_and_par_spacing() {
        let source = r#"#set text(font: "Linux Libertine", size: 10.5pt)"#;
        let ovr = extract_source_style_overrides(source, &[]);
        assert_eq!(ovr.text_size_half_pt, Some(21));
        assert_eq!(
            ovr.text_font.as_deref(),
            Some(&["Linux Libertine".to_string()][..])
        );
    }

    #[test]
    fn source_ast_extracts_par_settings() {
        let source = r#"#set par(first-line-indent: 2em, spacing: 1.5em, justify: true)"#;
        let ovr = extract_source_style_overrides(source, &[]);
        assert_eq!(ovr.first_line_indent_em, Some(2.0));
        assert_eq!(ovr.par_spacing_em, Some(1.5));
        assert_eq!(ovr.justify, Some(true));
    }

    #[test]
    fn source_ast_extracts_page_margin() {
        let source = r#"#set page(margin: 2cm)"#;
        let ovr = extract_source_style_overrides(source, &[]);
        let expected = pt_to_twips(2.0_f64 * 72.0 / 2.54);
        assert_eq!(ovr.margin_top, Some(expected));
        assert_eq!(ovr.margin_left, Some(expected));
    }

    #[test]
    fn source_ast_extracts_page_margin_em_deferred() {
        // em margins are stored, not converted: the 12pt assumption is gone.
        let source = r#"#set page(margin: 2em)"#;
        let ovr = extract_source_style_overrides(source, &[]);
        assert_eq!(ovr.margin_top_em, Some(2.0));
        assert_eq!(ovr.margin_bottom_em, Some(2.0));
        assert_eq!(ovr.margin_left_em, Some(2.0));
        assert_eq!(ovr.margin_right_em, Some(2.0));
        assert_eq!(ovr.margin_top, None);
    }

    #[test]
    fn source_ast_extracts_page_margin_mixed_units() {
        let source = r#"#set page(margin: (x: 1em, top: 2cm))"#;
        let ovr = extract_source_style_overrides(source, &[]);
        assert_eq!(ovr.margin_left_em, Some(1.0));
        assert_eq!(ovr.margin_right_em, Some(1.0));
        assert_eq!(ovr.margin_left, None);
        let expected = pt_to_twips(2.0_f64 * 72.0 / 2.54);
        assert_eq!(ovr.margin_top, Some(expected));
        assert_eq!(ovr.margin_top_em, None);
        assert_eq!(ovr.margin_bottom, None);
        assert_eq!(ovr.margin_bottom_em, None);
    }

    #[test]
    fn source_ast_complex_paper_pattern() {
        let source = r#"#set par(leading: 1.5em, first-line-indent: 2em)"#;
        let ovr = extract_source_style_overrides(source, &[]);
        assert!(
            ovr.first_line_indent_em.is_some(),
            "first-line-indent should be detected, got None"
        );
        assert!(
            ovr.first_line_indent_em.unwrap() > 0.0,
            "first-line-indent should be > 0"
        );
        assert!(ovr.par_leading_em.is_some(), "leading should be detected");
    }

    #[test]
    fn source_ast_scope_aware_show_template_body_size() {
        // The real global body size lives inside the show-template closure;
        // a 9pt helper closure and a 9pt nested block must NOT win.
        let body = r#"
#let helper() = {
  set text(size: 9pt)
  [helper text]
}
#let tmpl(body) = {
  set text(size: 12pt)
  body
}
= Heading
Body paragraph.
#block[
  #set text(size: 9pt)
  A small block.
]
"#;

        let with_call = format!("{body}\n#show: tmpl.with()");
        let names = extract_show_template_names_from_source(&with_call);
        assert_eq!(names, vec!["tmpl".to_string()]);
        let ovr = extract_source_style_overrides(&with_call, &names);
        assert_eq!(
            ovr.text_size_half_pt,
            Some(24),
            "show-template body size (12pt) must win over helper/block 9pt"
        );

        // Bare `#show: tmpl` form resolves identically.
        let bare = format!("{body}\n#show: tmpl");
        let bare_names = extract_show_template_names_from_source(&bare);
        assert_eq!(bare_names, vec!["tmpl".to_string()]);
        let ovr_bare = extract_source_style_overrides(&bare, &bare_names);
        assert_eq!(ovr_bare.text_size_half_pt, Some(24));

        // No template at all: a bare top-level `#set text` still resolves.
        let top_level = r#"#set text(size: 10.5pt)
= Heading
Body."#;
        let top_names = extract_show_template_names_from_source(top_level);
        assert!(top_names.is_empty());
        let ovr_top = extract_source_style_overrides(top_level, &top_names);
        assert_eq!(ovr_top.text_size_half_pt, Some(21));
    }

    #[test]
    fn footnote_counter_reset_in_page_header_cases() {
        let none: &[String] = &[];
        // The canonical per-page reset: an integer update in the header.
        assert!(footnote_counter_reset_in_page_header(
            "#set page(header: context counter(footnote).update(0))",
            none
        ));
        // A function-valued update is not a reset.
        assert!(!footnote_counter_reset_in_page_header(
            "#set page(header: context counter(footnote).update(n => n + 1))",
            none
        ));
        // A header without a reset does not count.
        assert!(!footnote_counter_reset_in_page_header(
            "#set page(header: [Running head])",
            none
        ));
        // An update in the body restarts numbering once; it is not per-page.
        assert!(!footnote_counter_reset_in_page_header(
            "#set page(header: [Running head])\n\n#counter(footnote).update(0)",
            none
        ));
        // The `#page(header: …)` call form counts too.
        assert!(footnote_counter_reset_in_page_header(
            "#page(header: context counter(footnote).update(0))[Body.]",
            none
        ));
        // A header set rule buried in a content block is locally scoped.
        assert!(!footnote_counter_reset_in_page_header(
            "#block[#set page(header: context counter(footnote).update(0))]",
            none
        ));
        // …but one inside the show-template closure is document-global.
        let tmpl = "#let tmpl(..args) = {\n  set page(header: context counter(footnote).update(0))\n  args.pos().join()\n}\n#show: tmpl.with()\n= Heading\nBody.";
        let names = extract_show_template_names_from_source(tmpl);
        assert_eq!(names, vec!["tmpl".to_string()]);
        assert!(footnote_counter_reset_in_page_header(tmpl, &names));
    }
}
