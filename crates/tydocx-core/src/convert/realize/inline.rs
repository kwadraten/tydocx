use std::collections::HashMap;

use crate::convert::footnote::parse_circled_number;
use crate::convert::page::{
    CounterUse, DatePattern, DateTimeUse, classify_counter_use, counter_footnote_label,
    counter_page_label, date_display_pattern, datetime_use, docproperty_use, typst_pattern_to_word,
};
use tydocx_ir::{
    Degradation, DegradationAction, HangingIndent, IrField, IrFirstLineIndent, IrImage, IrInline,
    IrPageTarget, IrParStyle, IrParagraph,
};
use tydocx_ooxml::document::Alignment;
use typst_library::diag::SourceResult;
use typst_library::engine::Engine;
use typst_library::foundations::{
    Content, ContextElem, Packed, SequenceElem, Smart, StyleChain, StyledElem,
};
use typst_library::introspection::{Location, SplitLocator, Tag, TagElem};
use typst_library::layout::{
    Abs, AlignElem, BlockElem, BoxElem, HAlignment, HElem, HideElem, PadElem, Spacing,
};
use typst_library::math::EquationElem;
use typst_library::model::{
    CiteElem, CiteGroup, Destination, FootnoteBody, FootnoteElem, LinkElem, LinkTarget, Numbering,
    ParElem, ParbreakElem, RefElem, RefForm,
};
use typst_library::routines::Pair;
use typst_library::text::{
    BottomEdge, BottomEdgeMetric, DecoLine, Lang, LinebreakElem, Region, SmartQuoteElem,
    SmartQuoter, SmartQuotes, SpaceElem, TextElem, TopEdge, TopEdgeMetric,
};
use typst_library::visualize::{ImageElem, ImageFit, ImageKind};
use typst_syntax::Source;
use typst_syntax::Span;

use super::collect::{block_body_content, shape_fill_hex, shape_parts, transparent_container_body};
use super::shape::resolve_shape;
use super::style::{StyleDefaults, lower_runs_match, run_from_styles};
use super::with_fragment;
use crate::convert::image::hash_bytes;
use crate::convert::table_align::paint_to_shd_hex;

/// A footnote seen in the pair stream: its location (matched against the
/// paged frame entries), its rendered marker text, and the label a
/// `counter(footnote).at(<label>)` reference would name.
pub struct FootnoteMarker {
    pub location: Location,
    pub rendered: String,
    pub label: Option<String>,
}

pub struct InlineCtx<'a> {
    pub defaults: &'a StyleDefaults,
    pub quoter: SmartQuoter,
    pub prev_char: Option<char>,
    pub label_map: &'a HashMap<Location, String>,
    pub bib_map: &'a HashMap<Location, String>,
    /// Cite-group source span → entry bookmark, resolved via `Works` (the
    /// rendered citation's entry link is layout-time content the pairs lack).
    pub cite_span_map: &'a HashMap<Span, String>,
    pub cite_render_map: &'a HashMap<Span, String>,
    pub fragment_tags: &'a mut Vec<(Tag, usize)>,
    pub pair_index: usize,
    pub footnote_markers: &'a mut Vec<FootnoteMarker>,
    pub degradations: &'a mut Vec<Degradation>,
    pub source: Option<&'a Source>,
    pub block_math_as_inline: bool,
    pub footnote_bodies: &'a mut HashMap<Location, Vec<IrParagraph>>,
    pub footnote_circled: &'a mut bool,
    pub layouter_index: &'a HashMap<Span, Content>,
    /// The `par` style resolved on the fragment's first realized element —
    /// the chain a set rule scoped inside a show-rule body lands on, whether
    /// or not a `ParElem` pair materializes. `None` for an empty fragment.
    pub scoped_par: Option<IrParStyle>,
    /// The `\@` picture `document.date` renders in its kind-dependent auto
    /// format — `None` when the `document(date:)` value has no datetime
    /// (`none` or unrecoverable).
    pub doc_date_picture: Option<&'static str>,
}

pub fn smart_quotes_for(styles: StyleChain<'_>) -> (Lang, Option<Region>) {
    (styles.get(TextElem::lang), styles.get(TextElem::region))
}

pub fn collect_inlines(
    content: &Content,
    styles: StyleChain<'_>,
    engine: &mut Engine,
    locator: &mut SplitLocator,
    ctx: &mut InlineCtx<'_>,
) -> SourceResult<Vec<IrInline>> {
    with_fragment(
        engine,
        locator,
        content,
        styles,
        |engine, locator, pairs| {
            let mut depth = 0usize;
            let mut capture_at: Option<usize> = None;
            for (content, _) in pairs {
                let Some(tagged) = content.to_packed::<TagElem>() else {
                    continue;
                };
                match &tagged.tag {
                    Tag::Start(inner, _) => {
                        if inner.to_packed::<CiteGroup>().is_some() && capture_at.is_none() {
                            capture_at = Some(depth + 1);
                        }
                        if capture_at.is_some() {
                            ctx.fragment_tags.push((tagged.tag.clone(), ctx.pair_index));
                        }
                        depth += 1;
                    }
                    Tag::End(..) => {
                        if capture_at.is_some() {
                            ctx.fragment_tags.push((tagged.tag.clone(), ctx.pair_index));
                        }
                        if capture_at == Some(depth) {
                            capture_at = None;
                        }
                        depth = depth.saturating_sub(1);
                    }
                }
            }
            ctx.scoped_par = ctx.scoped_par.or_else(|| {
                pairs
                    .iter()
                    .find(|(pair, _)| pair.to_packed::<TagElem>().is_none())
                    .map(|(_, chain)| par_style_from_styles(*chain))
            });
            Ok(collect_fragment_inlines(pairs, engine, locator, ctx))
        },
    )
}

fn collect_fragment_inlines(
    pairs: &[Pair<'_>],
    engine: &mut Engine,
    locator: &mut SplitLocator,
    ctx: &mut InlineCtx<'_>,
) -> Vec<IrInline> {
    let mut out: Vec<IrInline> = Vec::new();
    let mut i = 0;
    while i < pairs.len() {
        let (content, styles) = pairs[i];
        if let Some(tagged) = content.to_packed::<TagElem>() {
            i = match &tagged.tag {
                Tag::Start(inner, _) => {
                    handle_tag_start(pairs, i, inner, engine, locator, ctx, &mut out)
                }
                Tag::End(_, _, _) => i + 1,
            };
            continue;
        }
        if let Some(par) = content.to_packed::<ParElem>() {
            push_par_inlines(par, styles, engine, locator, ctx, &mut out);
            i += 1;
            continue;
        }
        if let Some(text) = content.to_packed::<TextElem>() {
            push_text(&mut out, &text.text, styles, Some(content.span()), ctx);
            i += 1;
            continue;
        }
        if let Some(sq) = content.to_packed::<SmartQuoteElem>() {
            push_smartquote(sq, styles, content.span(), ctx, &mut out);
            i += 1;
            continue;
        }
        if content.to_packed::<SpaceElem>().is_some() {
            out.push(IrInline::Space);
            ctx.prev_char = Some(' ');
            i += 1;
            continue;
        }
        if let Some(linebreak) = content.to_packed::<LinebreakElem>() {
            out.push(IrInline::LineBreak {
                justify: linebreak.justify.get(styles),
            });
            ctx.prev_char = Some('\n');
            i += 1;
            continue;
        }
        if let Some(equation) = content.to_packed::<EquationElem>() {
            if !equation.block.get(styles) {
                let omml = super::equation_omml_ir(content, ctx.degradations);
                out.push(IrInline::Math { omml });
                ctx.prev_char = Some('\u{FFFC}');
            }
            i += 1;
            continue;
        }
        if let Some(image) = content.to_packed::<ImageElem>() {
            push_image_inline(image, styles, engine, ctx, &mut out);
            i += 1;
            continue;
        }
        if let Some(box_elem) = content.to_packed::<BoxElem>() {
            collect_box_inlines(box_elem, styles, engine, locator, ctx, &mut out);
            i += 1;
            continue;
        }
        if let Some(h) = content.to_packed::<HElem>() {
            push_tracked_space(h, styles, content.span(), ctx, &mut out);
            i += 1;
            continue;
        }
        if let Some(pad) = content.to_packed::<PadElem>() {
            collect_pad_inlines(pad, styles, engine, locator, ctx, &mut out);
            i += 1;
            continue;
        }
        if collect_shape_or_transparent(content, styles, engine, locator, ctx, &mut out) {
            i += 1;
            continue;
        }
        i += 1;
    }
    out
}

/// Layouter shapes, rect/curve/etc. bodies, and transparent layout wrappers
/// all contribute inlines without their chrome; returns whether the pair
/// matched one of them.
fn collect_shape_or_transparent(
    content: &Content,
    styles: StyleChain<'_>,
    engine: &mut Engine,
    locator: &mut SplitLocator,
    ctx: &mut InlineCtx<'_>,
    out: &mut Vec<IrInline>,
) -> bool {
    if let Some(block) = content.to_packed::<BlockElem>()
        && block_body_content(block, styles).is_none()
    {
        collect_layouter_shape(content, styles, engine, locator, ctx, out);
        return true;
    }
    if let Some((kind, body)) = shape_parts(content) {
        push_shape_inline(
            content,
            kind,
            body,
            shape_fill_hex(content, styles),
            content.span(),
            styles,
            engine,
            locator,
            ctx,
            out,
        );
        return true;
    }
    if let Some(inlines) = collect_transparent_inlines(content, styles, engine, locator, ctx) {
        out.extend(inlines);
        return true;
    }
    false
}

/// `move`/`rotate`/`scale`/`skew`/`columns` inside a fragment: chrome is
/// dropped (degraded), body inlines are kept.
fn collect_transparent_inlines(
    content: &Content,
    styles: StyleChain<'_>,
    engine: &mut Engine,
    locator: &mut SplitLocator,
    ctx: &mut InlineCtx<'_>,
) -> Option<Vec<IrInline>> {
    let (construct, body) = transparent_container_body(content)?;
    ctx.degradations.push(Degradation {
        span: Some(content.span()),
        construct,
        action: DegradationAction::Skipped,
        message: "the container's transform or column layout has no OOXML equivalent; the \
                  content is kept without it"
            .to_string(),
    });
    if let Ok(inlines) = collect_inlines(body, styles, engine, locator, ctx) {
        return Some(inlines);
    }
    ctx.degradations.push(Degradation {
        span: Some(content.span()),
        construct: "transformed-content",
        action: DegradationAction::Skipped,
        message: "content inside a transform could not be realized; its text was dropped"
            .to_string(),
    });
    Some(Vec::new())
}

fn push_tracked_space(
    h: &Packed<HElem>,
    styles: StyleChain<'_>,
    span: Span,
    ctx: &mut InlineCtx<'_>,
    out: &mut Vec<IrInline>,
) {
    if let Some(twips) = spacing_twips(&h.amount, styles) {
        let mut run = run_from_styles(" ", styles, ctx.defaults, Some(span));
        run.tracking_twips = Some(twips);
        out.push(IrInline::Text(run));
        ctx.prev_char = Some(' ');
    } else {
        report_spacing_gap(&h.amount, true, span, ctx.degradations);
    }
}

/// `h`/`v` amounts that `spacing_twips` cannot map: `fr` flex space (no OOXML
/// equivalent; a right tab is the C2 target for `h(1fr)`), a ratio part
/// (`h(50%)`/`v(25%)`), and negative space.
pub(crate) fn report_spacing_gap(
    amount: &Spacing,
    horizontal: bool,
    span: Span,
    degradations: &mut Vec<Degradation>,
) {
    // A detached span means the spacing element was synthesized by Typst's
    // own realization (e.g. a heading's numbering run), not authored —
    // reporting it would degrade for a construct the author never wrote.
    if span.id().is_none() {
        return;
    }
    let (construct, message) = match amount {
        Spacing::Fr(_) => (
            if horizontal {
                "h-fr-space"
            } else {
                "v-fr-space"
            },
            "an fr flex space is dropped; it has no fixed-size OOXML equivalent",
        ),
        Spacing::Rel(rel) if rel.abs.abs < Abs::zero() => (
            if horizontal {
                "h-negative-space"
            } else {
                "v-negative-space"
            },
            "a negative space is dropped; OOXML has no negative inter-block spacing",
        ),
        Spacing::Rel(rel) if rel.rel != typst_library::layout::Ratio::zero() => (
            if horizontal {
                "h-ratio-space"
            } else {
                "v-ratio-space"
            },
            "a ratio space is dropped; its parent-relative extent cannot be resolved",
        ),
        Spacing::Rel(_) => return,
    };
    degradations.push(Degradation {
        span: Some(span),
        construct,
        action: DegradationAction::Skipped,
        message: message.to_string(),
    });
}

fn collect_pad_inlines(
    pad: &Packed<PadElem>,
    styles: StyleChain<'_>,
    engine: &mut Engine,
    locator: &mut SplitLocator,
    ctx: &mut InlineCtx<'_>,
    out: &mut Vec<IrInline>,
) {
    match collect_inlines(&pad.body, styles, engine, locator, ctx) {
        Ok(inlines) => out.extend(inlines),
        Err(_) => ctx.degradations.push(Degradation {
            span: None,
            construct: "pad-in-fragment",
            action: DegradationAction::Skipped,
            message: "a #pad inside a paragraph could not be realized; its content was dropped"
                .to_string(),
        }),
    }
}

fn collect_layouter_shape(
    content: &Content,
    styles: StyleChain<'_>,
    engine: &mut Engine,
    locator: &mut SplitLocator,
    ctx: &mut InlineCtx<'_>,
    out: &mut Vec<IrInline>,
) {
    if let Some(original) = ctx.layouter_index.get(&content.span()).cloned()
        && let Some((kind, body)) = shape_parts(&original)
    {
        push_shape_inline(
            &original,
            kind,
            body,
            shape_fill_hex(&original, styles),
            original.span(),
            styles,
            engine,
            locator,
            ctx,
            out,
        );
    }
}

fn push_par_inlines(
    par: &Packed<ParElem>,
    styles: StyleChain<'_>,
    engine: &mut Engine,
    locator: &mut SplitLocator,
    ctx: &mut InlineCtx<'_>,
    out: &mut Vec<IrInline>,
) {
    match collect_inlines(&par.body, styles, engine, locator, ctx) {
        Ok(inlines) => out.extend(inlines),
        Err(_) => ctx.degradations.push(Degradation {
            span: None,
            construct: "paragraph-in-fragment",
            action: DegradationAction::Skipped,
            message: "a paragraph inside a fragment could not be realized; its text was dropped"
                .to_string(),
        }),
    }
}

fn push_smartquote(
    sq: &Packed<SmartQuoteElem>,
    styles: StyleChain<'_>,
    span: Span,
    ctx: &mut InlineCtx<'_>,
    out: &mut Vec<IrInline>,
) {
    let double = sq.double.get(styles);
    let (lang, region) = smart_quotes_for(styles);
    let quotes = SmartQuotes::get(&Smart::Auto, lang, region, false);
    let quote = ctx.quoter.quote(ctx.prev_char, &quotes, double).to_string();
    ctx.prev_char = quote.chars().next_back();
    out.push(IrInline::Text(run_from_styles(
        &quote,
        styles,
        ctx.defaults,
        Some(span),
    )));
}

fn push_image_inline(
    image: &Packed<ImageElem>,
    styles: StyleChain<'_>,
    engine: &mut Engine,
    ctx: &mut InlineCtx<'_>,
    out: &mut Vec<IrInline>,
) {
    if let Some(ir) = image_inline(image, styles, engine, ctx) {
        out.push(ir);
        ctx.prev_char = Some('\u{FFFC}');
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "the inline walk context is split by convention"
)]
fn push_shape_inline(
    content: &Content,
    kind: tydocx_ir::IrShapeKind,
    body: Option<Content>,
    fill_hex: Option<String>,
    span: Span,
    styles: StyleChain<'_>,
    engine: &mut Engine,
    locator: &mut SplitLocator,
    ctx: &mut InlineCtx<'_>,
    out: &mut Vec<IrInline>,
) {
    let span = if span.id().is_some() {
        Some(span)
    } else {
        None
    };
    let resolved = resolve_shape(content, styles, engine, locator);
    let Some(body_content) = body else {
        out.push(IrInline::Shape {
            kind,
            span,
            body: Vec::new(),
            fill_hex,
            geometry: resolved.geometry,
            loss: resolved.loss,
        });
        ctx.prev_char = Some('\u{FFFC}');
        return;
    };
    match collect_inlines(&body_content, styles, engine, locator, ctx) {
        Ok(body_inlines) => {
            out.push(IrInline::Shape {
                kind,
                span,
                body: body_inlines,
                fill_hex,
                geometry: resolved.geometry,
                loss: resolved.loss,
            });
            ctx.prev_char = Some('\u{FFFC}');
        }
        Err(_) => ctx.degradations.push(Degradation {
            span: None,
            construct: "shape-in-fragment",
            action: DegradationAction::Skipped,
            message: "a shape inside a fragment could not be realized; its text was dropped"
                .to_string(),
        }),
    }
}

fn collect_box_inlines(
    box_elem: &Packed<BoxElem>,
    styles: StyleChain<'_>,
    engine: &mut Engine,
    locator: &mut SplitLocator,
    ctx: &mut InlineCtx<'_>,
    out: &mut Vec<IrInline>,
) {
    let Some(Some(body)) = box_elem.body.as_option() else {
        return;
    };
    report_box_aspects(box_elem, styles, ctx);
    let fill_hex = match box_elem.fill.as_option() {
        Some(Some(paint)) => paint_to_shd_hex(paint),
        _ => None,
    };
    match collect_inlines(body, styles, engine, locator, ctx) {
        Ok(mut inlines) => {
            if let Some(hex) = fill_hex {
                for inline in &mut inlines {
                    if let IrInline::Text(run) = inline {
                        run.shading = Some(hex.clone());
                    }
                }
            }
            out.extend(inlines);
        }
        Err(_) => ctx.degradations.push(Degradation {
            span: None,
            construct: "box-in-fragment",
            action: DegradationAction::Skipped,
            message: "a box inside a fragment could not be realized; its text was dropped"
                .to_string(),
        }),
    }
}

/// The box properties that the inline-text C1 form drops: width/height
/// constraints (`box(width: 1fr)` right-push, fixed widths), a non-default
/// baseline, and the stroke/radius/clip decoration (fill is mapped to run
/// shading, so it is not reported).
fn report_box_aspects(box_elem: &Packed<BoxElem>, styles: StyleChain<'_>, ctx: &mut InlineCtx<'_>) {
    let span = Some(box_elem.span());
    if !box_elem.width.get(styles).is_auto() || box_elem.height.get(styles).is_custom() {
        ctx.degradations.push(Degradation {
            span,
            construct: "box-width",
            action: DegradationAction::Skipped,
            message: "a box width/height constraint is dropped; the content flows full-width"
                .to_string(),
        });
    }
    if box_elem.baseline.get(styles) != typst_library::layout::BaselinePos::default() {
        ctx.degradations.push(Degradation {
            span,
            construct: "box-baseline",
            action: DegradationAction::Skipped,
            message: "a box baseline override is dropped".to_string(),
        });
    }
    let stroked = box_elem
        .stroke
        .get_cloned(styles)
        .iter()
        .any(|side| side.as_ref().is_some_and(Option::is_some));
    let rounded = box_elem
        .radius
        .get_cloned(styles)
        .iter()
        .any(Option::is_some);
    if stroked || rounded || box_elem.clip.get(styles) {
        ctx.degradations.push(Degradation {
            span,
            construct: "box-decoration",
            action: DegradationAction::Skipped,
            message: "a box stroke/radius/clip decoration is dropped".to_string(),
        });
    }
}

#[allow(clippy::too_many_lines, reason = "one dispatch per tag kind")]
fn handle_tag_start(
    pairs: &[Pair<'_>],
    i: usize,
    inner: &Content,
    engine: &mut Engine,
    locator: &mut SplitLocator,
    ctx: &mut InlineCtx<'_>,
    out: &mut Vec<IrInline>,
) -> usize {
    let (_, styles) = pairs[i];
    let Some(location) = inner.location() else {
        return i + 1;
    };
    // `#hide[…]` leaves no trace in the output at all.
    if inner.to_packed::<HideElem>().is_some() {
        ctx.degradations.push(Degradation {
            span: Some(inner.span()),
            construct: "hidden-content",
            action: DegradationAction::Skipped,
            message: "hidden content is dropped along with its reserved space".to_string(),
        });
        return skip_to_end(pairs, i + 1, location);
    }
    if let Some((construct, _)) = transparent_container_body(inner) {
        ctx.degradations.push(Degradation {
            span: Some(inner.span()),
            construct,
            action: DegradationAction::Skipped,
            message: "the container's transform or column layout has no OOXML equivalent; the \
                      content is kept without it"
                .to_string(),
        });
        return i + 1;
    }
    if inner.to_packed::<ContextElem>().is_some() {
        if let Some(label) = ctx
            .source
            .and_then(|source| counter_page_label(source, inner.span()))
        {
            let end = find_end(pairs, i + 1, location);
            let display = marker_text_in(&pairs[i + 1..end]);
            ctx.prev_char = display.chars().next_back().or(ctx.prev_char);
            let bookmark_name = label_location(ctx.label_map, &label)
                .and_then(|loc| ctx.label_map.get(&loc).cloned())
                .unwrap_or_else(|| label.clone());
            out.push(IrInline::Ref {
                target_label: bookmark_name,
                display_text: display,
                page_ref: true,
                page_target: label_location(ctx.label_map, &label).map(|loc| IrPageTarget {
                    location: loc.hash(),
                    numbered: false,
                    span: Some(inner.span()),
                }),
            });
            return end + 1;
        }
        match ctx.source.map_or(CounterUse::Other, |source| {
            classify_counter_use(source, inner.span())
        }) {
            CounterUse::CurrentPage => {
                let end = find_end(pairs, i + 1, location);
                let display = marker_text_in(&pairs[i + 1..end]);
                ctx.prev_char = display.chars().next_back().or(ctx.prev_char);
                out.push(IrInline::PageField {
                    display_text: display,
                    location: Some(location.hash()),
                    span: Some(inner.span()),
                });
                return end + 1;
            }
            CounterUse::PageTotal => {
                return push_context_field(
                    pairs,
                    i,
                    location,
                    IrField::NumPages,
                    inner.span(),
                    engine,
                    locator,
                    ctx,
                    out,
                );
            }
            CounterUse::PageCounterUnsupported => {
                ctx.degradations.push(Degradation {
                    span: Some(inner.span()),
                    construct: "page-counter-value",
                    action: DegradationAction::Literalized,
                    message: "page-counter value Word cannot recompute (e.g. arithmetic around \
                              counter(page)) kept as literal text"
                        .to_string(),
                });
            }
            CounterUse::FootnoteCounter => {
                if let Some(label) = ctx
                    .source
                    .and_then(|source| counter_footnote_label(source, inner.span()))
                {
                    return push_context_field(
                        pairs,
                        i,
                        location,
                        IrField::NoteRef { label },
                        inner.span(),
                        engine,
                        locator,
                        ctx,
                        out,
                    );
                }
                ctx.degradations.push(Degradation {
                    span: Some(inner.span()),
                    construct: "footnote-counter-reference",
                    action: DegradationAction::Literalized,
                    message: "footnote-counter reference kept as literal text (NOTEREF is a C3 \
                              field)"
                        .to_string(),
                });
            }
            CounterUse::Other => {
                let field = ctx
                    .source
                    .and_then(|source| {
                        datetime_use(source, inner.span()).map(|use_| {
                            let pattern = date_display_pattern(source, inner.span());
                            match use_ {
                                DateTimeUse::Date => IrField::Date {
                                    picture: date_picture(
                                        pattern,
                                        Some("yyyy-MM-dd"),
                                        inner.span(),
                                        ctx.degradations,
                                    ),
                                },
                                DateTimeUse::Time => IrField::Time,
                            }
                        })
                    })
                    .or_else(|| {
                        ctx.source.and_then(|source| {
                            docproperty_use(source, inner.span()).map(|name| {
                                let picture = if name == "CreateDate" {
                                    date_picture(
                                        date_display_pattern(source, inner.span()),
                                        ctx.doc_date_picture,
                                        inner.span(),
                                        ctx.degradations,
                                    )
                                } else {
                                    None
                                };
                                IrField::DocProperty {
                                    name: name.to_string(),
                                    picture,
                                }
                            })
                        })
                    });
                if let Some(field) = field {
                    return push_context_field(
                        pairs,
                        i,
                        location,
                        field,
                        inner.span(),
                        engine,
                        locator,
                        ctx,
                        out,
                    );
                }
            }
        }
    }
    if let Some(image) = inner.to_packed::<ImageElem>() {
        if let Some(ir) = image_inline(image, styles, engine, ctx) {
            out.push(ir);
            ctx.prev_char = Some('\u{FFFC}');
        }
        return skip_to_end(pairs, i + 1, location);
    }
    if let Some(fn_elem) = inner.to_packed::<FootnoteElem>() {
        if let Numbering::Pattern(pattern) = fn_elem.numbering.get_ref(styles)
            && pattern
                .apply(None, &[1])
                .is_ok_and(|text| parse_circled_number(text.trim()).is_some())
        {
            *ctx.footnote_circled = true;
        }
        let rendered = match next_marker_text(pairs, i + 1) {
            Some(text) if text != "0" => text,
            _ => (ctx.footnote_markers.len() + 1).to_string(),
        };
        ctx.footnote_markers.push(FootnoteMarker {
            location,
            rendered: rendered.clone(),
            label: inner.label().map(|label| label.resolve().to_string()),
        });
        let mut body_ctx = InlineCtx {
            defaults: ctx.defaults,
            quoter: SmartQuoter::new(),
            prev_char: None,
            label_map: ctx.label_map,
            bib_map: ctx.bib_map,
            cite_span_map: ctx.cite_span_map,
            cite_render_map: ctx.cite_render_map,
            fragment_tags: &mut *ctx.fragment_tags,
            pair_index: ctx.pair_index,
            footnote_markers: &mut *ctx.footnote_markers,
            degradations: &mut *ctx.degradations,
            source: ctx.source,
            block_math_as_inline: true,
            footnote_bodies: &mut *ctx.footnote_bodies,
            footnote_circled: &mut *ctx.footnote_circled,
            layouter_index: ctx.layouter_index,
            scoped_par: None,
            doc_date_picture: ctx.doc_date_picture,
        };
        if let FootnoteBody::Content(body) = &fn_elem.body {
            // Split on parbreak children so a multi-paragraph footnote body
            // keeps its paragraphs (each maps to its own `w:p` downstream).
            // `set`/`align` rules inside the body arrive as `StyledElem` /
            // `AlignElem` children — their chain decides that segment's
            // paragraph alignment, not the footnote's ambient chain.
            let mut alignment = alignment_from_styles(styles);
            let mut par = IrParStyle {
                justify: Some(styles.get(ParElem::justify)),
                ..IrParStyle::default()
            };
            let mut paragraphs: Vec<IrParagraph> = Vec::new();
            let mut inlines = Vec::new();
            for child in body
                .to_packed::<SequenceElem>()
                .map_or_else(|| vec![body.clone()], |seq| seq.children.clone())
            {
                if child.is::<ParbreakElem>() {
                    if !inlines.is_empty() {
                        paragraphs.push(IrParagraph {
                            inlines: std::mem::take(&mut inlines),
                            alignment,
                            par,
                            ..IrParagraph::default()
                        });
                    }
                } else if let Ok(mut child_inlines) =
                    collect_inlines(&child, styles, engine, locator, &mut body_ctx)
                {
                    inlines.append(&mut child_inlines);
                    if let Some(styled) = child.to_packed::<StyledElem>() {
                        let chain = styles.chain(&styled.styles);
                        alignment = alignment_from_styles(chain);
                        par = IrParStyle {
                            justify: Some(chain.get(ParElem::justify)),
                            ..IrParStyle::default()
                        };
                    } else if let Some(align) = child.to_packed::<AlignElem>() {
                        alignment = match align.alignment.get(styles).x() {
                            Some(HAlignment::Center) => Some(Alignment::Center),
                            Some(HAlignment::Right | HAlignment::End) => Some(Alignment::Right),
                            Some(HAlignment::Left | HAlignment::Start) | None => None,
                        };
                    }
                }
            }
            if !inlines.is_empty() {
                paragraphs.push(IrParagraph {
                    inlines,
                    alignment,
                    par,
                    ..IrParagraph::default()
                });
            }
            if !paragraphs.is_empty() {
                ctx.footnote_bodies.insert(location, paragraphs);
            }
        }
        out.push(IrInline::FootnoteMarker {
            ordinal: u32::try_from(ctx.footnote_markers.len()).unwrap_or(u32::MAX),
            rendered,
        });
        ctx.prev_char = Some('\u{FFFC}');
        return skip_to_end(pairs, i + 1, location);
    }
    if let Some(reference) = inner.to_packed::<RefElem>() {
        let target_label = reference.target.resolve().to_string();
        if !ctx.label_map.values().any(|label| label == &target_label) {
            let end = find_end(pairs, i + 1, location);
            let rendered = ctx
                .cite_render_map
                .get(&inner.span())
                .cloned()
                .unwrap_or_else(|| marker_text_in(&pairs[i + 1..end]));
            let target = ctx.cite_span_map.get(&inner.span()).cloned().or_else(|| {
                bib_link_in(&pairs[i + 1..end]).and_then(|loc| ctx.bib_map.get(&loc).cloned())
            });
            ctx.prev_char = rendered.chars().next_back().or(ctx.prev_char);
            out.push(IrInline::Cite {
                keys: vec![target_label],
                rendered,
                target,
            });
            return end + 1;
        }
        let display_text = next_marker_text(pairs, i + 1).unwrap_or_default();
        ctx.prev_char = display_text.chars().next_back().or(ctx.prev_char);
        let page_ref = reference.form.get(styles) == RefForm::Page;
        let bookmark_name = label_location(ctx.label_map, &target_label)
            .and_then(|loc| ctx.label_map.get(&loc).cloned())
            .unwrap_or_else(|| target_label.clone());
        out.push(IrInline::Ref {
            target_label: bookmark_name,
            display_text,
            page_ref,
            page_target: page_ref
                .then(|| {
                    label_location(ctx.label_map, &target_label).map(|loc| IrPageTarget {
                        location: loc.hash(),
                        numbered: true,
                        span: None,
                    })
                })
                .flatten(),
        });
        return skip_to_end(pairs, i + 1, location);
    }
    if let Some(group) = inner.to_packed::<CiteGroup>() {
        let keys: Vec<String> = group
            .children
            .iter()
            .filter_map(|child| child.to_packed::<CiteElem>())
            .map(|cite| cite.key.resolve().to_string())
            .collect();
        let end = find_end(pairs, i + 1, location);
        let rendered = ctx
            .cite_render_map
            .get(&inner.span())
            .cloned()
            .unwrap_or_else(|| marker_text_in(&pairs[i + 1..end]));
        let target = ctx.cite_span_map.get(&inner.span()).cloned().or_else(|| {
            bib_link_in(&pairs[i + 1..end]).and_then(|loc| ctx.bib_map.get(&loc).cloned())
        });
        ctx.prev_char = rendered.chars().next_back().or(ctx.prev_char);
        out.push(IrInline::Cite {
            keys,
            rendered,
            target,
        });
        return skip_to_end(pairs, i + 1, location);
    }
    if let Some(equation) = inner.to_packed::<EquationElem>() {
        if !equation.block.get(styles) || ctx.block_math_as_inline {
            let omml = super::equation_omml_ir(inner, ctx.degradations);
            out.push(IrInline::Math { omml });
            ctx.prev_char = Some('\u{FFFC}');
        }
        return skip_to_end(pairs, i + 1, location);
    }
    if let Some(link) = inner.to_packed::<LinkElem>() {
        let end = find_end(pairs, i + 1, location);
        let body = collect_fragment_inlines(&pairs[i + 1..end], engine, locator, ctx);
        let (url, anchor) = match &link.dest {
            LinkTarget::Dest(Destination::Url(url)) => (Some(url.to_string()), None),
            LinkTarget::Dest(Destination::Location(loc)) => (None, ctx.label_map.get(loc).cloned()),
            LinkTarget::Dest(Destination::Position(_)) => (None, None),
            LinkTarget::Label(label) => (None, Some(label.resolve().to_string())),
        };
        if let Some(url) = url {
            out.push(IrInline::Link {
                url: Some(url),
                anchor: None,
                body,
            });
        } else if let Some(anchor) = anchor {
            out.push(IrInline::Link {
                url: None,
                anchor: Some(anchor),
                body,
            });
        } else {
            out.extend(body);
            ctx.degradations.push(Degradation {
                span: None,
                construct: "unresolved-link",
                action: DegradationAction::Literalized,
                message: "an internal link has no labelled target; kept as plain text".to_string(),
            });
        }
        ctx.prev_char = Some('\u{FFFC}');
        return end + 1;
    }
    i + 1
}

/// The `\@` picture a date/time field emits: the translated literal pattern,
/// or `auto` for the kind-dependent auto format. An unmappable component or
/// a computed pattern emits a bare field plus a `date-format-switch`
/// degradation naming what was dropped.
fn date_picture(
    pattern: DatePattern,
    auto: Option<&'static str>,
    span: Span,
    degradations: &mut Vec<Degradation>,
) -> Option<String> {
    let degrade = |degradations: &mut Vec<Degradation>, message: String| {
        degradations.push(Degradation {
            span: Some(span),
            construct: "date-format-switch",
            action: DegradationAction::Approximated,
            message,
        });
        None
    };
    match pattern {
        DatePattern::Auto => match auto {
            Some(picture) => Some(picture.to_string()),
            None => degrade(
                degradations,
                "the datetime kind cannot be recovered, so no `\\@` picture switch \
                 is emitted"
                    .to_string(),
            ),
        },
        DatePattern::Literal(pattern) => match typst_pattern_to_word(&pattern) {
            Ok(picture) => Some(picture),
            Err(component) => degrade(
                degradations,
                format!(
                    "the `{component}` component in the display pattern has no Word `\\@` \
                     equivalent; the field is emitted without a picture switch"
                ),
            ),
        },
        DatePattern::Computed => degrade(
            degradations,
            "the display pattern is computed, not a string literal; the field is emitted \
             without a `\\@` picture switch"
                .to_string(),
        ),
    }
}

/// Consume a `context` tag group's rendered inlines into a generic C3 field:
/// the group's own realization is the cached result (the C2 literal text),
/// and `kind` is the field instruction Word recomputes on update.
#[allow(clippy::too_many_arguments)]
fn push_context_field(
    pairs: &[Pair<'_>],
    start_tag: usize,
    location: Location,
    kind: IrField,
    span: Span,
    engine: &mut Engine,
    locator: &mut SplitLocator,
    ctx: &mut InlineCtx<'_>,
    out: &mut Vec<IrInline>,
) -> usize {
    let end = find_end(pairs, start_tag + 1, location);
    let body = collect_fragment_inlines(&pairs[start_tag + 1..end], engine, locator, ctx);
    ctx.prev_char = last_inline_char(&body).or(ctx.prev_char);
    out.push(IrInline::Field {
        kind,
        body,
        span: Some(span),
        location: Some(location.hash()),
    });
    end + 1
}

/// The `Location` of the element carrying `label` — the map is keyed
/// location → name, so a page-target lookup scans it.
fn label_location(map: &HashMap<Location, String>, label: &str) -> Option<Location> {
    map.iter()
        .find(|(_, name)| name.as_str() == label)
        .map(|(loc, _)| *loc)
}

/// The last rendered character a run of inlines contributes — for the
/// smart-quote/space state the next text run sees.
fn last_inline_char(inlines: &[IrInline]) -> Option<char> {
    for inline in inlines.iter().rev() {
        match inline {
            IrInline::Text(run) => {
                if let ch @ Some(_) = run.text.chars().next_back() {
                    return ch;
                }
            }
            IrInline::Space => return Some(' '),
            IrInline::Link { body, .. }
            | IrInline::Shape { body, .. }
            | IrInline::Field { body, .. } => {
                if let ch @ Some(_) = last_inline_char(body) {
                    return ch;
                }
            }
            _ => {}
        }
    }
    None
}

/// Every `TextElem` up to the tag's `End`, concatenated — a rendered marker
/// may be several runs (a custom `@ref[supplement]` prefix plus the target
/// text); taking only the first silently drops the rest.
fn next_marker_text(pairs: &[Pair<'_>], start: usize) -> Option<String> {
    let mut out = String::new();
    for (content, _) in &pairs[start..] {
        if let Some(tagged) = content.to_packed::<TagElem>() {
            if matches!(tagged.tag, Tag::End(_, _, _)) {
                break;
            }
            continue;
        }
        if let Some(text) = content.to_packed::<TextElem>() {
            out.push_str(&text.text);
        }
    }
    if out.is_empty() {
        return None;
    }
    Some(tydocx_ooxml::document::strip_break_hints(&out))
}

fn marker_text_in(pairs: &[Pair<'_>]) -> String {
    let mut out = String::new();
    for (content, _) in pairs {
        if let Some(text) = content.to_packed::<TextElem>() {
            out.push_str(&text.text);
        }
    }
    tydocx_ooxml::document::strip_break_hints(&out)
}

/// First location-destination `LinkElem` inside a tag group — a rendered
/// citation wraps its marker in a link to the bibliography entry's backlink
/// location.
fn bib_link_in(pairs: &[Pair<'_>]) -> Option<Location> {
    for (content, _) in pairs {
        let Some(tagged) = content.to_packed::<TagElem>() else {
            continue;
        };
        if let Tag::Start(inner, _) = &tagged.tag
            && let Some(link) = inner.to_packed::<LinkElem>()
            && let LinkTarget::Dest(Destination::Location(loc)) = &link.dest
        {
            return Some(*loc);
        }
    }
    None
}

fn find_end(pairs: &[Pair<'_>], start: usize, location: Location) -> usize {
    let mut i = start;
    while i < pairs.len() {
        if let Some(tagged) = pairs[i].0.to_packed::<TagElem>()
            && let Tag::End(loc, _, _) = tagged.tag
            && loc == location
        {
            return i;
        }
        i += 1;
    }
    pairs.len()
}

fn skip_to_end(pairs: &[Pair<'_>], start: usize, location: Location) -> usize {
    find_end(pairs, start, location) + 1
}

fn degrade_once(
    degradations: &mut Vec<Degradation>,
    span: Option<Span>,
    construct: &'static str,
    message: &'static str,
) {
    if !degradations.iter().any(|d| d.construct == construct) {
        degradations.push(Degradation {
            span,
            construct,
            action: DegradationAction::Skipped,
            message: message.to_string(),
        });
    }
}

/// Text-level properties the run C1 form drops: non-default `top-edge` /
/// `bottom-edge` metrics, a custom `underline(offset:)`, and `highlight`
/// geometry (edges/extent/radius/stroke beyond the fill that maps to
/// `w:highlight`/`w:shd`). Each construct is reported once.
fn report_text_aspects(styles: StyleChain<'_>, span: Option<Span>, ctx: &mut InlineCtx<'_>) {
    if styles.get(TextElem::top_edge) != TopEdge::Metric(TopEdgeMetric::CapHeight)
        || styles.get(TextElem::bottom_edge) != BottomEdge::Metric(BottomEdgeMetric::Baseline)
    {
        degrade_once(
            ctx.degradations,
            span,
            "text-edge-metrics",
            "a text top-edge/bottom-edge metric is dropped; the default metrics are used",
        );
    }
    for decoration in styles.get_cloned(TextElem::deco) {
        match decoration.line {
            DecoLine::Underline {
                offset: Smart::Custom(_),
                ..
            } => degrade_once(
                ctx.degradations,
                span,
                "underline-offset",
                "an underline offset is dropped; the underline uses the default position",
            ),
            DecoLine::Highlight {
                stroke,
                top_edge,
                bottom_edge,
                radius,
                ..
            } => {
                let non_default = decoration.extent > Abs::zero()
                    || stroke.iter().any(Option::is_some)
                    || top_edge != TopEdge::Metric(TopEdgeMetric::CapHeight)
                    || bottom_edge != BottomEdge::Metric(BottomEdgeMetric::Baseline)
                    || radius.iter().any(|r| !r.is_zero());
                if non_default {
                    degrade_once(
                        ctx.degradations,
                        span,
                        "highlight-geometry",
                        "a highlight edge/extent/radius/stroke detail is dropped; only the \
                         fill is kept",
                    );
                }
            }
            _ => {}
        }
    }
}

fn push_text(
    out: &mut Vec<IrInline>,
    text: &str,
    styles: StyleChain<'_>,
    span: Option<Span>,
    ctx: &mut InlineCtx<'_>,
) {
    if text.is_empty() {
        return;
    }
    report_text_aspects(styles, span, ctx);
    let run = run_from_styles(text, styles, ctx.defaults, span);
    ctx.prev_char = run.text.chars().next_back();
    if let Some(IrInline::Text(prev)) = out.last_mut()
        && lower_runs_match(prev, &run)
    {
        prev.text.push_str(&run.text);
        return;
    }
    out.push(IrInline::Text(run));
}

pub(crate) fn image_inline(
    image: &Packed<ImageElem>,
    styles: StyleChain<'_>,
    engine: &mut Engine,
    ctx: &mut InlineCtx<'_>,
) -> Option<IrInline> {
    let Ok(decoded) = image.decode(engine, styles) else {
        return None;
    };
    let vector = matches!(decoded.kind(), ImageKind::Svg(_));
    let (bytes, mime, source_hash, svg) = match decoded.kind() {
        ImageKind::Raster(raster) => {
            let original = raster.data().as_slice();
            let source_hash = hash_bytes(original);
            let bytes = original.to_vec();
            if is_jpeg(&bytes) {
                (bytes, Some("image/jpeg".to_string()), source_hash, None)
            } else if is_png(&bytes) {
                (bytes, Some("image/png".to_string()), source_hash, None)
            } else {
                let png = reencode_png(&bytes)?;
                (png, Some("image/png".to_string()), source_hash, None)
            }
        }
        ImageKind::Svg(svg_data) => {
            let original = svg_data.data().as_slice();
            let png = rasterize_svg(original)?;
            (
                png,
                Some("image/png".to_string()),
                hash_bytes(original),
                Some(original.to_vec()),
            )
        }
        ImageKind::Pdf(_) => {
            ctx.degradations.push(Degradation {
                span: None,
                construct: "pdf-image",
                action: DegradationAction::Skipped,
                message: "a PDF image has no raster form in the IR path; skipped".to_string(),
            });
            return None;
        }
    };
    Some(IrInline::Image(IrImage {
        bytes,
        source_hash,
        width_emu: 0,
        height_emu: 0,
        mime,
        span: {
            let span = image.span();
            span.id().map(|_| span)
        },
        svg,
        vector,
        non_cover_fit: image
            .fit
            .as_option()
            .is_some_and(|fit| fit != ImageFit::Cover),
        src_rect: None,
        alt: image
            .alt
            .as_option()
            .clone()
            .flatten()
            .filter(|alt| !alt.is_empty())
            .map(|alt| alt.to_string()),
    }))
}

fn is_png(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes[..4] == [0x89, 0x50, 0x4E, 0x47]
}

fn is_jpeg(bytes: &[u8]) -> bool {
    bytes.len() >= 3 && bytes[..3] == [0xFF, 0xD8, 0xFF]
}

fn reencode_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let decoded = ::image::load_from_memory(bytes).ok()?;
    let mut png = Vec::new();
    decoded
        .write_to(
            &mut std::io::Cursor::new(&mut png),
            ::image::ImageFormat::Png,
        )
        .ok()?;
    Some(png)
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "SVG canvas dimensions are small positive values"
)]
fn rasterize_svg(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_data(bytes, &options).ok()?;
    let size = tree.size();
    let (pixel_w, pixel_h) = (size.width().ceil() as u32, size.height().ceil() as u32);
    if pixel_w == 0 || pixel_h == 0 {
        return None;
    }
    let mut pixmap = tiny_skia::Pixmap::new(pixel_w, pixel_h)?;
    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());
    pixmap.encode_png().ok()
}

pub fn collect_paragraph(
    content: &Content,
    styles: StyleChain<'_>,
    engine: &mut Engine,
    locator: &mut SplitLocator,
    ctx: &mut InlineCtx<'_>,
) -> SourceResult<IrParagraph> {
    let inlines = collect_inlines(content, styles, engine, locator, ctx)?;
    Ok(IrParagraph {
        inlines,
        alignment: alignment_from_styles(styles),
        par: par_style_from_styles(styles),
        hanging_indent: hanging_indent_from_styles(styles),
        ..IrParagraph::default()
    })
}

/// `par` settings resolved on the element's own `StyleChain` — the values
/// Typst actually laid the paragraph out with, including set rules scoped
/// inside `#let` helpers and show rules (the source-AST pass sees only
/// top-level set rules). An em indent resolves at the chain's font size so a
/// `#set par(first-line-indent: 2em)` under a smaller caption size yields
/// the caption's own twips, matching Typst's layout.
#[allow(
    clippy::cast_possible_truncation,
    reason = "paragraph indents are small pt/em values"
)]
pub(crate) fn par_style_from_styles(styles: StyleChain<'_>) -> IrParStyle {
    let amount = styles.get(ParElem::first_line_indent).amount();
    let size_pt = styles.resolve(TextElem::size).to_pt();
    let abs_pt = amount.abs.to_pt();
    let em = amount.em.get();
    let twips = ((abs_pt + em * size_pt) * 20.0).round() as i64;
    let chars = (em != 0.0 && abs_pt == 0.0).then(|| (em * 100.0).round() as i64);
    IrParStyle {
        first_line_indent: Some(IrFirstLineIndent { twips, chars }),
        justify: Some(styles.get(ParElem::justify)),
    }
}

/// `#set par(hanging-indent:)` — read from the element's `StyleChain` so rules
/// scoped inside `#let`/function bodies still apply to the paragraphs they
/// create (the source-AST pass only sees top-level set rules).
pub(crate) fn hanging_indent_from_styles(styles: StyleChain<'_>) -> Option<HangingIndent> {
    let hanging = styles.resolve(ParElem::hanging_indent);
    if hanging <= Abs::zero() {
        return None;
    }
    Some(HangingIndent::Twips(crate::convert::page::pt_to_twips(
        hanging.to_pt(),
    )))
}

/// Absolute twips for a `#v()`/`#h()` spacing amount (`em` resolved against the
/// current font size). `Fr` spacing and the ratio component cannot be resolved
/// without the parent extent and return `None`.
pub(crate) fn spacing_twips(amount: &Spacing, styles: StyleChain<'_>) -> Option<u32> {
    let Spacing::Rel(rel) = amount else {
        return None;
    };
    let abs = rel.abs.at(styles.resolve(TextElem::size));
    if abs <= Abs::zero() {
        return None;
    }
    Some(crate::convert::page::pt_to_twips(abs.to_pt()))
}

pub(crate) fn alignment_from_styles(styles: StyleChain<'_>) -> Option<Alignment> {
    match styles.get(AlignElem::alignment).x() {
        Some(HAlignment::Center) => Some(Alignment::Center),
        Some(HAlignment::Right | HAlignment::End) => Some(Alignment::Right),
        Some(HAlignment::Left | HAlignment::Start) | None => None,
    }
}
