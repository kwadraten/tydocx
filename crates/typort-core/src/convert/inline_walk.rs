use std::collections::HashMap;

use typst::World;

use crate::compat::{CompatMode, Degradation, DegradationAction};

use typst_library::visualize::ImageElem;

use super::{
    Alignment, CiteElem, CiteGroup, EmphElem, EquationElem, HighlightElem, HighlightStyle,
    HtmlDocument, HtmlElement, HtmlNode, InlineFmt, LinkElem, Paragraph, RefElem, Run, StrongElem,
    Tag, WalkCtx, collect_flat_text, content_at_location, drain_text_runs, element_at_location,
    emit_block_equation, find_img_src, find_tag_end, first_biblioref_href, footnote,
    get_attr_value, has_attr_value, highlight_style, image, inline, is_block_equation, page,
    run_with_span, sanitize_anchor, subtree_has_element, tag_name, tag_span,
};

/// The behavior needed by a caller of the shared HTML inline collector.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum InlinePurpose {
    Paragraph,
    Generic,
    FormattedRun,
    Footnote,
}

/// Options for the single HTML inline collector.
#[derive(Clone, Copy)]
pub(super) struct InlineOptions<'a> {
    fmt: InlineFmt,
    html_doc: Option<&'a HtmlDocument>,
    purpose: InlinePurpose,
    /// Display sizes of rendered images, keyed by content hash. Only
    /// populated for footnote walks — body walks read sizes through the
    /// shared `WalkCtx` instead.
    image_sizes: Option<&'a HashMap<u64, (u64, u64)>>,
}

impl<'a> InlineOptions<'a> {
    pub(super) fn paragraph() -> Self {
        Self {
            fmt: InlineFmt::default(),
            html_doc: None,
            purpose: InlinePurpose::Paragraph,
            image_sizes: None,
        }
    }

    pub(super) fn generic(fmt: InlineFmt, html_doc: Option<&'a HtmlDocument>) -> Self {
        Self {
            fmt,
            html_doc,
            purpose: InlinePurpose::Generic,
            image_sizes: None,
        }
    }

    pub(super) fn formatted_run() -> Self {
        Self {
            fmt: InlineFmt::default(),
            html_doc: None,
            purpose: InlinePurpose::FormattedRun,
            image_sizes: None,
        }
    }

    pub(super) fn footnote(fmt: InlineFmt, image_sizes: &'a HashMap<u64, (u64, u64)>) -> Self {
        Self {
            fmt,
            html_doc: None,
            purpose: InlinePurpose::Footnote,
            image_sizes: Some(image_sizes),
        }
    }

    pub(super) fn with_fmt(self, fmt: InlineFmt) -> Self {
        Self { fmt, ..self }
    }
}

/// Collect inline elements using the exact behavior selected by `options`.
pub(super) fn collect_inlines(
    children: &[HtmlNode],
    para: &mut Paragraph,
    mut ctx: Option<&mut WalkCtx<'_>>,
    options: InlineOptions<'_>,
) {
    let mut i = 0;
    while i < children.len() {
        match &children[i] {
            HtmlNode::Text(text, span) => {
                if !text.is_empty() {
                    let mut run = if options.purpose == InlinePurpose::Footnote {
                        Run::new(text.as_str())
                    } else {
                        run_with_span(text.as_str(), *span)
                    };
                    options.fmt.apply_to(&mut run);
                    para.push_run(run);
                }
            }
            HtmlNode::Tag(tag) => {
                if let Tag::Start(..) = tag {
                    match options.purpose {
                        InlinePurpose::Paragraph => {
                            if let Some(ctx) = ctx.as_deref_mut() {
                                i = handle_inline_tag(tag, children, i, ctx, para);
                            }
                        }
                        InlinePurpose::Generic => {
                            collect_generic_inline_tag(children, i, tag, para, options);
                        }
                        InlinePurpose::FormattedRun => {
                            i = collect_formatted_inline_tag(children, i, tag, para, options);
                        }
                        InlinePurpose::Footnote => collect_footnote_inline_tag(tag, para),
                    }
                }
            }
            HtmlNode::Element(elem) => {
                collect_inline_element(elem, para, ctx.as_deref_mut(), options);
            }
            HtmlNode::Frame(frame) => {
                if matches!(
                    options.purpose,
                    InlinePurpose::Paragraph | InlinePurpose::Generic
                ) {
                    // Layouted-opaque inline content (e.g. a boxed drawing as a
                    // figure body): rasterize in place as an inline image.
                    if let Some(img) = image::rasterize_html_frame(frame) {
                        // U21 (C1 only, Paragraph purpose only — the Generic
                        // path carries no degradation sink): the frame's
                        // vector/text content is no longer editable. The frame
                        // carries no span or page, so the degradation is
                        // span-less without a page hint.
                        if options.purpose == InlinePurpose::Paragraph
                            && let Some(ctx) = ctx.as_deref_mut()
                            && ctx.compat == CompatMode::C1
                        {
                            ctx.degradations.push(Degradation::new(
                                None,
                                "html-frame",
                                DegradationAction::Approximated,
                                "HTML frame rasterized to PNG; content is no longer editable text",
                            ));
                        }
                        para.add_image(img);
                    }
                }
            }
        }
        i += 1;
    }
}

/// Whether an inline element with this tag name and `role` attribute is
/// skipped (not descended into) for the given collection purpose.
///
/// Generic and Footnote both skip `MathML` — `<math>` is the rendered echo of
/// an equation the Tag walker already emitted as OMML — and differ only in
/// which footnote-marker role they suppress (`doc-noteref` vs `doc-backlink`).
/// `FormattedRun` deliberately skips nothing: it descends into `MathML`, and
/// changing that output belongs to #12.
fn skip_inline_element(purpose: InlinePurpose, tag: &str, role: Option<&str>) -> bool {
    match purpose {
        InlinePurpose::Generic => tag == "math" || role == Some("doc-noteref"),
        InlinePurpose::Footnote => tag == "math" || role == Some("doc-backlink"),
        InlinePurpose::FormattedRun | InlinePurpose::Paragraph => false,
    }
}

pub(super) fn collect_inline_element(
    elem: &HtmlElement,
    para: &mut Paragraph,
    ctx: Option<&mut WalkCtx<'_>>,
    options: InlineOptions<'_>,
) {
    match options.purpose {
        InlinePurpose::Paragraph => {
            if let Some(ctx) = ctx {
                handle_inline_html_element(elem, ctx, para);
            }
        }
        InlinePurpose::Generic | InlinePurpose::FormattedRun | InlinePurpose::Footnote => {
            // The three non-Paragraph purposes share one descend shape: skip
            // what the purpose suppresses, then recurse with the tag's
            // formatting. Only Generic treats `<br>` as a line break.
            let tag = tag_name(elem);
            if skip_inline_element(
                options.purpose,
                &tag,
                get_attr_value(elem, "role").as_deref(),
            ) {
                return;
            }
            if options.purpose == InlinePurpose::Generic && tag == "br" {
                // A forced line break (`#linebreak()`) — without this it falls
                // through to the child recursion below (a `<br>` has no children)
                // and the break is silently dropped, gluing the surrounding words.
                para.push_run(Run::line_break());
                return;
            }
            if options.purpose == InlinePurpose::Footnote {
                // A hyperlink in a footnote keeps its target: only the
                // display runs are collected from its children.
                if tag == "a"
                    && let Some(href) = get_attr_value(elem, "href")
                {
                    let mut tmp = collect_children_generic(&elem.children, options.fmt, None);
                    let runs = drain_text_runs(&mut tmp);
                    if !runs.is_empty() {
                        para.add_hyperlink(href, runs);
                    }
                    return;
                }
                // An image in a footnote embeds like a body image: the
                // `<img>` src data-URL carries the bytes and the display
                // size comes from the paged render, falling back to the
                // image's intrinsic size when no rendered size is known.
                if tag == "img"
                    && let Some(src) = get_attr_value(elem, "src")
                {
                    let empty = HashMap::new();
                    let sizes = options.image_sizes.unwrap_or(&empty);
                    if let Some(img) = image::image_data_from_src(&src, sizes) {
                        para.add_image(img);
                    }
                    return;
                }
            }
            let fmt = options.fmt.for_tag(&tag);
            collect_inlines(&elem.children, para, None, options.with_fmt(fmt));
        }
    }
}

/// Collect an element's children into a fresh paragraph with the given
/// formatting, using the Generic collector. The Generic collector is
/// element-aware (equations resolve to OMML when `html_doc` is present), so
/// this preserves inline math that `drain_text_runs` would drop.
fn collect_children_generic(
    children: &[HtmlNode],
    fmt: InlineFmt,
    html_doc: Option<&HtmlDocument>,
) -> Paragraph {
    let mut tmp = Paragraph::new();
    collect_inlines(
        children,
        &mut tmp,
        None,
        InlineOptions::generic(fmt, html_doc),
    );
    tmp
}

pub(super) fn collect_generic_inline_tag(
    children: &[HtmlNode],
    i: usize,
    tag: &Tag,
    para: &mut Paragraph,
    options: InlineOptions<'_>,
) {
    let Tag::Start(content, _) = tag else { return };
    match content.elem().name() {
        "footnote" => {
            if let Some(id) = footnote::find_footnote_id_in_range(&children[i..]) {
                para.add_footnote_ref(id + 1);
            }
        }
        "equation" => {
            if let Some(html_doc) = options.html_doc
                && let Some(content) = content_at_location(html_doc, tag.location())
            {
                para.add_math(typort_math::equation_to_omml(&content));
            }
        }
        _ => {}
    }
}

pub(super) fn collect_formatted_inline_tag(
    children: &[HtmlNode],
    i: usize,
    tag: &Tag,
    para: &mut Paragraph,
    options: InlineOptions<'_>,
) -> usize {
    let Tag::Start(content, _) = tag else {
        return i;
    };
    let end = find_tag_end(children, i, tag.location());
    let fmt = match content.elem().name() {
        "strong" | "emph" => options.fmt.for_tag(content.elem().name()),
        "raw" => InlineFmt {
            monospace: true,
            ..options.fmt
        },
        _ => options.fmt,
    };
    collect_inlines(&children[i + 1..end], para, None, options.with_fmt(fmt));
    end
}

pub(super) fn collect_footnote_inline_tag(tag: &Tag, para: &mut Paragraph) {
    if let Tag::Start(content, _) = tag
        && content.elem().name() == "equation"
    {
        para.add_math(typort_math::equation_to_omml(content));
    }
}

/// Whether `content` (recursively) contains an `EquationElem`.
///
/// Used to decide whether an emphasis/strong body must be descended through the
/// equation-aware DOM walk (typst 0.15 nests inline math inside emphasis) rather
/// than the run-only `inline::extract_runs` fast path.
pub(super) fn content_has_equation(content: &typst::foundations::Content) -> bool {
    use typst_library::foundations::SequenceElem;
    use typst_library::model::{EmphElem, StrongElem};

    if content.to_packed::<EquationElem>().is_some() {
        true
    } else if let Some(seq) = content.to_packed::<SequenceElem>() {
        seq.children.iter().any(content_has_equation)
    } else if let Some(s) = content.to_packed::<StrongElem>() {
        content_has_equation(&s.body)
    } else if let Some(e) = content.to_packed::<EmphElem>() {
        content_has_equation(&e.body)
    } else {
        false
    }
}

/// Process a single inline `Tag::Start` within a paragraph.
/// Returns the new index (pointing at the matching End tag).
pub(super) fn handle_inline_tag(
    tag: &Tag,
    children: &[HtmlNode],
    i: usize,
    ctx: &mut WalkCtx,
    para: &mut Paragraph,
) -> usize {
    let Tag::Start(content, _) = tag else {
        return i;
    };
    match content.elem().name() {
        "strong" | "emph" => handle_emphasis_tag(tag, children, i, ctx, para),
        "equation" => handle_inline_equation_tag(tag, children, i, ctx, para),
        "footnote" => handle_inline_footnote_tag(tag, children, i, ctx, para),
        "image" => handle_inline_image_tag(tag, children, i, ctx, para),
        "ref" => handle_inline_ref_tag(tag, children, i, ctx, para),
        "link" => handle_inline_link_tag(tag, children, i, ctx, para),
        "super" | "sub" | "raw" | "underline" | "strike" | "highlight" | "overline"
        | "smallcaps" => handle_inline_format_tag(tag, children, i, ctx, para),
        "cite-group" => handle_inline_cite_group_tag(tag, children, i, ctx, para),
        "par" => handle_nested_inline_tag(tag, children, i, ctx, para),
        "context" => handle_inline_context_tag(tag, children, i, ctx, para),
        "caption" => handle_inline_caption_tag(tag, children, i, ctx, para),
        _ => find_tag_end(children, i, tag.location()),
    }
}

pub(super) fn handle_emphasis_tag(
    tag: &Tag,
    children: &[HtmlNode],
    i: usize,
    ctx: &mut WalkCtx,
    para: &mut Paragraph,
) -> usize {
    let end = find_tag_end(children, i, tag.location());
    let Tag::Start(content, _) = tag else {
        return end;
    };
    let (body, fmt) = if content.elem().name() == "strong" {
        (
            element_at_location::<StrongElem>(ctx.html_doc, tag.location())
                .map(|strong| strong.body.clone()),
            InlineFmt::bold(),
        )
    } else {
        (
            element_at_location::<EmphElem>(ctx.html_doc, tag.location())
                .map(|emph| emph.body.clone()),
            InlineFmt::italic(),
        )
    };

    // `extract_runs` only carries text. Descend through the equation-aware HTML
    // collector when emphasis wraps math; retain the cheaper Content walk otherwise.
    if body.as_ref().is_some_and(content_has_equation) {
        let mut tmp = Paragraph::new();
        collect_inlines(
            &children[i + 1..end],
            &mut tmp,
            None,
            InlineOptions::generic(fmt, Some(ctx.html_doc)),
        );
        para.inlines.append(&mut tmp.inlines);
    } else if let Some(body) = body {
        for mut run in inline::extract_runs(&body) {
            // The runs already carry formatting from nested elements
            // (smallcaps/super/sub); merge the emphasis flag instead of
            // overwriting them (see #20).
            fmt.or_into(&mut run);
            para.push_run(run);
        }
    }
    end
}

pub(super) fn handle_inline_equation_tag(
    tag: &Tag,
    children: &[HtmlNode],
    i: usize,
    ctx: &mut WalkCtx,
    para: &mut Paragraph,
) -> usize {
    if let Some(content) = content_at_location(ctx.html_doc, tag.location()) {
        if is_block_equation(&content) {
            if !para.inlines.is_empty() {
                ctx.doc.add_paragraph(std::mem::take(para));
            }
            emit_block_equation(&content, ctx);
        } else {
            para.add_math(typort_math::equation_to_omml(&content));
        }
    }
    find_tag_end(children, i, tag.location())
}

pub(super) fn handle_inline_footnote_tag(
    tag: &Tag,
    children: &[HtmlNode],
    i: usize,
    ctx: &mut WalkCtx,
    para: &mut Paragraph,
) -> usize {
    if let Some(id) = footnote::find_footnote_id_in_range(&children[i..]) {
        para.add_footnote_ref(id + 1);
    }
    // U26 (C1 only): a non-decimal, non-circled numbering has no OOXML
    // equivalent. The marker text is the rendered numbering, so a `#set
    // footnote(numbering: …)` rule is seen here even though the element
    // field cannot see it. Span-less: the §4.1 dedup collapses a
    // document's numbering to its first occurrence plus a count.
    if ctx.compat == CompatMode::C1
        && let Some(marker) = footnote::find_footnote_marker_text(&children[i..])
        && !footnote::footnote_marker_representable(&marker)
    {
        ctx.degradations.push(Degradation::new(
            None,
            "footnote-numbering",
            DegradationAction::Literalized,
            format!("footnote numbering {marker:?} is not representable; references using it are dropped"),
        ));
    }
    find_tag_end(children, i, tag.location())
}

pub(super) fn handle_inline_image_tag(
    tag: &Tag,
    children: &[HtmlNode],
    i: usize,
    ctx: &mut WalkCtx,
    para: &mut Paragraph,
) -> usize {
    let end = find_tag_end(children, i, tag.location());
    let src = find_img_src(&children[i..=end]);
    if let Some(src) = &src
        && let Some(img_data) = image::image_data_from_src(src, ctx.image_sizes)
    {
        para.add_image(img_data);
    }
    if ctx.compat == CompatMode::C1 {
        // L4: an SVG image is rasterized to PNG; the vector aspect is dropped.
        if let Some(src) = &src
            && image::is_vector_image_src(src)
        {
            ctx.degradations.push(Degradation::new(
                tag_span(tag),
                "vector-image",
                DegradationAction::Approximated,
                "SVG image rasterized to PNG; vector aspect dropped",
            ));
        }
        // U19: a non-cover fit and a non-empty alt text are dropped.
        if let Some(image_elem) = element_at_location::<ImageElem>(ctx.html_doc, tag.location()) {
            image::report_image_aspects(&image_elem, tag_span(tag), ctx.degradations);
        }
    }
    end
}

pub(super) fn handle_inline_ref_tag(
    tag: &Tag,
    children: &[HtmlNode],
    i: usize,
    ctx: &WalkCtx,
    para: &mut Paragraph,
) -> usize {
    let end = find_tag_end(children, i, tag.location());
    if let Some(reference) = element_at_location::<RefElem>(ctx.html_doc, tag.location()) {
        let target = reference.target.resolve().to_string();
        let display = collect_flat_text(&children[i + 1..end]);
        if ctx.bib_keys.contains(&target) {
            let mut run = Run::new(&display);
            run.superscript = subtree_has_element(&children[i + 1..end], "sup");
            match first_biblioref_href(&children[i + 1..end]) {
                Some(href) => {
                    para.add_internal_link(
                        sanitize_anchor(href.trim_start_matches('#')),
                        vec![run],
                    );
                }
                None => para.push_run(run),
            }
        } else {
            para.add_field_ref(target, display);
        }
    }
    end
}

pub(super) fn handle_inline_link_tag(
    tag: &Tag,
    children: &[HtmlNode],
    i: usize,
    ctx: &WalkCtx,
    para: &mut Paragraph,
) -> usize {
    let end = find_tag_end(children, i, tag.location());
    let Some(link) = element_at_location::<LinkElem>(ctx.html_doc, tag.location()) else {
        return end;
    };
    let typst_library::model::LinkTarget::Dest(typst_library::model::Destination::Url(url)) =
        &link.dest
    else {
        return end;
    };
    let mut display = Paragraph::new();
    collect_inlines(
        &children[i + 1..end],
        &mut display,
        None,
        InlineOptions::formatted_run(),
    );
    let runs = drain_text_runs(&mut display);
    if !runs.is_empty() {
        para.add_hyperlink(url.to_string(), runs);
    }
    end
}

pub(super) fn handle_inline_format_tag(
    tag: &Tag,
    children: &[HtmlNode],
    i: usize,
    ctx: &mut WalkCtx,
    para: &mut Paragraph,
) -> usize {
    let end = find_tag_end(children, i, tag.location());
    let text = collect_flat_text(&children[i + 1..end]);
    if !text.is_empty() {
        let mut run = Run::new(&text);
        if let Tag::Start(content, _) = tag {
            let name = content.elem().name();
            if name == "highlight" {
                apply_highlight_fill(ctx, tag, &mut run);
            } else if name == "overline" && ctx.compat == CompatMode::C1 {
                // M9: C1 drops the overline aspect instead of silently
                // rewriting it as underline; the text survives unadorned.
                ctx.degradations.push(Degradation::new(
                    tag_span(tag),
                    "overline",
                    DegradationAction::Skipped,
                    "overline dropped; text kept without decoration",
                ));
            } else {
                apply_inline_format(name, &mut run);
            }
        }
        para.push_run(run);
    }
    end
}

/// Resolve a highlight's declared fill and write its OOXML form onto the
/// run. Falls back to the historical yellow when the element can't be
/// resolved at this location, or when the fill is unset (Typst's default).
fn apply_highlight_fill(ctx: &WalkCtx, tag: &Tag, run: &mut Run) {
    let Some(highlight) = element_at_location::<HighlightElem>(ctx.html_doc, tag.location()) else {
        run.highlight_color = Some("yellow".into());
        return;
    };
    // `fill` is a settable field: `as_option` reads the value stored on the
    // realized element. An explicitly set `none` means no highlight; an
    // unset fill means Typst's default yellow — the same pattern
    // `is_block_equation` uses for `block`.
    let style = match highlight.fill.as_option() {
        Some(Some(paint)) => highlight_style(Some(paint)),
        Some(None) => HighlightStyle::None,
        None => HighlightStyle::Named("yellow"),
    };
    match style {
        HighlightStyle::Named(name) => run.highlight_color = Some(name.into()),
        HighlightStyle::Shading(hex) => run.shading = Some(hex),
        HighlightStyle::None => {}
    }
}

pub(super) fn handle_inline_cite_group_tag(
    tag: &Tag,
    children: &[HtmlNode],
    i: usize,
    ctx: &WalkCtx,
    para: &mut Paragraph,
) -> usize {
    let end = find_tag_end(children, i, tag.location());
    if let Some(cite_group) = element_at_location::<CiteGroup>(ctx.html_doc, tag.location()) {
        let keys = cite_group
            .children
            .iter()
            .filter_map(|cite| cite.to_packed::<CiteElem>())
            .map(|cite| cite.key.resolve().to_string())
            .collect::<Vec<_>>();
        let display = collect_flat_text(&children[i + 1..end]);
        if !keys.is_empty() && !display.is_empty() {
            para.add_citation(keys, display);
        }
    }
    end
}

/// A `context` Tag wraps a `context ...` expression. Most of the time it is
/// transparent — the rendered text is the expression's value, collected as
/// usual — but `counter(page).at(<label>)` is a page reference to a labelled
/// location: emit a PAGEREF field to that label's bookmark instead, with the
/// rendered page number as the cached display text. The label is recovered
/// from the source AST (the Tag's span points at the expression); anything
/// the resolver can't identify falls back to the plain-text collection.
pub(super) fn handle_inline_context_tag(
    tag: &Tag,
    children: &[HtmlNode],
    i: usize,
    ctx: &mut WalkCtx,
    para: &mut Paragraph,
) -> usize {
    let end = find_tag_end(children, i, tag.location());
    let Tag::Start(content, _) = tag else {
        return end;
    };
    let span = content.span();
    let label = span
        .id()
        .and_then(|id| ctx.world.source(id).ok())
        .and_then(|source| page::counter_page_label(&source, span));
    match label {
        Some(label) => {
            let display = collect_flat_text(&children[i + 1..end]);
            para.add_field_pageref(label, display);
        }
        None => {
            collect_inlines(
                &children[i + 1..end],
                para,
                Some(ctx),
                InlineOptions::paragraph(),
            );
        }
    }
    end
}

pub(super) fn handle_nested_inline_tag(
    tag: &Tag,
    children: &[HtmlNode],
    i: usize,
    ctx: &mut WalkCtx,
    para: &mut Paragraph,
) -> usize {
    let end = find_tag_end(children, i, tag.location());
    collect_inlines(
        &children[i + 1..end],
        para,
        Some(ctx),
        InlineOptions::paragraph(),
    );
    end
}

pub(super) fn handle_inline_caption_tag(
    tag: &Tag,
    children: &[HtmlNode],
    i: usize,
    ctx: &mut WalkCtx,
    para: &mut Paragraph,
) -> usize {
    let end = find_tag_end(children, i, tag.location());
    let text = collect_flat_text(&children[i + 1..end]);
    if !text.trim().is_empty() {
        if !para.inlines.is_empty() {
            ctx.doc.add_paragraph(std::mem::take(para));
        }
        let mut caption = Paragraph::new();
        caption.alignment = Some(Alignment::Center);
        caption.push_run(Run::new(text.trim()));
        ctx.doc.add_paragraph(caption);
    }
    end
}

/// Apply the appropriate formatting flag to a `Run` based on the inline tag name.
/// Highlight is not handled here: its declared fill is resolved with the
/// element in `apply_highlight_fill`.
pub(super) fn apply_inline_format(tag_name: &str, run: &mut Run) {
    match tag_name {
        "super" => run.superscript = true,
        "sub" => run.subscript = true,
        "raw" => run.monospace = true,
        "underline" | "overline" => run.underline = true,
        "strike" => run.strikethrough = true,
        "smallcaps" => run.smallcaps = true,
        _ => {}
    }
}

/// Process a single inline HTML element within a paragraph.
pub(super) fn handle_inline_html_element(
    elem: &HtmlElement,
    ctx: &mut WalkCtx,
    para: &mut Paragraph,
) {
    let tag_str = tag_name(elem);
    match tag_str.as_str() {
        "strong" | "b" | "em" | "i" | "code" => {
            // Emphasis and code share one collection shape: descend with the
            // tag's formatting (`for_tag` maps "code" to monospace) and move
            // ALL inlines (not just text runs) — emphasis can wrap an inline
            // equation (typst 0.15 nests math inside the emphasis element),
            // which the shared inline collector emits as OMML via the
            // introspector; `drain_text_runs` would silently drop that math.
            let mut tmp = collect_children_generic(
                &elem.children,
                InlineFmt::default().for_tag(tag_str.as_str()),
                Some(ctx.html_doc),
            );
            para.inlines.append(&mut tmp.inlines);
        }
        "math" => {
            // typst 0.15 emits an inline equation as a native MathML `<math>`
            // element ALONGSIDE the `Tag::Start("equation")` introspection marker.
            // The equation handler already produced OMML from that marker, so skip
            // the `<math>` element — walking it would re-emit the equation's glyphs
            // as literal duplicate text.
        }
        _ if has_attr_value(elem, "role", "doc-noteref") => {
            // The footnote reference marker. typst 0.15 puts the role on the
            // wrapping `<sup>` (0.14 used `<a>`), so match by role, not tag name.
            // Already emitted by the `Tag::Start("footnote")` handler; skip its
            // text so the number isn't also rendered as literal superscript.
        }
        "a" => {
            // External hyperlink from HTML <a href="...">
            if let Some(href) = get_attr_value(elem, "href") {
                let mut tmp = collect_children_generic(&elem.children, InlineFmt::default(), None);
                let runs = drain_text_runs(&mut tmp);
                if !runs.is_empty() {
                    para.add_hyperlink(href, runs);
                }
            } else {
                collect_inlines(&elem.children, para, Some(ctx), InlineOptions::paragraph());
            }
        }
        "sup" | "sub" => {
            // One shape, one flag difference: collect the children as plain
            // text runs, then mark them superscript or subscript. Text runs
            // only — `drain_text_runs` drops anything else (e.g. an equation
            // nested under the marker), as before.
            let mut tmp = collect_children_generic(&elem.children, InlineFmt::default(), None);
            for mut run in drain_text_runs(&mut tmp) {
                if tag_str == "sup" {
                    run.superscript = true;
                } else {
                    run.subscript = true;
                }
                para.push_run(run);
            }
        }
        "br" => {
            // A forced line break (`\`) — without this it falls into the default arm
            // (no children) and the surrounding words glue together.
            para.push_run(Run::line_break());
        }
        _ => {
            collect_inlines(&elem.children, para, Some(ctx), InlineOptions::paragraph());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{InlinePurpose, skip_inline_element};

    #[test]
    fn skip_inline_element_generic_skips_math_and_noteref() {
        // Generic skips the MathML echo of an equation and the footnote
        // reference marker, but descends into anything else.
        assert!(skip_inline_element(InlinePurpose::Generic, "math", None));
        assert!(skip_inline_element(
            InlinePurpose::Generic,
            "sup",
            Some("doc-noteref")
        ));
        assert!(!skip_inline_element(InlinePurpose::Generic, "sup", None));
        assert!(!skip_inline_element(
            InlinePurpose::Generic,
            "sup",
            Some("doc-backlink")
        ));
        assert!(!skip_inline_element(InlinePurpose::Generic, "span", None));
    }

    #[test]
    fn skip_inline_element_formatted_run_skips_nothing() {
        // FormattedRun deliberately descends into MathML (see #12); only the
        // other purposes suppress anything.
        assert!(!skip_inline_element(
            InlinePurpose::FormattedRun,
            "math",
            None
        ));
        assert!(!skip_inline_element(
            InlinePurpose::FormattedRun,
            "sup",
            Some("doc-noteref")
        ));
        assert!(!skip_inline_element(
            InlinePurpose::FormattedRun,
            "a",
            Some("doc-backlink")
        ));
    }

    #[test]
    fn skip_inline_element_footnote_skips_math_and_backlink() {
        assert!(skip_inline_element(InlinePurpose::Footnote, "math", None));
        assert!(skip_inline_element(
            InlinePurpose::Footnote,
            "a",
            Some("doc-backlink")
        ));
        assert!(!skip_inline_element(
            InlinePurpose::Footnote,
            "a",
            Some("doc-noteref")
        ));
        assert!(!skip_inline_element(InlinePurpose::Footnote, "span", None));
    }

    #[test]
    fn skip_inline_element_paragraph_never_skips() {
        // Paragraph dispatches to handle_inline_html_element, which owns its
        // own skipping; the shared predicate stays out of its way.
        assert!(!skip_inline_element(InlinePurpose::Paragraph, "math", None));
        assert!(!skip_inline_element(
            InlinePurpose::Paragraph,
            "sup",
            Some("doc-noteref")
        ));
    }
}
