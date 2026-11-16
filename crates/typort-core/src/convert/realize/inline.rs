//! Inline collection: a paragraph body (or any inline `Content`) ->
//! `Vec<IrInline>`.
//!
//! The body is re-realized as a fragment (the same `realize_fragment`
//! typst-html uses) so every run arrives with its resolved `StyleChain`:
//! `#Set` children are style changes, not text, and the fragment turns
//! them into per-run styles. Smart quotes are resolved with Typst's own
//! `SmartQuoter`, as the legacy path does. Footnote, reference, citation
//! and link markers are read from the fragment's tag stream; their
//! rendered text is the following link-marker text.

use std::collections::HashMap;

use crate::convert::footnote::parse_circled_number;
use crate::convert::page::counter_page_label;
use typort_ir::{Degradation, DegradationAction, IrImage, IrInline, IrParagraph};
use typort_ooxml::document::Alignment;
use typst_library::diag::SourceResult;
use typst_library::engine::Engine;
use typst_library::foundations::{Content, ContextElem, Packed, Smart, StyleChain};
use typst_library::introspection::{Location, SplitLocator, Tag, TagElem};
use typst_library::layout::{AlignElem, BoxElem, HAlignment};
use typst_library::math::EquationElem;
use typst_library::model::{
    CiteElem, CiteGroup, Destination, FootnoteBody, FootnoteElem, LinkElem, LinkTarget, Numbering,
    ParElem, RefElem, RefForm,
};
use typst_library::routines::Pair;
use typst_library::text::{
    Lang, LinebreakElem, Region, SmartQuoteElem, SmartQuoter, SmartQuotes, SpaceElem, TextElem,
};
use typst_library::visualize::ImageElem;
use typst_library::visualize::ImageKind;
use typst_syntax::Source;
use typst_syntax::Span;

use super::style::{StyleDefaults, lower_runs_match, run_from_styles};
use super::with_fragment;
use crate::convert::image::hash_bytes;
use crate::convert::table_align::paint_to_shd_hex;

/// Shared state for one inline walk (one paragraph body).
pub struct InlineCtx<'a> {
    /// Document style defaults (for per-run override decisions).
    pub defaults: &'a StyleDefaults,
    /// Stateful quote resolver, fresh per paragraph body.
    pub quoter: SmartQuoter,
    /// The last character emitted, for quote open/close decisions.
    pub prev_char: Option<char>,
    /// Labelled-element locations -> bookmark names (pass 1).
    pub label_map: &'a HashMap<Location, String>,
    /// Bibliography-entry locations -> `"bib-n"` bookmark names (pass 1).
    pub bib_map: &'a HashMap<Location, String>,
    /// Footnote markers in document order: (marker location, rendered number).
    pub footnote_markers: &'a mut Vec<(Location, String)>,
    /// Degradations recorded while collecting.
    pub degradations: &'a mut Vec<Degradation>,
    /// The document's main source, for source-AST recovery of page
    /// references (see the `ContextElem` branch in `handle_tag_start`).
    pub source: Option<&'a Source>,
    /// Block equations in this fragment keep their `<m:oMathPara>` wrapper
    /// as an inline Math instead of being dropped: set when collecting a
    /// footnote body, whose single paragraph carries both text and display
    /// math (the legacy shape embeds the oMathPara in the same `<w:p>`).
    pub block_math_as_inline: bool,
    /// Footnote entry bodies collected from content, keyed by marker
    /// location (see the `FootnoteElem` branch in `handle_tag_start`).
    pub footnote_bodies: &'a mut HashMap<Location, Vec<IrParagraph>>,
    /// Whether any footnote uses circled numbering (①②③), from the
    /// footnote elements' numbering patterns.
    pub footnote_circled: &'a mut bool,
}

/// The document language/region for smart quotes, read from the root
/// `StyleChain` (the same source the legacy path's `smart_quote_lang`
/// reads, via `doc.style`).
pub fn smart_quotes_for(styles: StyleChain<'_>) -> (Lang, Option<Region>) {
    (styles.get(TextElem::lang), styles.get(TextElem::region))
}

/// Collect the inlines of one paragraph body.
///
/// `content` is the `ParElem` body (or any inline `Content`); `styles` is
/// the pair's `StyleChain`. The body is fragment-re-realized so runs carry
/// resolved styles; the fragment pairs are walked in order.
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
        |engine, locator, pairs| Ok(collect_fragment_inlines(pairs, engine, locator, ctx)),
    )
}

/// Walk realized fragment pairs into inlines.
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
            match &tagged.tag {
                Tag::Start(inner, _) => {
                    i = handle_tag_start(pairs, i, inner, engine, locator, ctx, &mut out);
                }
                Tag::End(_, _, _) => {
                    i += 1;
                }
            }
            continue;
        }
        if let Some(par) = content.to_packed::<ParElem>() {
            // A paragraph pair inside a fragment: a block-level body
            // realized as a fragment wraps its text in a paragraph, so
            // the text lives in `par.body`, not in a text pair. Collect
            // the body inlines (a fresh fragment realize of the inline
            // body).
            match collect_inlines(&par.body, styles, engine, locator, ctx) {
                Ok(inlines) => out.extend(inlines),
                Err(_) => ctx.degradations.push(Degradation {
                    span: None,
                    construct: "paragraph-in-fragment",
                    action: DegradationAction::Skipped,
                    message:
                        "a paragraph inside a fragment could not be realized; its text was dropped"
                            .to_string(),
                }),
            }
            i += 1;
            continue;
        }
        if let Some(text) = content.to_packed::<TextElem>() {
            push_text(&mut out, &text.text, styles, Some(content.span()), ctx);
            i += 1;
            continue;
        }
        if let Some(sq) = content.to_packed::<SmartQuoteElem>() {
            let double = sq.double.get(styles);
            // Quotes follow the pair's own language: a `#set text(lang:)`
            // inside a paragraph changes the quote glyphs (`« »` for
            // French, `„ “` for German), so the dictionary is derived
            // per pair, not once from the root chain.
            let (lang, region) = smart_quotes_for(styles);
            let quotes = SmartQuotes::get(&Smart::Auto, lang, region, false);
            let quote = ctx.quoter.quote(ctx.prev_char, &quotes, double);
            let quote = quote.to_string();
            ctx.prev_char = quote.chars().next_back();
            out.push(IrInline::Text(run_from_styles(
                &quote,
                styles,
                ctx.defaults,
                Some(content.span()),
                ctx.degradations,
            )));
            i += 1;
            continue;
        }
        if content.to_packed::<SpaceElem>().is_some() {
            out.push(IrInline::Space);
            ctx.prev_char = Some(' ');
            i += 1;
            continue;
        }
        if content.to_packed::<LinebreakElem>().is_some() {
            out.push(IrInline::LineBreak);
            ctx.prev_char = Some('\n');
            i += 1;
            continue;
        }
        if let Some(equation) = content.to_packed::<EquationElem>() {
            if !equation.block.get(styles) {
                let omml = typort_math::equation_to_omml(content);
                out.push(IrInline::Math { omml });
                ctx.prev_char = Some('\u{FFFC}');
            }
            i += 1;
            continue;
        }
        if let Some(image) = content.to_packed::<ImageElem>() {
            if let Some(ir) = image_inline(image, styles, engine, ctx) {
                out.push(ir);
                ctx.prev_char = Some('\u{FFFC}');
            }
            i += 1;
            continue;
        }
        if let Some(box_elem) = content.to_packed::<BoxElem>() {
            collect_box_inlines(box_elem, styles, engine, locator, ctx, &mut out);
            i += 1;
            continue;
        }
        // Horizontal spacing, vertical spacing, paragraph breaks and any
        // other non-inline element inside a fragment: skip. Their text (if
        // any) is already in the fragment's text pairs.
        i += 1;
    }
    out
}

/// Collect an inline box's body inlines, shading its text runs with
/// the box's fill (the legacy `w:shd` for `box(fill: ...)`).
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

/// Handle a `Tag::Start` at `pairs[i]`; returns the next index to process.
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
    // The location lives on the tagged element (`inner`), not on the
    // `TagElem` wrapper: `TagElem` is not `Locatable`, so the pair
    // content's own `location()` is always `None` here.
    let Some(location) = inner.location() else {
        return i + 1;
    };
    if inner.to_packed::<ContextElem>().is_some() {
        // A `context` tag is transparent for most expressions (the
        // rendered text is collected as usual), but
        // `counter(page).at(<label>)` is a page reference to a labelled
        // location: emit a PAGEREF field to that label's bookmark, with
        // the rendered page number as the cached display text. The tag
        // carries only the rendered number; the label is recovered from
        // the source AST via the span, mirroring the legacy
        // `handle_inline_context_tag`. Anything the resolver cannot
        // identify falls through to plain-text collection.
        if let Some(label) = ctx
            .source
            .and_then(|source| counter_page_label(source, inner.span()))
        {
            let end = find_end(pairs, i + 1, location);
            let display = marker_text_in(&pairs[i + 1..end]);
            ctx.prev_char = display.chars().next_back().or(ctx.prev_char);
            out.push(IrInline::Ref {
                target_label: label,
                display_text: display,
                page_ref: true,
            });
            return end + 1;
        }
    }
    if let Some(image) = inner.to_packed::<ImageElem>() {
        // An inline image arrives as a tag in fragments; without this
        // branch it falls through to the skip below and is dropped (the
        // bare-pair branch in `collect_fragment_inlines` only sees
        // non-tag images). This covers images mid-paragraph and images
        // inside footnote bodies, which are collected from content.
        if let Some(ir) = image_inline(image, styles, engine, ctx) {
            out.push(ir);
            ctx.prev_char = Some('\u{FFFC}');
        }
        return skip_to_end(pairs, i + 1, location);
    }
    if let Some(fn_elem) = inner.to_packed::<FootnoteElem>() {
        // The footnote numbering format (e.g. circled ①②③) is declared
        // on the element itself: apply its pattern to 1 and test the
        // result, reusing the legacy `parse_circled_number` predicate.
        // The collected marker text cannot be used — in fragments the
        // counter never resolves, so markers fall back to plain ordinals.
        if let Numbering::Pattern(pattern) = fn_elem.numbering.get_ref(styles)
            && pattern
                .apply(None, &[1])
                .is_ok_and(|text| parse_circled_number(text.trim()).is_some())
        {
            *ctx.footnote_circled = true;
        }
        // The marker text is the footnote's rendered number — except in
        // a fragment realization, where the counter never resolves and
        // the marker renders "0" (fragments realize once; the main
        // document iterates to convergence). "0" is never a real
        // footnote number, so fall back to the ordinal there.
        let rendered = match next_marker_text(pairs, i + 1) {
            Some(text) if text != "0" => text,
            _ => (ctx.footnote_markers.len() + 1).to_string(),
        };
        ctx.footnote_markers.push((location, rendered.clone()));
        // The entry body lives on the element itself (the tag group
        // carries only the marker); collect it from content, with a
        // fresh quote state and block equations kept inline. Frame text
        // alone would render math as literal Mathematical-Alphanumeric
        // glyphs and drop hyperlink display text, so the collected body
        // is preferred over the Paged-frame body in the patch; a failed
        // or empty collection leaves the frame body as the fallback.
        let mut body_ctx = InlineCtx {
            defaults: ctx.defaults,
            quoter: SmartQuoter::new(),
            prev_char: None,
            label_map: ctx.label_map,
            bib_map: ctx.bib_map,
            footnote_markers: &mut *ctx.footnote_markers,
            degradations: &mut *ctx.degradations,
            source: ctx.source,
            block_math_as_inline: true,
            footnote_bodies: &mut *ctx.footnote_bodies,
            footnote_circled: &mut *ctx.footnote_circled,
        };
        // A `Reference` body points at another footnote's entry by label;
        // it has no content of its own, so it keeps the frame body.
        if let FootnoteBody::Content(body) = &fn_elem.body
            && let Ok(inlines) = collect_inlines(body, styles, engine, locator, &mut body_ctx)
            && !inlines.is_empty()
        {
            ctx.footnote_bodies.insert(
                location,
                vec![IrParagraph {
                    inlines,
                    ..IrParagraph::default()
                }],
            );
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
        // A citation (`@key`) arrives in fragments as a `ref` tag, not a
        // `cite-group` tag: its target is a bibliography key, never a
        // document label. A real cross-reference's target is a label pass
        // 1 recorded; a citation key is not. (A document label that
        // happens to spell a bibliography key would be misread —
        // pathological.) The rendered marker is the whole text up to the
        // tag end ("[1]", or "[1], [2]" for a group) — the first text
        // pair alone is just "[".
        if !ctx.label_map.values().any(|label| label == &target_label) {
            let end = find_end(pairs, i + 1, location);
            let rendered = marker_text_in(&pairs[i + 1..end]);
            ctx.prev_char = rendered.chars().next_back().or(ctx.prev_char);
            out.push(IrInline::Cite {
                keys: vec![target_label],
                rendered,
                // No link yet: the bibliography entries (and their
                // "bib-n" bookmarks) are not collected from fragments —
                // see the bibliography-entries gap in the fallback
                // registry — so there is nothing to link to.
                target: None,
            });
            return end + 1;
        }
        let display_text = next_marker_text(pairs, i + 1).unwrap_or_default();
        ctx.prev_char = display_text.chars().next_back().or(ctx.prev_char);
        out.push(IrInline::Ref {
            target_label,
            display_text,
            page_ref: reference.form.get(styles) == RefForm::Page,
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
        let rendered = next_marker_text(pairs, i + 1).unwrap_or_default();
        // The citation renders as a link to the entry; resolve its
        // location to the entry's "bib-n" bookmark via the pass-1 map.
        let target =
            link_location_after(pairs, i + 1).and_then(|loc| ctx.bib_map.get(&loc).cloned());
        ctx.prev_char = rendered.chars().next_back().or(ctx.prev_char);
        out.push(IrInline::Cite {
            keys,
            rendered,
            target,
        });
        return skip_to_end(pairs, i + 1, location);
    }
    if let Some(equation) = inner.to_packed::<EquationElem>() {
        // Inline math arrives as a tag in fragments (the block form is
        // collected at the block level); without this branch the tag is
        // skipped and the math internals are unknown elements, so the
        // whole equation is dropped. A footnote body is collected as a
        // single paragraph, so its display equations are kept here as
        // inline Math (their OMML already carries the oMathPara wrapper)
        // instead of being dropped; see `block_math_as_inline`.
        if !equation.block.get(styles) || ctx.block_math_as_inline {
            let omml = typort_math::equation_to_omml(inner);
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
            // An internal link whose target has no label (or a page
            // position): keep the text, drop the link.
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
    // Any other tag (a `PdfMarkerTag`, a heading tag inside a fragment,
    // …): its content is consumed by the enclosing element's handler, or
    // is inline text already present as a text pair — skip the tag itself.
    i + 1
}

/// The rendered marker text following a footnote/reference/citation tag:
/// the first text pair after `start` (tags and style-only pairs skipped).
fn next_marker_text(pairs: &[Pair<'_>], start: usize) -> Option<String> {
    for (content, _) in &pairs[start..] {
        if let Some(tagged) = content.to_packed::<TagElem>() {
            if matches!(tagged.tag, Tag::End(_, _, _)) {
                return None;
            }
            continue;
        }
        if let Some(text) = content.to_packed::<TextElem>() {
            return Some(text.text.to_string());
        }
    }
    None
}

/// All rendered text in a slice of pairs (tag pairs skipped): the full
/// marker of a citation tag group ("[1]", "[1], [2]").
fn marker_text_in(pairs: &[Pair<'_>]) -> String {
    let mut out = String::new();
    for (content, _) in pairs {
        if let Some(text) = content.to_packed::<TextElem>() {
            out.push_str(&text.text);
        }
    }
    out
}

/// The location a citation's link points at: the first `LinkElem` tag
/// after `start` whose destination is a `Location` (the bibliography
/// entry). Stops at the enclosing tag's end.
fn link_location_after(pairs: &[Pair<'_>], start: usize) -> Option<Location> {
    for (content, _) in &pairs[start..] {
        let Some(tagged) = content.to_packed::<TagElem>() else {
            continue;
        };
        match &tagged.tag {
            Tag::End(_, _, _) => return None,
            Tag::Start(inner, _) => {
                if let Some(link) = inner.to_packed::<LinkElem>()
                    && let LinkTarget::Dest(Destination::Location(loc)) = &link.dest
                {
                    return Some(*loc);
                }
            }
        }
    }
    None
}

/// Find the index of the `Tag::End` matching the `Tag::Start` at `start`
/// (same location), or the end of the slice when absent.
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

/// Skip forward to just past the matching `Tag::End` for `location`.
fn skip_to_end(pairs: &[Pair<'_>], start: usize, location: Location) -> usize {
    find_end(pairs, start, location) + 1
}

/// Push a text run, merging it into the previous run when the format
/// matches (the fragment splits runs at style boundaries the lowering
/// would otherwise have to re-merge).
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
    let run = run_from_styles(text, styles, ctx.defaults, span, ctx.degradations);
    ctx.prev_char = run.text.chars().next_back();
    if let Some(IrInline::Text(prev)) = out.last_mut()
        && lower_runs_match(prev, &run)
    {
        prev.text.push_str(&run.text);
        return;
    }
    out.push(IrInline::Text(run));
}

/// An inline image: decode it, rasterizing SVGs and re-encoding GIF/WebP
/// to PNG (as the legacy writer does), so the lowering only ever sees PNG
/// or JPEG bytes. Display size stays zero here; the patch step fills it
/// from the Paged `image_sizes` map.
/// Decode one image element into an `IrInline::Image` (raster bytes kept
/// as-is for PNG/JPEG, re-encoded to PNG otherwise; SVGs rasterized).
/// Display sizes stay zero here: the companion-Paged patch fills them in
/// by content hash.
pub(crate) fn image_inline(
    image: &Packed<ImageElem>,
    styles: StyleChain<'_>,
    engine: &mut Engine,
    ctx: &mut InlineCtx<'_>,
) -> Option<IrInline> {
    let Ok(decoded) = image.decode(engine, styles) else {
        return None;
    };
    let (bytes, mime, source_hash) = match decoded.kind() {
        ImageKind::Raster(raster) => {
            let original = raster.data().as_slice();
            let source_hash = hash_bytes(original);
            let bytes = original.to_vec();
            if is_jpeg(&bytes) {
                (bytes, Some("image/jpeg".to_string()), source_hash)
            } else if is_png(&bytes) {
                (bytes, Some("image/png".to_string()), source_hash)
            } else {
                // GIF, WebP and anything else Word cannot embed: re-encode.
                let png = reencode_png(&bytes)?;
                (png, Some("image/png".to_string()), source_hash)
            }
        }
        ImageKind::Svg(svg) => {
            let original = svg.data().as_slice();
            let png = rasterize_svg(original)?;
            (png, Some("image/png".to_string()), hash_bytes(original))
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
    }))
}

/// Whether `bytes` are a PNG (magic `89 50 4E 47`).
fn is_png(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes[..4] == [0x89, 0x50, 0x4E, 0x47]
}

/// Whether `bytes` are a JPEG (magic `FF D8 FF`).
fn is_jpeg(bytes: &[u8]) -> bool {
    bytes.len() >= 3 && bytes[..3] == [0xFF, 0xD8, 0xFF]
}

/// Re-encode arbitrary raster bytes to PNG via the `image` crate.
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

/// Rasterize SVG bytes to PNG via resvg (the legacy writer's path).
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

/// Collect a paragraph's inlines from a `ParElem` pair.
///
/// The alignment comes from the pair's style chain: the `AlignElem`
/// alignment in scope (`#set align(...)`, `#align(...)`, the figure
/// caption's centered show rule), or justified when `ParElem::justify`
/// is set. `Start`/`Left` map to `None` — Word's default — so a plain
/// paragraph carries no `jc`.
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
        ..IrParagraph::default()
    })
}

/// A paragraph's alignment from its style chain: the `AlignElem`
/// alignment in scope (`#set align(...)`, `#align(...)`, the figure
/// caption's centered show rule). `Start`/`Left` map to `None` — Word's
/// default — so a plain paragraph carries no `jc`. Justification
/// (`ParElem::justify`) is deliberately NOT mapped here: it is a
/// document-level default that the shared style pass writes into the
/// Normal style (`body_alignment`), not a per-paragraph property.
pub(crate) fn alignment_from_styles(styles: StyleChain<'_>) -> Option<Alignment> {
    match styles.get(AlignElem::alignment).x() {
        Some(HAlignment::Center) => Some(Alignment::Center),
        Some(HAlignment::Right | HAlignment::End) => Some(Alignment::Right),
        Some(HAlignment::Left | HAlignment::Start) | None => None,
    }
}
