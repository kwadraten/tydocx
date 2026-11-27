use super::units::{numeric_to_half_pt, numeric_to_twips};
use crate::ooxml::document::VerticalAlign;

/// All style overrides extracted from `#set` rules in the source AST.
/// Each field is `None` if the source doesn't set it (use heuristic fallback).
#[derive(Clone, Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each flag is a distinct source-declared fact; grouping them is artificial"
)]
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
    pub numbering_changed: bool,
    pub margins_mirrored: bool,
    /// A margin dict resolved left ≠ right without inside/outside — combined
    /// with `margins_mirrored` it means the document-global `w:mirrorMargins`
    /// also swaps that section's margins on even pages.
    pub one_sided_asymmetric_margins: bool,
    pub page_furniture: bool,
    // Which furniture slot was declared — lets the paged-frame walk know a
    // trailing `place` tag at the page origin can only be `page(foreground:)`
    // content, not an in-body placement at a zero margin.
    pub page_foreground: bool,
    pub page_background: bool,
    // A `#show heading:` / `#show heading.where(...):` rule can consume the
    // HeadingElem and replace it with arbitrary content — losing the Heading
    // style and the outline entry.
    pub show_heading_rule: bool,
    pub show_outline_entry_rule: bool,
    // `#move`/`#rotate`/`#scale`/`#skew` calls leave no element trace in the
    // realize pairs — the transform is applied during layout.
    pub move_call: bool,
    pub rotate_call: bool,
    pub scale_call: bool,
    pub skew_call: bool,
    // #set page(header-ascent: / footer-descent:) — `w:pgMar w:header/footer`.
    pub header_twips: Option<u32>,
    pub header_em: Option<f64>,
    pub footer_twips: Option<u32>,
    pub footer_em: Option<f64>,
    // #set par.line(numbering:) enables OOXML line numbering;
    // `number-clearance` is the `w:lnNumType w:distance`.
    pub line_numbering: Option<bool>,
    pub line_number_distance_twips: Option<u32>,
    pub line_number_distance_em: Option<f64>,
    // Document-global `#set align(… + horizon|bottom|top …)` — the page's
    // vertical placement, `w:sectPr/w:vAlign`.
    pub vertical_align: Option<VerticalAlign>,
    // #set columns(gutter:) — `w:cols w:space`.
    pub column_gutter_twips: Option<u32>,
    pub column_gutter_em: Option<f64>,
    // #set footnote.entry(clearance:/gap:/indent:/separator: line(stroke:))
    pub footnote_clearance_twips: Option<u32>,
    pub footnote_clearance_em: Option<f64>,
    pub footnote_gap_twips: Option<u32>,
    pub footnote_gap_em: Option<f64>,
    pub footnote_indent_twips: Option<u32>,
    pub footnote_indent_em: Option<f64>,
    pub footnote_separator_stroke_eighths: Option<u32>,
    // `#show footnote.entry: set par(justify:)` — entry-scoped; the footnote
    // paragraphs pin it so `FootnoteText` does not inherit Normal's `w:jc`.
    pub footnote_par_justify: Option<bool>,
    // #set text(font: ..., size: ...)
    pub text_font: Option<Vec<String>>,
    pub text_size_half_pt: Option<u32>,
    // #set text(lang: "zh", region: "cn") — ISO 639 lang + optional ISO 3166 region.
    pub text_lang: Option<String>,
    pub text_region: Option<String>,
    pub text_hyphenate: Option<bool>,
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
        self.numbering_changed |= other.numbering_changed;
        self.margins_mirrored |= other.margins_mirrored;
        self.one_sided_asymmetric_margins |= other.one_sided_asymmetric_margins;
        self.page_furniture |= other.page_furniture;
        self.page_foreground |= other.page_foreground;
        self.page_background |= other.page_background;
        self.show_heading_rule |= other.show_heading_rule;
        self.show_outline_entry_rule |= other.show_outline_entry_rule;
        self.move_call |= other.move_call;
        self.rotate_call |= other.rotate_call;
        self.scale_call |= other.scale_call;
        self.skew_call |= other.skew_call;
        fill!(header_twips);
        fill!(header_em);
        fill!(footer_twips);
        fill!(footer_em);
        fill!(line_numbering);
        fill!(line_number_distance_twips);
        fill!(line_number_distance_em);
        fill!(vertical_align);
        fill!(column_gutter_twips);
        fill!(column_gutter_em);
        fill!(footnote_clearance_twips);
        fill!(footnote_clearance_em);
        fill!(footnote_gap_twips);
        fill!(footnote_gap_em);
        fill!(footnote_indent_twips);
        fill!(footnote_indent_em);
        fill!(footnote_separator_stroke_eighths);
        fill!(footnote_par_justify);
        fill!(text_font);
        fill!(text_size_half_pt);
        fill!(text_lang);
        fill!(text_region);
        fill!(text_hyphenate);
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
    scan_transform_calls(&root, &mut ovr);
    ovr
}

/// Records a call to `move`/`rotate`/`scale`/`skew` anywhere in the source —
/// the layout elements are consumed before the realize pairs, so their
/// presence can only be seen in the AST.
fn scan_transform_calls(node: &typst_syntax::SyntaxNode, ovr: &mut SourceStyleOverrides) {
    use typst_syntax::ast;
    if let Some(args) = page_args(node) {
        for arg in args {
            if !is_furniture_named(arg) {
                scan_transform_calls(arg, ovr);
            }
        }
        return;
    }
    if node.kind() == typst_syntax::SyntaxKind::FuncCall
        && let Some(call) = node.cast::<ast::FuncCall<'_>>()
        && let ast::Expr::Ident(ident) = call.callee()
    {
        match ident.as_str() {
            "move" => ovr.move_call = true,
            "rotate" => ovr.rotate_call = true,
            "scale" => ovr.scale_call = true,
            "skew" => ovr.skew_call = true,
            _ => {}
        }
    }
    for child in node.children() {
        scan_transform_calls(child, ovr);
    }
}

/// The literal markup text of every `move`/`rotate`/`scale`/`skew` body in
/// source order. Those elements are consumed during layout — the realize
/// pairs never see them — so the bodies' text is recovered from the AST.
#[must_use]
pub fn transform_bodies_text(source: &str) -> Vec<String> {
    let root = typst_syntax::parse(source);
    let mut out = Vec::new();
    collect_transform_text(&root, &mut out);
    out
}

/// The arg nodes of a `#page(...)` call or `#set page(...)` rule — `Some`
/// marks a node whose `header`/`footer`/`foreground`/`background` named args
/// are page furniture and must be skipped during body-text recovery. A
/// `SetRule` is not a `FuncCall` node, so both shapes are checked. The
/// positional body arg of `#page(...)[body]` stays in scope.
fn page_args(node: &typst_syntax::SyntaxNode) -> Option<Vec<&typst_syntax::SyntaxNode>> {
    use typst_syntax::ast;
    use typst_syntax::ast::AstNode as _;
    let args = match node.kind() {
        typst_syntax::SyntaxKind::FuncCall => node
            .cast::<ast::FuncCall<'_>>()
            .filter(
                |call| matches!(call.callee(), ast::Expr::Ident(ident) if ident.as_str() == "page"),
            )
            .map(ast::FuncCall::args),
        typst_syntax::SyntaxKind::SetRule => node
            .cast::<ast::SetRule<'_>>()
            .filter(
                |rule| matches!(rule.target(), ast::Expr::Ident(ident) if ident.as_str() == "page"),
            )
            .map(ast::SetRule::args),
        _ => None,
    }?;
    Some(args.items().map(ast::Arg::to_untyped).collect())
}

/// A `header:`/`footer:`/`foreground:`/`background:` named arg — furniture.
fn is_furniture_named(node: &typst_syntax::SyntaxNode) -> bool {
    node.cast::<typst_syntax::ast::Named<'_>>()
        .is_some_and(|named| {
            matches!(
                named.name().get().as_str(),
                "header" | "footer" | "foreground" | "background"
            )
        })
}

fn collect_transform_text(node: &typst_syntax::SyntaxNode, out: &mut Vec<String>) {
    use typst_syntax::ast::{self, AstNode};
    // `page` args carry header/footer/foreground/background content —
    // furniture, not body text. Descend into the other args only.
    if let Some(args) = page_args(node) {
        for arg in args {
            if !is_furniture_named(arg) {
                collect_transform_text(arg, out);
            }
        }
        return;
    }
    if node.kind() == typst_syntax::SyntaxKind::FuncCall
        && let Some(call) = node.cast::<ast::FuncCall<'_>>()
        && let ast::Expr::Ident(ident) = call.callee()
        && matches!(ident.as_str(), "move" | "rotate" | "scale" | "skew")
    {
        for arg in call.args().items() {
            if let ast::Arg::Pos(ast::Expr::ContentBlock(block)) = arg {
                let text = markup_text(block.to_untyped());
                if !text.is_empty() {
                    out.push(text);
                }
            }
        }
    }
    for child in node.children() {
        collect_transform_text(child, out);
    }
}

fn markup_text(node: &typst_syntax::SyntaxNode) -> String {
    let mut text = String::new();
    for child in node.children() {
        match child.kind() {
            typst_syntax::SyntaxKind::Text => text.push_str(child.leaf_text()),
            typst_syntax::SyntaxKind::Space => text.push(' '),
            _ => text.push_str(&markup_text(child)),
        }
    }
    text
}

/// A heading query detected in a `page(header:)`/`page(footer:)` zone, for
/// the C3 `STYLEREF` field.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ZoneStyleref {
    /// Queried heading level (`where(level: N)`, default 1).
    pub level: u8,
    /// Literal text runs collected from the zone's markup in source order —
    /// used to isolate the query-result span inside a rendered zone
    /// paragraph (`[Report — #q]` → `["Report — "]`).
    pub literals: Vec<String>,
    /// More than one heading query — the result span can't be isolated.
    pub multiple_queries: bool,
}

/// Heading queries found in the page zones.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageZoneHeadings {
    /// `Some` when the header zone contains a heading query; `None` for
    /// static text or no query.
    pub header: Option<ZoneStyleref>,
    /// Same for the footer zone.
    pub footer: Option<ZoneStyleref>,
}

/// The heading query each page zone's content makes — document-global
/// `set page(header:/footer:)` rules (same scoping as
/// [`footnote_counter_reset_in_page_header`]). The zone may mix the query
/// with literal markup; [`ZoneStyleref::literals`] lets the wrapper isolate
/// the query-result span.
#[must_use]
pub fn page_zone_heading_levels(source: &str, template_names: &[String]) -> PageZoneHeadings {
    let root = typst_syntax::parse(source);
    let mut out = PageZoneHeadings::default();
    collect_zone_headings(&root, template_names, true, &mut out);
    out
}

/// Mirror of [`header_resets_footnote_counter`]'s scoping walk.
fn collect_zone_headings(
    node: &typst_syntax::SyntaxNode,
    template_names: &[String],
    in_global_scope: bool,
    out: &mut PageZoneHeadings,
) {
    use typst_syntax::SyntaxKind;
    use typst_syntax::ast;

    match node.kind() {
        SyntaxKind::ShowRule => return,
        SyntaxKind::ContentBlock => {
            for child in node.children() {
                collect_zone_headings(child, template_names, false, out);
            }
            return;
        }
        SyntaxKind::LetBinding => {
            if let Some(binding) = node.cast::<ast::LetBinding<'_>>()
                && let ast::LetBindingKind::Closure(name) = binding.kind()
            {
                let child_global = template_names.iter().any(|t| t == name.as_str());
                for child in node.children() {
                    collect_zone_headings(child, template_names, child_global, out);
                }
                return;
            }
        }
        _ => {}
    }

    if in_global_scope {
        let args = match node.kind() {
            SyntaxKind::SetRule => node
                .cast::<ast::SetRule<'_>>()
                .filter(|set| matches!(set.target(), ast::Expr::Ident(i) if i.as_str() == "page"))
                .map(ast::SetRule::args),
            SyntaxKind::FuncCall => node
                .cast::<ast::FuncCall<'_>>()
                .filter(|call| matches!(call.callee(), ast::Expr::Ident(i) if i.as_str() == "page"))
                .map(ast::FuncCall::args),
            _ => None,
        };
        if let Some(args) = args {
            for arg in args.items() {
                if let ast::Arg::Named(named) = arg {
                    let slot = match named.name().as_str() {
                        "header" => &mut out.header,
                        "footer" => &mut out.footer,
                        _ => continue,
                    };
                    *slot = zone_styleref(&named.expr()).or_else(|| slot.take());
                }
            }
        }
    }

    for child in node.children() {
        collect_zone_headings(child, template_names, in_global_scope, out);
    }
}

/// `Some` when the zone content contains a heading query, with the literal
/// text runs recoverable from the zone's markup.
fn zone_styleref(expr: &typst_syntax::ast::Expr<'_>) -> Option<ZoneStyleref> {
    use typst_syntax::ast::AstNode;
    let (level, queries) = heading_query_level(expr.to_untyped())?;
    let mut literals = Vec::new();
    collect_literal_pieces(expr.to_untyped(), &mut literals);
    Some(ZoneStyleref {
        level,
        literals,
        multiple_queries: queries > 1,
    })
}

/// Collect each maximal run of markup text (`Text`/`Space`) in source
/// order — every code node between literals is a piece boundary. Blank
/// pieces (whitespace between code tokens) are dropped.
fn collect_literal_pieces(node: &typst_syntax::SyntaxNode, pieces: &mut Vec<String>) {
    use typst_syntax::SyntaxKind;

    fn flush(current: &mut String, pieces: &mut Vec<String>) {
        if !current.trim().is_empty() && !pieces.contains(current) {
            pieces.push(std::mem::take(current));
        } else {
            current.clear();
        }
    }
    fn walk(node: &typst_syntax::SyntaxNode, current: &mut String, pieces: &mut Vec<String>) {
        for child in node.children() {
            match child.kind() {
                SyntaxKind::Text => current.push_str(child.leaf_text()),
                SyntaxKind::Space => current.push(' '),
                _ => {
                    flush(current, pieces);
                    walk(child, current, pieces);
                }
            }
        }
    }

    let mut current = String::new();
    walk(node, &mut current, pieces);
    flush(&mut current, pieces);
}

/// Walk the subtree for `heading` referenced inside a query/context
/// expression, picking up `where(level: N)` along the way. Returns the
/// level and the number of heading queries found.
fn heading_query_level(node: &typst_syntax::SyntaxNode) -> Option<(u8, usize)> {
    use typst_syntax::SyntaxKind;
    use typst_syntax::ast;

    let mut has_heading = false;
    let mut has_queryish = false;
    let mut queries = 0usize;
    let mut level = None;
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        match n.kind() {
            SyntaxKind::Contextual => has_queryish = true,
            SyntaxKind::FuncCall => {
                if let Some(call) = n.cast::<ast::FuncCall<'_>>()
                    && matches!(
                        call.callee(),
                        ast::Expr::Ident(i)
                            if matches!(
                                i.get().as_str(),
                                "query" | "selector" | "locate" | "counter" | "where"
                            )
                    )
                {
                    has_queryish = true;
                    if matches!(
                        call.callee(),
                        ast::Expr::Ident(i)
                            if matches!(i.get().as_str(), "query" | "locate" | "counter")
                    ) && has_ident(n, "heading")
                    {
                        queries += 1;
                    }
                }
            }
            _ => {}
        }
        if let Some(ident) = n.cast::<ast::Ident<'_>>()
            && ident.get().as_str() == "heading"
        {
            has_heading = true;
        }
        if let Some(named) = n.cast::<ast::Named<'_>>()
            && named.name().as_str() == "level"
            && let ast::Expr::Int(value) = named.expr()
        {
            level = u8::try_from(value.get()).ok();
        }
        stack.extend(n.children());
    }
    (has_heading && has_queryish).then_some((level.unwrap_or(1), queries))
}

/// Whether the subtree contains the identifier `name`.
fn has_ident(node: &typst_syntax::SyntaxNode, name: &str) -> bool {
    let mut stack = vec![node.clone()];
    while let Some(n) = stack.pop() {
        if let Some(ident) = n.cast::<typst_syntax::ast::Ident<'_>>()
            && ident.get().as_str() == name
        {
            return true;
        }
        stack.extend(n.children().cloned());
    }
    false
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

/// True when `node` (a show-rule selector expression) mentions `name` —
/// e.g. `heading`, `heading.where(level: 1)`, or `heading.where(..).or(..)`.
fn syntax_mentions_ident(node: &typst_syntax::SyntaxNode, name: &str) -> bool {
    if let Some(ident) = node.cast::<typst_syntax::ast::Ident<'_>>()
        && ident.as_str() == name
    {
        return true;
    }
    node.children()
        .any(|child| syntax_mentions_ident(child, name))
}

fn collect_global_set_rules(
    node: &typst_syntax::SyntaxNode,
    template_names: &[String],
    in_global_scope: bool,
    ovr: &mut SourceStyleOverrides,
) {
    use typst_syntax::SyntaxKind;
    use typst_syntax::ast::{self, AstNode};

    match node.kind() {
        // A show rule's recipe applies to specific elements, not globally — and
        // its own `set` rules are element-scoped. Don't descend — but note a
        // rule that selects `heading` and drops `it` from its recipe body:
        // the heading is then replaced by arbitrary content and loses its
        // identity. Recipes that use `it` keep the element.
        SyntaxKind::ShowRule => {
            if let Some(show) = node.cast::<ast::ShowRule<'_>>()
                && let Some(selector) = show.selector()
            {
                let recipe_uses_it = match show.transform() {
                    ast::Expr::Closure(closure) => {
                        syntax_mentions_ident(closure.body().to_untyped(), "it")
                    }
                    _ => false,
                };
                if syntax_mentions_ident(selector.to_untyped(), "heading") && !recipe_uses_it {
                    ovr.show_heading_rule = true;
                }
                if syntax_mentions_ident(selector.to_untyped(), "outline") {
                    ovr.show_outline_entry_rule = true;
                }
                // `#show footnote.entry: set par(...)` — the entry element is
                // realized in the page's footnote area, so no StyleChain in
                // the pairs carries these values back to the footnote text.
                if syntax_mentions_ident(selector.to_untyped(), "footnote")
                    && syntax_mentions_ident(selector.to_untyped(), "entry")
                    && let ast::Expr::SetRule(set) = show.transform()
                    && matches!(set.target(), ast::Expr::Ident(i) if i.as_str() == "par")
                {
                    parse_footnote_entry_par_args(set.args(), ovr);
                }
            }
            return;
        }
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
        {
            match set.target() {
                ast::Expr::Ident(ident) => match ident.as_str() {
                    "page" => parse_page_args(set.args(), ovr),
                    "text" => parse_text_args(set.args(), ovr),
                    "par" => parse_par_args(set.args(), ovr),
                    "columns" => parse_columns_args(set.args(), ovr),
                    "align" => parse_align_args(set.args(), ovr),
                    _ => {}
                },
                // `#set par.line(...)` / `#set footnote.entry(...)` — the
                // target is a field access on the parent element.
                ast::Expr::FieldAccess(access) => {
                    match (access.target(), access.field().get().as_str()) {
                        (ast::Expr::Ident(i), "line") if i.as_str() == "par" => {
                            parse_par_line_args(set.args(), ovr);
                        }
                        (ast::Expr::Ident(i), "entry") if i.as_str() == "footnote" => {
                            parse_footnote_entry_args(set.args(), ovr);
                        }
                        _ => {}
                    }
                }
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

/// The page-setting fields whose mid-document change starts a new Typst
/// section (M6): column count and the four margins, on both the absolute
/// (twips) and em tracks. A numbering change is deliberately excluded —
/// it is a separate concern (T-08, Phase 2c), not a section break.
#[derive(Clone, PartialEq)]
struct PageSettingSnapshot {
    columns: Option<u32>,
    margin_top: Option<u32>,
    margin_bottom: Option<u32>,
    margin_left: Option<u32>,
    margin_right: Option<u32>,
    margin_top_em: Option<f64>,
    margin_bottom_em: Option<f64>,
    margin_left_em: Option<f64>,
    margin_right_em: Option<f64>,
}

impl PageSettingSnapshot {
    /// Snapshot the page-setting fields of a merged overrides value.
    fn from_overrides(ovr: &SourceStyleOverrides) -> Self {
        Self {
            columns: ovr.columns,
            margin_top: ovr.margin_top,
            margin_bottom: ovr.margin_bottom,
            margin_left: ovr.margin_left,
            margin_right: ovr.margin_right,
            margin_top_em: ovr.margin_top_em,
            margin_bottom_em: ovr.margin_bottom_em,
            margin_left_em: ovr.margin_left_em,
            margin_right_em: ovr.margin_right_em,
        }
    }
}

/// Merge one `#set page(...)` / `#page(...)` call's values into the running
/// settings, overwriting the fields it sets. Per side, a margin value sets
/// exactly one track (twips or em), so both tracks are copied together — a
/// switch from one track to the other clears the old one.
fn merge_page_settings(current: &mut SourceStyleOverrides, step: &SourceStyleOverrides) {
    if step.columns.is_some() {
        current.columns = step.columns;
    }
    macro_rules! merge_side {
        ($twips:ident, $em:ident) => {
            if step.$twips.is_some() || step.$em.is_some() {
                current.$twips = step.$twips;
                current.$em = step.$em;
            }
        };
    }
    merge_side!(margin_top, margin_top_em);
    merge_side!(margin_bottom, margin_bottom_em);
    merge_side!(margin_left, margin_left_em);
    merge_side!(margin_right, margin_right_em);
}

/// Mutable state for the page-setting-change walk: the running merged
/// settings, the previous snapshot, and the first change found.
struct PageSettingChangeState {
    current: SourceStyleOverrides,
    previous: Option<PageSettingSnapshot>,
    found: Option<typst_syntax::Span>,
}

impl PageSettingChangeState {
    fn new() -> Self {
        Self {
            current: SourceStyleOverrides::default(),
            previous: None,
            found: None,
        }
    }

    /// Fold one global `#set page(...)` / `#page(...)` call into the
    /// running settings; when the snapshot changes against the previous
    /// one, record the call's span as the first change.
    fn record_step(&mut self, args: typst_syntax::ast::Args<'_>, span: typst_syntax::Span) {
        let mut step = SourceStyleOverrides::default();
        parse_page_args(args, &mut step);
        merge_page_settings(&mut self.current, &step);
        let snapshot = PageSettingSnapshot::from_overrides(&self.current);
        match &self.previous {
            None => self.previous = Some(snapshot),
            Some(prev) => {
                // The walker stops at the first change (`found.is_some()`),
                // so `previous` never needs updating after a change; when
                // the snapshot is unchanged it already equals `previous`.
                if *prev != snapshot {
                    self.found = Some(span);
                }
            }
        }
    }
}

/// Detect a mid-document change of page columns or margins (M6).
///
/// Walks the source AST in document order, with the same scope rules as
/// [`collect_global_set_rules`] (show-rule recipes are not descended;
/// content blocks and non-template closures are local). Each global
/// `#set page(...)` / `#page(...)` call snapshots the running page
/// settings; the first snapshot only establishes the baseline. A later
/// snapshot that changes `columns` or any margin means Typst starts a new
/// section there — C1 keeps a single section, so the change is dropped.
/// Returns the span of the first changing rule, or `None` when the
/// settings never change. Only the main source is scanned; a change in an
/// imported file is not detected (same scope as
/// [`extract_source_style_overrides`]).
///
/// Takes the `Source`, not its text: spans are numbered against the source
/// only when it is constructed (`Source::new` numberizes the tree), while a
/// bare `typst_syntax::parse` leaves every node detached.
#[must_use]
pub fn detect_page_setting_change(
    source: &typst_syntax::Source,
    template_names: &[String],
) -> Option<typst_syntax::Span> {
    let mut state = PageSettingChangeState::new();
    collect_page_setting_changes(source.root(), template_names, true, &mut state);
    state.found
}

/// Recursive walker for [`detect_page_setting_change`]; mirrors
/// [`collect_global_set_rules`] scope handling.
fn collect_page_setting_changes(
    node: &typst_syntax::SyntaxNode,
    template_names: &[String],
    in_global_scope: bool,
    state: &mut PageSettingChangeState,
) {
    use typst_syntax::SyntaxKind;
    use typst_syntax::ast;

    if state.found.is_some() {
        return;
    }

    match node.kind() {
        // A show rule's recipe applies to specific elements, not globally — and
        // its own `set` rules are element-scoped. Don't descend.
        SyntaxKind::ShowRule => return,
        // A content block (`[...]` / `#block[...]`) introduces a local scope.
        SyntaxKind::ContentBlock => {
            for child in node.children() {
                collect_page_setting_changes(child, template_names, false, state);
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
                    collect_page_setting_changes(child, template_names, child_global, state);
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
            && ident.as_str() == "page"
        {
            state.record_step(set.args(), node.span());
        } else if node.kind() == SyntaxKind::FuncCall
            && let Some(call) = node.cast::<ast::FuncCall<'_>>()
            && matches!(call.callee(), ast::Expr::Ident(i) if i.as_str() == "page")
        {
            state.record_step(call.args(), node.span());
        }
    }

    for child in node.children() {
        collect_page_setting_changes(child, template_names, in_global_scope, state);
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
                    let decl = PageNumberingDecl::Pattern(s.get().to_string());
                    if let Some(prev) = &ovr.page_numbering
                        && *prev != decl
                    {
                        ovr.numbering_changed = true;
                    }
                    ovr.page_numbering = Some(decl);
                }
                // `numbering: none` disables numbering — no declaration.
                typst_syntax::ast::Expr::None(_) => {}
                // A function, a variable, … — declared but not mappable.
                _ => {
                    if let Some(prev) = &ovr.page_numbering
                        && *prev != PageNumberingDecl::Unmappable
                    {
                        ovr.numbering_changed = true;
                    }
                    ovr.page_numbering = Some(PageNumberingDecl::Unmappable);
                }
            },
            "foreground" | "background"
                if !matches!(named.expr(), typst_syntax::ast::Expr::None(_)) =>
            {
                ovr.page_furniture = true;
                if named.name().get().as_str() == "foreground" {
                    ovr.page_foreground = true;
                } else {
                    ovr.page_background = true;
                }
            }
            "header-ascent" => {
                if let typst_syntax::ast::Expr::Numeric(n) = named.expr() {
                    let (twips, em) = length_amount(n);
                    if twips.is_some() || em.is_some() {
                        ovr.header_twips = twips;
                        ovr.header_em = em;
                    }
                }
            }
            "footer-descent" => {
                if let typst_syntax::ast::Expr::Numeric(n) = named.expr() {
                    let (twips, em) = length_amount(n);
                    if twips.is_some() || em.is_some() {
                        ovr.footer_twips = twips;
                        ovr.footer_em = em;
                    }
                }
            }
            _ => {}
        }
    }
}

/// A declared length as absolute twips or deferred em (zero is kept — a zero
/// header ascent is a real value, unlike margins where zero means "unset").
fn length_amount(n: typst_syntax::ast::Numeric<'_>) -> (Option<u32>, Option<f64>) {
    let (value, unit) = n.get();
    if unit == typst_syntax::ast::Unit::Em {
        (None, Some(value))
    } else {
        (Some(numeric_to_twips(n)), None)
    }
}

fn parse_par_line_args(args: typst_syntax::ast::Args<'_>, ovr: &mut SourceStyleOverrides) {
    for arg in args.items() {
        let typst_syntax::ast::Arg::Named(named) = arg else {
            continue;
        };
        match named.name().as_str() {
            // A non-`none` numbering enables line numbering; `none` disables.
            "numbering" => {
                ovr.line_numbering =
                    Some(!matches!(named.expr(), typst_syntax::ast::Expr::None(_)));
            }
            "number-clearance" => {
                if let typst_syntax::ast::Expr::Numeric(n) = named.expr() {
                    let (twips, em) = length_amount(n);
                    ovr.line_number_distance_twips = twips;
                    ovr.line_number_distance_em = em;
                }
            }
            _ => {}
        }
    }
}

/// `#set columns(gutter: 12pt)` — also `#set columns(N, gutter: …)`; the
/// count itself is ignored here (it is a `#columns(N)` call property, not a
/// section column count).
fn parse_columns_args(args: typst_syntax::ast::Args<'_>, ovr: &mut SourceStyleOverrides) {
    for arg in args.items() {
        if let typst_syntax::ast::Arg::Named(named) = arg
            && named.name().as_str() == "gutter"
            && let typst_syntax::ast::Expr::Numeric(n) = named.expr()
        {
            let (twips, em) = length_amount(n);
            ovr.column_gutter_twips = twips;
            ovr.column_gutter_em = em;
        }
    }
}

/// Document-global `#set align(…)` — only the vertical component maps to a
/// section `w:vAlign`; `horizon` centers, `bottom`/`top` pin the content.
fn parse_align_args(args: typst_syntax::ast::Args<'_>, ovr: &mut SourceStyleOverrides) {
    for arg in args.items() {
        let typst_syntax::ast::Arg::Pos(expr) = arg else {
            continue;
        };
        if let Some(align) = vertical_align_of(expr) {
            ovr.vertical_align = Some(align);
        }
    }
}

/// Find `horizon`/`bottom`/`top` in an alignment expression (`center +
/// horizon` parses as a binary `+` of two idents).
fn vertical_align_of(expr: typst_syntax::ast::Expr<'_>) -> Option<VerticalAlign> {
    use typst_syntax::ast::Expr;
    match expr {
        Expr::Ident(ident) => match ident.get().as_str() {
            "horizon" => Some(VerticalAlign::Center),
            "bottom" => Some(VerticalAlign::Bottom),
            "top" => Some(VerticalAlign::Top),
            _ => None,
        },
        Expr::Binary(binary) => {
            let lhs = vertical_align_of(binary.lhs());
            let rhs = vertical_align_of(binary.rhs());
            lhs.or(rhs)
        }
        _ => None,
    }
}

/// `#set footnote.entry(indent:/gap:/clearance:)` and a literal
/// `line(stroke: N)` separator (thickness → `w:pBdr w:sz` eighths).
fn parse_footnote_entry_args(args: typst_syntax::ast::Args<'_>, ovr: &mut SourceStyleOverrides) {
    use typst_syntax::ast::Expr;
    for arg in args.items() {
        let typst_syntax::ast::Arg::Named(named) = arg else {
            continue;
        };
        match named.name().as_str() {
            "clearance" => {
                if let Expr::Numeric(n) = named.expr() {
                    let (twips, em) = length_amount(n);
                    ovr.footnote_clearance_twips = twips;
                    ovr.footnote_clearance_em = em;
                }
            }
            "gap" => {
                if let Expr::Numeric(n) = named.expr() {
                    let (twips, em) = length_amount(n);
                    ovr.footnote_gap_twips = twips;
                    ovr.footnote_gap_em = em;
                }
            }
            "indent" => {
                if let Expr::Numeric(n) = named.expr() {
                    let (twips, em) = length_amount(n);
                    ovr.footnote_indent_twips = twips;
                    ovr.footnote_indent_em = em;
                }
            }
            "separator" => {
                if let Expr::FuncCall(call) = named.expr()
                    && matches!(call.callee(), Expr::Ident(i) if i.as_str() == "line")
                {
                    for sep_arg in call.args().items() {
                        if let typst_syntax::ast::Arg::Named(sep_named) = sep_arg
                            && sep_named.name().as_str() == "stroke"
                            && let Expr::Numeric(n) = sep_named.expr()
                        {
                            let (value, unit) = n.get();
                            if unit == typst_syntax::ast::Unit::Pt && value > 0.0 {
                                #[allow(
                                    clippy::cast_possible_truncation,
                                    clippy::cast_sign_loss,
                                    reason = "positive point size to eighths"
                                )]
                                {
                                    ovr.footnote_separator_stroke_eighths =
                                        Some((value * 8.0).round() as u32);
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// The `par` set inside a `#show footnote.entry:` recipe: `first-line-indent`
/// fills the same `footnote.entry(indent:)` field (both mean "the entry's
/// first-line indent"); `justify` pins the entry's `w:jc`.
fn parse_footnote_entry_par_args(
    args: typst_syntax::ast::Args<'_>,
    ovr: &mut SourceStyleOverrides,
) {
    use typst_syntax::ast::{Arg, DictItem, Expr};
    for arg in args.items() {
        let Arg::Named(named) = arg else {
            continue;
        };
        match named.name().as_str() {
            "first-line-indent" => match named.expr() {
                Expr::Numeric(n) => {
                    let (twips, em) = length_amount(n);
                    ovr.footnote_indent_twips = twips;
                    ovr.footnote_indent_em = em;
                }
                Expr::Dict(dict) => {
                    for item in dict.items() {
                        if let DictItem::Named(entry) = item
                            && entry.name().as_str() == "amount"
                            && let Expr::Numeric(n) = entry.expr()
                        {
                            let (twips, em) = length_amount(n);
                            ovr.footnote_indent_twips = twips;
                            ovr.footnote_indent_em = em;
                        }
                    }
                }
                _ => {}
            },
            "justify" => {
                if let Expr::Bool(b) = named.expr() {
                    ovr.footnote_par_justify = Some(b.get());
                }
            }
            _ => {}
        }
    }
}

/// A `#set page(margin: …)` value as declared: absolute twips, or em to
/// resolve later against the body size. An explicit zero is a declared value
/// (`0pt` → `Some(0)`), distinct from an unsupported relative unit (`%`,
/// `fr`), which stays unset.
fn margin_amount(n: typst_syntax::ast::Numeric<'_>) -> (Option<u32>, Option<f64>) {
    let (value, unit) = n.get();
    match unit {
        typst_syntax::ast::Unit::Em => (None, Some(value)),
        typst_syntax::ast::Unit::Pt
        | typst_syntax::ast::Unit::Cm
        | typst_syntax::ast::Unit::Mm
        | typst_syntax::ast::Unit::In => (Some(numeric_to_twips(n)), None),
        _ => (None, None),
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
                match entry.name().as_str() {
                    "inside" | "outside" => ovr.margins_mirrored = true,
                    _ => {}
                }
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
            if left_twips != right_twips || left_em != right_em {
                ovr.one_sided_asymmetric_margins = true;
            }
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
                "hyphenate" => {
                    if ovr.text_hyphenate.is_none()
                        && let typst_syntax::ast::Expr::Bool(b) = named.expr()
                    {
                        ovr.text_hyphenate = Some(b.get());
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

pub(super) fn for_each_declared_font(
    node: &typst_syntax::SyntaxNode,
    visit: &mut impl FnMut(String),
) {
    use typst_syntax::ast::{Arg, Expr};

    let args = match node.cast::<Expr<'_>>() {
        Some(Expr::SetRule(rule)) if is_text_function(rule.target()) => Some(rule.args()),
        Some(Expr::FuncCall(call))
            if is_text_function(call.callee())
                || matches!(ungroup(call.callee()), Expr::FieldAccess(access)
                    if access.field().as_str() == "with" && is_text_function(access.target())) =>
        {
            Some(call.args())
        }
        _ => None,
    };
    for arg in args.into_iter().flat_map(typst_syntax::ast::Args::items) {
        if let Arg::Named(named) = arg
            && named.name().as_str() == "font"
        {
            visit_font_literals(named.expr(), visit);
        }
    }
    for child in node.children() {
        for_each_declared_font(child, visit);
    }
}

fn visit_font_literals(expr: typst_syntax::ast::Expr<'_>, visit: &mut impl FnMut(String)) {
    use typst_syntax::ast::{ArrayItem, Expr};
    match ungroup(expr) {
        Expr::Array(array) => {
            for item in array.items() {
                if let ArrayItem::Pos(expr) = item
                    && let Some(name) = font_family_literal(expr)
                {
                    visit(name);
                }
            }
        }
        expr => {
            if let Some(name) = font_family_literal(expr) {
                visit(name);
            }
        }
    }
}

fn is_text_function(expr: typst_syntax::ast::Expr<'_>) -> bool {
    use typst_syntax::ast::Expr;
    match ungroup(expr) {
        Expr::Ident(ident) => ident.as_str() == "text",
        Expr::FieldAccess(access) => {
            access.field().as_str() == "text"
                && matches!(access.target(), Expr::Ident(ident) if ident.as_str() == "std")
        }
        _ => false,
    }
}

fn ungroup(mut expr: typst_syntax::ast::Expr<'_>) -> typst_syntax::ast::Expr<'_> {
    while let typst_syntax::ast::Expr::Parenthesized(group) = expr {
        expr = group.expr();
    }
    expr
}

fn font_family_literal(expr: typst_syntax::ast::Expr<'_>) -> Option<String> {
    use typst_syntax::ast::{DictItem, Expr};
    let expr = match ungroup(expr) {
        Expr::Dict(dict) => dict.items().find_map(|item| match item {
            DictItem::Named(named) if named.name().as_str() == "name" => Some(named.expr()),
            DictItem::Keyed(keyed)
                if matches!(keyed.key(), Expr::Str(key) if key.get() == "name") =>
            {
                Some(keyed.expr())
            }
            _ => None,
        })?,
        expr => expr,
    };
    match ungroup(expr) {
        Expr::Str(name) => Some(name.get().to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::units::pt_to_twips;
    use super::*;
    use crate::diag::span_line_col;
    use typst_syntax::{RootedPath, Source, VirtualPath, VirtualRoot};

    fn test_source(text: &str) -> Source {
        let vpath = VirtualPath::new("/m6-test.typ").unwrap();
        let id = RootedPath::new(VirtualRoot::Project, vpath).intern();
        Source::new(id, text.to_string())
    }

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
    fn page_setting_change_detects_columns_change() {
        let source = test_source("#set page(columns: 1)\nBody.\n\n#set page(columns: 2)\nMore.\n");
        assert!(detect_page_setting_change(&source, &[]).is_some());
    }

    #[test]
    fn page_setting_change_span_points_at_changing_rule() {
        let source = test_source("#set page(columns: 1)\nBody.\n\n#set page(columns: 2)\nMore.\n");
        let span = detect_page_setting_change(&source, &[]).expect("a change is detected");
        // Column 2, not 1: the `#` occupies column 1; the rule's span
        // starts at the `set` keyword.
        assert_eq!(span_line_col(&source, span), Some((4, 2)));
    }

    #[test]
    fn page_setting_change_detects_margin_change() {
        let source =
            test_source("#set page(margin: 2cm)\nBody.\n\n#set page(margin: 1cm)\nMore.\n");
        assert!(detect_page_setting_change(&source, &[]).is_some());
    }

    #[test]
    fn page_setting_change_detects_page_call_change() {
        let source = test_source("#set page(columns: 1)\nBody.\n\n#page(columns: 2)[More.]\n");
        assert!(detect_page_setting_change(&source, &[]).is_some());
    }

    #[test]
    fn page_setting_change_ignores_repeated_same_value() {
        let source = test_source("#set page(columns: 2)\nBody.\n\n#set page(columns: 2)\nMore.\n");
        assert!(detect_page_setting_change(&source, &[]).is_none());
    }

    #[test]
    fn page_setting_change_ignores_single_set() {
        let source = test_source("#set page(columns: 2)\nBody.\n");
        assert!(detect_page_setting_change(&source, &[]).is_none());
    }

    #[test]
    fn page_setting_change_ignores_numbering_only() {
        // A numbering change is T-08, not a section break.
        let source = test_source(
            "#set page(numbering: \"1\")\nBody.\n\n#set page(numbering: \"i\")\nMore.\n",
        );
        assert!(detect_page_setting_change(&source, &[]).is_none());
    }

    #[test]
    fn page_setting_change_ignores_scoped_set_in_block() {
        let source =
            test_source("#set page(columns: 1)\nBody.\n\n#block[#set page(columns: 2)]\nMore.\n");
        assert!(detect_page_setting_change(&source, &[]).is_none());
    }

    #[test]
    fn page_setting_change_tracks_em_margin_switch() {
        // Switching margin tracks (twips to em) is a change even when the
        // rendered length could coincide.
        let source =
            test_source("#set page(margin: 2cm)\nBody.\n\n#set page(margin: 2em)\nMore.\n");
        assert!(detect_page_setting_change(&source, &[]).is_some());
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

#[cfg(test)]
mod transform_text_tests {
    use super::transform_bodies_text;

    #[test]
    fn set_page_furniture_is_not_body_text() {
        let src = "#set page(foreground: place(center + horizon, rotate(45deg)[DRAFT]))

Body.
";
        assert!(transform_bodies_text(src).is_empty());
    }

    #[test]
    fn page_call_furniture_is_not_body_text() {
        let src = "#page(header: [H], footer: [F])[#move(dy: 2pt)[Shifted]]";
        assert_eq!(transform_bodies_text(src), vec!["Shifted"]);
    }

    #[test]
    fn body_transforms_still_recover() {
        let src = "#move(dy: 1pt)[Up] and #rotate(10deg)[Tilt]";
        assert_eq!(transform_bodies_text(src), vec!["Up", "Tilt"]);
    }
}
