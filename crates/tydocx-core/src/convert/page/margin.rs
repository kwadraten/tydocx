use codex::numeral_systems::NamedNumeralSystem;
use tydocx_ooxml::document::{
    Alignment, FirstLineIndent, HeaderFooter, InlineElement, PageNumberFormat, PageSettings,
    Paragraph, Run,
};
use typst::layout::{Frame, FrameItem};
use typst_layout::PagedDocument;
use typst_library::model::Numbering;

use crate::compat::{Degradation, DegradationAction};

use super::furniture::{FurnitureMaps, PageFurniture};
use super::run_style::effective_weight;
use super::units::pt_to_twips;

/// Fraction of each margin retained as a guard between body and margin content.
const BODY_ZONE_MARGIN_RATIO: f64 = 0.9;
/// Fraction of page center used to recognize centered margin text.
const MARGIN_CENTER_TOLERANCE_RATIO: f64 = 0.15;

/// A text fragment extracted from a page frame, with position info.
pub(super) struct TextFragment {
    pub(super) y: f64,
    pub(super) x: f64,
    pub(super) text: String,
    /// Rendered font size in half-points; preserved on header/footer runs.
    pub(super) size_half_pt: u32,
    /// Rendered font weight ≥ 600; preserved on header/footer runs.
    pub(super) bold: bool,
}

/// Collect text fragments with absolute positions from a frame.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "font sizes are small non-negative values"
)]
pub(super) fn collect_text_fragments(frame: &Frame, items: &mut Vec<TextFragment>) {
    super::super::frames::visit_frame_items(frame, false, &mut |position, item| {
        if let FrameItem::Text(text_item) = item {
            let text = text_item.text.to_string();
            if !text.is_empty() {
                items.push(TextFragment {
                    y: position.y.to_pt(),
                    x: position.x.to_pt(),
                    text,
                    size_half_pt: (text_item.size.to_pt() * 2.0).round().max(1.0) as u32,
                    bold: effective_weight(&text_item.font) >= 600,
                });
            }
        }
    });
}

/// The tag-bracket kind relevant to zone-text exclusion.
enum ZoneTag {
    /// An `ArtifactKind::Header`/`Footer` bracket — inside it, `place`
    /// brackets are furniture and their text is skipped.
    MarginArtifact,
    /// A `place` bracket inside a margin artifact.
    Place,
    /// Anything else.
    Other,
}

/// Open bracket state for [`collect_fragments_skip_furniture`]: the tag
/// stack plus the counts that decide whether text is furniture.
#[derive(Default)]
struct ZoneTagState {
    stack: Vec<(typst::introspection::Location, ZoneTag)>,
    /// Open `ArtifactKind::Header`/`Footer` brackets.
    artifact_depth: usize,
    /// Open `place` brackets inside a margin artifact.
    place_depth: usize,
}

/// Collect text fragments, skipping the furniture layers listed in
/// `furn.excluded_top` (background/foreground zones at top level) and the
/// contents of `place` tag brackets inside header/footer artifact brackets
/// (which are emitted as `wp:anchor` drawings instead). `place` brackets
/// inside the body keep their historical behavior — their text still counts
/// as zone text when it lands in a margin band.
pub(super) fn collect_fragments_skip_furniture(
    frame: &Frame,
    furn: &PageFurniture,
    items: &mut Vec<TextFragment>,
) {
    let mut state = ZoneTagState::default();
    collect_fragments_at(
        frame,
        typst::layout::Point::zero(),
        furn,
        true,
        &mut state,
        items,
    );
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "font sizes are small non-negative values"
)]
fn collect_fragments_at(
    frame: &Frame,
    offset: typst::layout::Point,
    furn: &PageFurniture,
    top_level: bool,
    state: &mut ZoneTagState,
    out: &mut Vec<TextFragment>,
) {
    for (index, (pos, item)) in frame.items().enumerate() {
        if top_level && furn.excluded_top.iter().any(|r| r.contains(&index)) {
            continue;
        }
        let absolute = offset + *pos;
        match item {
            FrameItem::Group(group) => {
                collect_fragments_at(&group.frame, absolute, furn, false, state, out);
            }
            FrameItem::Tag(typst::introspection::Tag::Start(elem, _)) => {
                let kind = zone_tag_kind(elem);
                match kind {
                    ZoneTag::MarginArtifact => state.artifact_depth += 1,
                    ZoneTag::Place => state.place_depth += 1,
                    ZoneTag::Other => {}
                }
                if let Some(location) = elem.location() {
                    state.stack.push((location, kind));
                }
            }
            FrameItem::Tag(typst::introspection::Tag::End(location, ..)) => {
                if state.stack.last().is_some_and(|(l, _)| l == location)
                    && let Some((_, kind)) = state.stack.pop()
                {
                    match kind {
                        ZoneTag::MarginArtifact => {
                            state.artifact_depth = state.artifact_depth.saturating_sub(1);
                        }
                        ZoneTag::Place => state.place_depth = state.place_depth.saturating_sub(1),
                        ZoneTag::Other => {}
                    }
                }
            }
            FrameItem::Text(text)
                if (state.artifact_depth == 0 || state.place_depth == 0)
                    && !text.text.is_empty() =>
            {
                out.push(TextFragment {
                    y: absolute.y.to_pt(),
                    x: absolute.x.to_pt(),
                    text: text.text.to_string(),
                    size_half_pt: (text.size.to_pt() * 2.0).round().max(1.0) as u32,
                    bold: effective_weight(&text.font) >= 600,
                });
            }
            _ => {}
        }
    }
}

/// Classify a `Tag::Start` element for zone-text exclusion.
fn zone_tag_kind(elem: &typst::foundations::Content) -> ZoneTag {
    use typst_library::pdf::{ArtifactElem, ArtifactKind};
    if let Some(artifact) = elem.to_packed::<ArtifactElem>()
        && matches!(
            artifact.kind.as_option(),
            Some(ArtifactKind::Header | ArtifactKind::Footer)
        )
    {
        return ZoneTag::MarginArtifact;
    }
    if elem.elem().name() == "place" {
        return ZoneTag::Place;
    }
    ZoneTag::Other
}

/// Compute the default Typst margin for a page in pt.
///
/// Typst uses `2.5/21 * min(width, height)` as the default margin.
pub(super) fn default_margin_pt(page_width: f64, page_height: f64) -> f64 {
    let smaller = page_width.min(page_height);
    2.5 / 21.0 * smaller
}

/// Extract page dimensions from the `PagedDocument` and apply to `PageSettings`.
pub fn extract_page_settings(paged: &PagedDocument, settings: &mut PageSettings) {
    let Some(page) = paged.pages().first() else {
        return;
    };
    let w = page.frame.width().to_pt();
    let h = page.frame.height().to_pt();
    settings.width_twips = pt_to_twips(w);
    settings.height_twips = pt_to_twips(h);

    let default_margin = default_margin_pt(w, h);
    let default_twips = pt_to_twips(default_margin);
    settings.margin_top = default_twips;
    settings.margin_bottom = default_twips;
    settings.margin_left = default_twips;
    settings.margin_right = default_twips;
}

/// Identify the body content zone using margin-based boundaries.
///
/// Resolved top/bottom page margins in pt — the same values the docx writes to
/// `w:pgMar` (paged default overridden by `#set page(margin:)` from the source
/// AST). Body-zone consumers take this so their header/footer boundary tracks
/// the document's actual margins: with author margins smaller than Typst's
/// default, using the default boundary silently drops real body content.
#[derive(Clone, Copy)]
pub struct MarginsPt {
    pub top: f64,
    pub bottom: f64,
    /// Left/right are needed by the furniture zone classifier (the inner
    /// frame sits at `(margin.left, margin.top)`).
    pub left: f64,
    pub right: f64,
}

impl MarginsPt {
    /// Read the resolved margins from the document's `PageSettings`.
    #[must_use]
    pub fn from_settings(settings: &PageSettings) -> Self {
        Self {
            top: f64::from(settings.margin_top) / 20.0,
            bottom: f64::from(settings.margin_bottom) / 20.0,
            left: f64::from(settings.margin_left) / 20.0,
            right: f64::from(settings.margin_right) / 20.0,
        }
    }
}

/// Typst renders headers in the top margin area and footers in the
/// bottom margin area. We use the actual margins as the boundary
/// to separate these zones.
///
/// `margin_top_pt` and `margin_bottom_pt`: actual margins in pt. If `None`,
/// falls back to Typst's default margin (`2.5/21 * min(w, h)`).
///
/// Returns `(body_top, body_bottom)` in pt — the y-range of the body zone.
pub(in crate::convert) fn find_body_zone(
    page_width: f64,
    page_height: f64,
    margin_top_pt: Option<f64>,
    margin_bottom_pt: Option<f64>,
) -> (f64, f64) {
    let default_margin = default_margin_pt(page_width, page_height);
    let mt = margin_top_pt.unwrap_or(default_margin);
    let mb = margin_bottom_pt.unwrap_or(default_margin);
    // Body starts at the top margin line and ends at the bottom margin line.
    // Headers are positioned at margin * (1 - header_ascent) where header_ascent
    // defaults to 0.3, so header text is at ~margin * 0.7 from top.
    // Use margin * 0.9 as the boundary to safely include all header content
    // above the body zone.
    let body_top = mt * BODY_ZONE_MARGIN_RATIO;
    let body_bottom = page_height - mb * BODY_ZONE_MARGIN_RATIO;
    (body_top, body_bottom)
}

/// Which page-margin band holds the content: top (headers) or bottom (footers).
#[derive(Clone, Copy)]
pub enum MarginZone {
    Top,
    Bottom,
}

/// Every header/footer part extracted from one margin zone across the
/// document's pages.
///
/// Typst evaluates `page(header:)`/`page(footer:)` per page in `context`, so
/// the same declared content can render differently per page. Three shapes
/// map onto OOXML header references: uniform content (`w:type="default"`
/// only), odd-vs-even content (`w:type="even"` + `w:evenAndOddHeaders`), and
/// a first page that differs from a uniform rest (`w:type="first"` +
/// `w:titlePg`). Anything else keeps the first page's content and reports the
/// first unrepresentable page.
#[must_use]
#[derive(Default)]
pub struct ZoneParts {
    /// `w:type="default"` part (odd pages under odd/even, otherwise all pages).
    pub default: Option<HeaderFooter>,
    /// `w:type="even"` part.
    pub even: Option<HeaderFooter>,
    /// `w:type="first"` part.
    pub first: Option<HeaderFooter>,
    /// Odd and even pages carried different content.
    pub even_and_odd: bool,
    /// Page 1's content differed from a uniform rest.
    pub title_page: bool,
    /// 1-based page index where content first differed in a way none of the
    /// supported reference shapes cover — the default (page-1) part is kept.
    pub unrepresentable_page: Option<usize>,
}

/// The page-number split of a zone's representative page for verification:
/// consecutive pages inside one parity/first-vs-rest group must show the
/// token incremented by the physical page distance within that group.
fn zone_text_at(
    paged: &PagedDocument,
    zone: MarginZone,
    margins: MarginsPt,
    page_index: usize,
    furn: Option<&PageFurniture>,
) -> String {
    let Some(page) = paged.pages().get(page_index) else {
        return String::new();
    };
    zone_fragments(&page.frame, zone, margins, furn)
        .iter()
        .map(|f| f.text.as_str())
        .collect()
}

/// Extract all header/footer parts of one margin zone. `declared` is the
/// `#set page(numbering:)` format, trusted to identify a page-number token
/// when a group has only one page to check. `maps` (C2/C3 only) carries the
/// furniture analysis: background/foreground text is excluded from the zone
/// text and `#place` units become anchors appended to the part.
pub fn extract_zone_parts(
    paged: &PagedDocument,
    zone: MarginZone,
    margins: MarginsPt,
    declared: Option<PageNumberFormat>,
    maps: Option<&FurnitureMaps>,
) -> ZoneParts {
    let page_count = paged.pages().len();
    if page_count == 0 {
        return ZoneParts::default();
    }
    let furn_at = |i: usize| maps.and_then(|m| m.per_page.get(i));
    let texts: Vec<String> = (0..page_count)
        .map(|i| zone_text_at(paged, zone, margins, i, furn_at(i)))
        .collect();
    let norm: Vec<String> = texts
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let mut n = normalize_zone_text_for_comparison(t);
            if let Some(furn) = furn_at(i) {
                let sig = super::furniture::zone_sig(furn, zone);
                if !sig.is_empty() {
                    n.push('\u{1F}');
                    n.push_str(sig);
                }
            }
            n
        })
        .collect();
    if norm.iter().all(String::is_empty) {
        return ZoneParts::default();
    }

    let first = &norm[0];
    if norm.iter().all(|n| n == first) {
        let indices: Vec<usize> = (0..page_count.min(3)).collect();
        return ZoneParts {
            default: extract_margin_zone_on(
                paged,
                zone,
                margins,
                declared,
                0,
                &rest_after(&texts, &indices),
                furn_at(0),
            ),
            ..ZoneParts::default()
        };
    }

    classify_zone_parts(paged, zone, margins, declared, &texts, &norm, maps)
}

/// Consecutive same-group pages for page-number verification:
/// `(text, value delta from the representative page)`.
fn rest_after(texts: &[String], indices: &[usize]) -> Vec<(String, u32)> {
    indices
        .iter()
        .skip(1)
        .take(2)
        .map(|&i| {
            (
                texts[i].clone(),
                u32::try_from(i - indices[0]).unwrap_or(u32::MAX),
            )
        })
        .collect()
}

/// Does a part carry a `PAGE` field? Then a lone group page can still be
/// trusted as a page-number token without a declared format.
fn has_page_field(hf: Option<&HeaderFooter>) -> bool {
    hf.is_some_and(|hf| {
        hf.paragraphs
            .iter()
            .flat_map(|p| &p.inlines)
            .any(|i| matches!(i, tydocx_ooxml::document::InlineElement::FieldPage { .. }))
    })
}

/// The three representable variance shapes, in priority order: page 1 vs a
/// uniform rest (`w:titlePg`), odd vs even (`w:evenAndOddHeaders`), else the
/// first page's part with the first unrepresentable page reported.
fn classify_zone_parts(
    paged: &PagedDocument,
    zone: MarginZone,
    margins: MarginsPt,
    declared: Option<PageNumberFormat>,
    texts: &[String],
    norm: &[String],
    maps: Option<&FurnitureMaps>,
) -> ZoneParts {
    let page_count = norm.len();
    let extract = |index: usize, rest: &[(String, u32)], declared| {
        extract_margin_zone_on(
            paged,
            zone,
            margins,
            declared,
            index,
            rest,
            maps.and_then(|m| m.per_page.get(index)),
        )
    };
    let first = &norm[0];
    if norm[1..].iter().all(|n| n == &norm[1]) {
        let rest_indices: Vec<usize> = (1..page_count.min(4)).collect();
        let default = if norm[1].is_empty() {
            None
        } else {
            extract(1, &rest_after(texts, &rest_indices), declared)
        };
        let declared_first = declared
            .or_else(|| has_page_field(default.as_ref()).then_some(PageNumberFormat::Decimal));
        let first_part = if norm[0].is_empty() {
            None
        } else {
            extract(0, &[], declared_first)
        };
        return ZoneParts {
            default,
            first: first_part,
            title_page: true,
            ..ZoneParts::default()
        };
    }

    let odd: Vec<usize> = (0..page_count).step_by(2).collect();
    let even: Vec<usize> = (1..page_count).step_by(2).collect();
    let odd_uniform = odd.iter().all(|&i| norm[i] == *first);
    let even_uniform = even.iter().all(|&i| norm[i] == norm[even[0]]);
    if odd_uniform && even_uniform {
        let default = if norm[0].is_empty() {
            None
        } else {
            extract(0, &rest_after(texts, &odd[..odd.len().min(3)]), declared)
        };
        let declared_even = declared
            .or_else(|| has_page_field(default.as_ref()).then_some(PageNumberFormat::Decimal));
        let even_part = if norm[even[0]].is_empty() {
            None
        } else {
            extract(
                even[0],
                &rest_after(texts, &even[..even.len().min(3)]),
                declared_even,
            )
        };
        return ZoneParts {
            default,
            even: even_part,
            even_and_odd: true,
            ..ZoneParts::default()
        };
    }

    let indices: Vec<usize> = (0..page_count.min(3)).collect();
    let differing = norm.iter().position(|n| n != first).map(|i| i + 1);
    ZoneParts {
        default: extract(0, &rest_after(texts, &indices), declared),
        unrepresentable_page: differing,
        ..ZoneParts::default()
    }
}

/// Extract header/footer content from the margin zone of one page.
///
/// `rest` holds consecutive same-group zone texts with the page-number delta
/// each one should show relative to this page, used to verify a page-number
/// token (see `split_page_number_token`).
fn extract_margin_zone_on(
    paged: &PagedDocument,
    zone: MarginZone,
    margins: MarginsPt,
    declared: Option<PageNumberFormat>,
    page_index: usize,
    rest: &[(String, u32)],
    furn: Option<&PageFurniture>,
) -> Option<HeaderFooter> {
    let page = paged.pages().get(page_index)?;
    let page_width = page.frame.width().to_pt();

    let fragments = zone_fragments(&page.frame, zone, margins, furn);
    let anchors = furn.map_or(&[][..], |f| super::furniture::zone_anchors(f, zone));
    if fragments.is_empty() && anchors.is_empty() {
        return None;
    }

    let text: String = fragments.iter().map(|f| f.text.as_str()).collect();

    let zone_size = fragments
        .first()
        .filter(|first| {
            fragments
                .iter()
                .all(|f| f.size_half_pt == first.size_half_pt)
        })
        .map(|f| f.size_half_pt);
    let zone_bold = !fragments.is_empty() && fragments.iter().all(|f| f.bold);
    let mut para = Paragraph::new();
    if let Some((prefix, token, suffix)) = split_page_number_token(&text, rest, declared) {
        if !prefix.is_empty() {
            let mut run = Run::new(&prefix);
            run.size_half_pt = zone_size;
            run.bold = zone_bold;
            para.push_run(run);
        }
        para.inlines
            .push(tydocx_ooxml::document::InlineElement::FieldPage {
                display_text: token,
                bold: zone_bold,
                size_half_pt: zone_size,
            });
        if !suffix.is_empty() {
            let mut run = Run::new(&suffix);
            run.size_half_pt = zone_size;
            run.bold = zone_bold;
            para.push_run(run);
        }
    } else {
        for item in &fragments {
            let mut run = Run::new(&item.text);
            run.size_half_pt = Some(item.size_half_pt);
            run.bold = item.bold;
            para.push_run(run);
        }
    }

    // Detect alignment — use 15% of page center as threshold to avoid
    // false-positives on left-aligned text that starts near the center.
    let page_center = page_width / 2.0;
    if fragments.iter().all(|f| f.x > page_center) {
        para.alignment = Some(Alignment::Right);
    } else if fragments
        .iter()
        .all(|f| (f.x - page_center).abs() < page_center * MARGIN_CENTER_TOLERANCE_RATIO)
    {
        para.alignment = Some(Alignment::Center);
    }

    let mut paragraphs = Vec::new();
    if !fragments.is_empty() {
        paragraphs.push(para);
    }
    // `#place` units inside the zone become anchored drawings in this part
    // (C2/C3) — each on its own zero-height carrier paragraph.
    for (i, anchor) in anchors.iter().enumerate() {
        paragraphs.push(super::furniture::anchor_paragraph(
            anchor,
            zone,
            u32::try_from(i + 1).unwrap_or(u32::MAX),
        ));
    }
    Some(HeaderFooter { paragraphs })
}

/// Text fragments in one page frame's margin zone, in reading (x) order.
/// `furn` (C2/C3) excludes furniture layers and `#place`-bracket items —
/// both are emitted as anchored drawings instead of flat zone text.
fn zone_fragments(
    frame: &Frame,
    zone: MarginZone,
    margins: MarginsPt,
    furn: Option<&PageFurniture>,
) -> Vec<TextFragment> {
    let page_width = frame.width().to_pt();
    let page_height = frame.height().to_pt();

    let mut fragments = Vec::new();
    match furn {
        Some(furn) => collect_fragments_skip_furniture(frame, furn, &mut fragments),
        None => collect_text_fragments(frame, &mut fragments),
    }

    let (body_top, body_bottom) = find_body_zone(
        page_width,
        page_height,
        Some(margins.top),
        Some(margins.bottom),
    );

    let mut items: Vec<TextFragment> = fragments
        .into_iter()
        .filter(|f| match zone {
            MarginZone::Top => f.y < body_top && f.y > 0.0,
            MarginZone::Bottom => f.y > body_bottom && f.y <= page_height,
        })
        .collect();

    items.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
    items
}

// ---------------------------------------------------------------------------
// Page-number tokens inside mixed margin zones
// ---------------------------------------------------------------------------

/// A number-like token found in a margin-zone text: its byte span, the format
/// it reads as, and its numeric value.
struct TokenCandidate {
    start: usize,
    end: usize,
    fmt: PageNumberFormat,
    value: u32,
}

/// Split a margin-zone text into `(prefix, page-number token, suffix)` when
/// the token is verifiably a page number, else `None`.
///
/// Verification: `rest` is `(text, value delta)` for up to two later pages in
/// the same parity/first-vs-rest group — the token must increment by exactly
/// `delta` on each. With no later page to check, consecutiveness cannot be
/// verified, so only the declared format is trusted — and only for Decimal
/// and Roman (a lone letter is too ambiguous) — and the token must be the
/// only candidate in the text.
///
/// Residual risk (accepted): a section number that happens to increment by
/// exactly the page distance is indistinguishable from a page number by this
/// check.
fn split_page_number_token(
    page1: &str,
    rest: &[(String, u32)],
    declared: Option<PageNumberFormat>,
) -> Option<(String, String, String)> {
    let candidates = find_token_candidates(page1);
    let split = |c: &TokenCandidate| {
        (
            page1[..c.start].to_string(),
            page1[c.start..c.end].to_string(),
            page1[c.end..].to_string(),
        )
    };
    if rest.is_empty() {
        if candidates.len() != 1 {
            return None;
        }
        let c = &candidates[0];
        if declared != Some(c.fmt) {
            return None;
        }
        if !matches!(
            c.fmt,
            PageNumberFormat::Decimal | PageNumberFormat::LowerRoman | PageNumberFormat::UpperRoman
        ) {
            return None;
        }
        return Some(split(c));
    }
    candidates
        .iter()
        .find(|c| {
            let prefix = &page1[..c.start];
            let suffix = &page1[c.end..];
            rest.iter().take(2).all(|(text, delta)| {
                c.value.checked_add(*delta).is_some_and(|v| {
                    text == &format!("{prefix}{}{suffix}", format_page_number(v, c.fmt))
                })
            })
        })
        .map(split)
}

/// Find number-like tokens in a margin-zone text: digit runs, Roman-numeral
/// runs, and standalone single letters. A run counts only when it is not
/// glued to an alphanumeric neighbour (so "2" in "Section2" and "D" in
/// "Draft" are not tokens); a Roman run counts only when it round-trips
/// through `format_page_number` (so "IIII" is not a numeral).
fn find_token_candidates(text: &str) -> Vec<TokenCandidate> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut candidates = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let (byte_idx, c) = chars[i];
        if c.is_ascii_digit() {
            let (candidate, next) = scan_digit_run(&chars, text, i);
            if let Some(candidate) = candidate {
                candidates.push(candidate);
            }
            i = next;
        } else if matches!(c, 'i' | 'v' | 'x' | 'l' | 'c' | 'd' | 'm') {
            let (candidate, next) = scan_roman_run(&chars, text, i, false);
            if let Some(candidate) = candidate {
                candidates.push(candidate);
            }
            i = next;
        } else if matches!(c, 'I' | 'V' | 'X' | 'L' | 'C' | 'D' | 'M') {
            let (candidate, next) = scan_roman_run(&chars, text, i, true);
            if let Some(candidate) = candidate {
                candidates.push(candidate);
            }
            i = next;
        } else if c.is_ascii_alphabetic() {
            // A standalone single letter (a word-boundary-delimited letter
            // that is not a Roman numeral — `i`/`v`/… are caught above).
            let prev = if i == 0 { None } else { Some(chars[i - 1].1) };
            let next = chars.get(i + 1).map(|&(_, c)| c);
            if !prev.is_some_and(|c| c.is_ascii_alphanumeric())
                && !next.is_some_and(|c| c.is_ascii_alphanumeric())
            {
                let end_byte = byte_idx + c.len_utf8();
                let (fmt, value) = if c.is_ascii_lowercase() {
                    (
                        PageNumberFormat::LowerLetter,
                        u32::from(c) - u32::from('a') + 1,
                    )
                } else {
                    (
                        PageNumberFormat::UpperLetter,
                        u32::from(c) - u32::from('A') + 1,
                    )
                };
                candidates.push(TokenCandidate {
                    start: byte_idx,
                    end: end_byte,
                    fmt,
                    value,
                });
            }
            i += 1;
        } else {
            i += 1;
        }
    }
    candidates
}

/// Scan a maximal digit run starting at `i`; returns the candidate (if the
/// run parses and has alnum-free neighbours) and the index just past it.
fn scan_digit_run(
    chars: &[(usize, char)],
    text: &str,
    i: usize,
) -> (Option<TokenCandidate>, usize) {
    let (byte_idx, _) = chars[i];
    let mut j = i;
    while j < chars.len() && chars[j].1.is_ascii_digit() {
        j += 1;
    }
    let end_byte = chars.get(j).map_or(text.len(), |&(b, _)| b);
    let candidate = if neighbours_are_free(chars, i, j)
        && let Ok(value) = text[byte_idx..end_byte].parse::<u32>()
    {
        Some(TokenCandidate {
            start: byte_idx,
            end: end_byte,
            fmt: PageNumberFormat::Decimal,
            value,
        })
    } else {
        None
    };
    (candidate, j)
}

/// Scan a maximal run of Roman-numeral characters starting at `i`; returns
/// the candidate (if the run is a valid numeral with alnum-free neighbours)
/// and the index just past it.
fn scan_roman_run(
    chars: &[(usize, char)],
    text: &str,
    i: usize,
    uppercase: bool,
) -> (Option<TokenCandidate>, usize) {
    let (byte_idx, _) = chars[i];
    let is_roman = |c: char| {
        if uppercase {
            matches!(c, 'I' | 'V' | 'X' | 'L' | 'C' | 'D' | 'M')
        } else {
            matches!(c, 'i' | 'v' | 'x' | 'l' | 'c' | 'd' | 'm')
        }
    };
    let mut j = i;
    while j < chars.len() && is_roman(chars[j].1) {
        j += 1;
    }
    let end_byte = chars.get(j).map_or(text.len(), |&(b, _)| b);
    let fmt = if uppercase {
        PageNumberFormat::UpperRoman
    } else {
        PageNumberFormat::LowerRoman
    };
    let candidate = if neighbours_are_free(chars, i, j) {
        let token = &text[byte_idx..end_byte];
        let value = roman_value(token, uppercase);
        if value > 0 && format_page_number(value, fmt) == token {
            Some(TokenCandidate {
                start: byte_idx,
                end: end_byte,
                fmt,
                value,
            })
        } else {
            None
        }
    } else {
        None
    };
    (candidate, j)
}

/// Whether the run `[i, j)` in `chars` is not glued to an alphanumeric
/// neighbour (so "2" in "Section2" and "D" in "Draft" are not tokens).
fn neighbours_are_free(chars: &[(usize, char)], i: usize, j: usize) -> bool {
    let prev = if i == 0 { None } else { Some(chars[i - 1].1) };
    let next = chars.get(j).map(|&(_, c)| c);
    !prev.is_some_and(|c| c.is_ascii_alphanumeric())
        && !next.is_some_and(|c| c.is_ascii_alphanumeric())
}

/// Format a page number in the given format: `1` → `"1"`, `4` in `LowerRoman`
/// → `"iv"`, `27` in `LowerLetter` → `"aa"` (bijective base-26).
fn format_page_number(value: u32, fmt: PageNumberFormat) -> String {
    match fmt {
        PageNumberFormat::Decimal => value.to_string(),
        PageNumberFormat::LowerRoman => int_to_roman(value).to_lowercase(),
        PageNumberFormat::UpperRoman => int_to_roman(value),
        PageNumberFormat::LowerLetter => int_to_letters(value, false),
        PageNumberFormat::UpperLetter => int_to_letters(value, true),
    }
}

/// Format a value as an uppercase Roman numeral (`""` for 0).
fn int_to_roman(mut value: u32) -> String {
    const TABLE: [(u32, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut out = String::new();
    for (v, sym) in TABLE {
        while value >= v {
            out.push_str(sym);
            value -= v;
        }
    }
    out
}

/// Format a value as bijective base-26 letters (`1` → `a`, `26` → `z`,
/// `27` → `aa`); `""` for 0.
fn int_to_letters(mut value: u32, upper: bool) -> String {
    let base = if upper { b'A' } else { b'a' };
    let mut out = Vec::new();
    while value > 0 {
        value -= 1;
        out.push(base + u8::try_from(value % 26).unwrap_or(0));
        value /= 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// Map a resolved `page(numbering:)` value to a `PageNumberFormat`. Mirrors
/// [`page_number_format_from_declaration`] for a compiled `Numbering` (from a
/// realize-pair `StyleChain` or a paged `Page`), not a source string.
pub(in crate::convert) fn numbering_page_format(
    numbering: Option<&Numbering>,
) -> Option<PageNumberFormat> {
    let Some(Numbering::Pattern(pattern)) = numbering else {
        return None;
    };
    if pattern.pieces.len() != 1 || !pattern.pieces[0].0.is_empty() || !pattern.suffix.is_empty() {
        return None;
    }
    match pattern.pieces[0].1 {
        NamedNumeralSystem::Arabic => Some(PageNumberFormat::Decimal),
        NamedNumeralSystem::LowerRoman => Some(PageNumberFormat::LowerRoman),
        NamedNumeralSystem::UpperRoman => Some(PageNumberFormat::UpperRoman),
        NamedNumeralSystem::LowerLatin => Some(PageNumberFormat::LowerLetter),
        NamedNumeralSystem::UpperLatin => Some(PageNumberFormat::UpperLetter),
        _ => None,
    }
}

/// Map a declared `#set page(numbering:)` pattern to a `PageNumberFormat`.
/// Only the plain single-symbol patterns have an OOXML equivalent: `"1"`
/// (decimal), `"i"`/`"I"` (roman), `"a"`/`"A"` (letter). Anything else — a
/// function, a decorated pattern (`"1."`, `"(1)"`, `"1.1"`), a zero-padded
/// pattern (`"01"`) — has no `w:pgNumType` equivalent and returns `None`
/// (the caller falls back to glyph detection and reports a degradation).
#[must_use]
pub fn page_number_format_from_declaration(pattern: &str) -> Option<PageNumberFormat> {
    match pattern {
        "1" => Some(PageNumberFormat::Decimal),
        "i" => Some(PageNumberFormat::LowerRoman),
        "I" => Some(PageNumberFormat::UpperRoman),
        "a" => Some(PageNumberFormat::LowerLetter),
        "A" => Some(PageNumberFormat::UpperLetter),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Page number detection
// ---------------------------------------------------------------------------

/// Check if the footer text on the first page looks like a page number.
///
/// Returns `Some(PageNumberFormat)` if the footer is just a page number
/// (e.g. "1", "i", "I", "a", "A"), otherwise `None`.
///
/// We check multiple pages to confirm — if different pages have different
/// consecutive numbers, it's definitely a page number rather than static text.
/// A single footer "i" (a word) or "5" (a static label) would be misclassified
/// without multi-page verification.
#[must_use]
pub fn detect_page_numbering(
    paged: &PagedDocument,
    margins: MarginsPt,
    maps: Option<&FurnitureMaps>,
) -> Option<PageNumberFormat> {
    if paged.pages().is_empty() {
        return None;
    }

    // Extract footer text from the first page
    let first_footer =
        extract_footer_text_from_page(&paged.pages()[0].frame, margins, furn_at(maps, 0))?;
    let first_trimmed = first_footer.trim();

    // Try to classify the text as a page number format
    let fmt = classify_page_number(first_trimmed)?;

    // If we have a second page, verify consecutiveness to avoid false positives
    if paged.pages().len() >= 2 {
        let second_footer =
            extract_footer_text_from_page(&paged.pages()[1].frame, margins, furn_at(maps, 1));
        // Second page has no footer text — can't confirm page numbering
        let text = second_footer?;
        let second_trimmed = text.trim();
        let fmt2 = classify_page_number(second_trimmed);
        // Both pages must have the same format
        if fmt2.as_ref() != Some(&fmt) {
            return None;
        }
        // Values must be consecutive (page 2 value = page 1 value + 1)
        let val1 = page_number_value(first_trimmed, fmt);
        let val2 = page_number_value(second_trimmed, fmt);
        if val1 == 0 || val2 == 0 || val2 != val1 + 1 {
            return None;
        }
    } else {
        // Single-page document: only accept a single-digit number "1" as
        // reasonably likely to be page numbering. Other formats ("i", "a",
        // etc.) are too ambiguous without a second page to confirm.
        if first_trimmed != "1" {
            return None;
        }
    }

    Some(fmt)
}

/// Multi-section sibling of [`detect_page_numbering`]: every page's footer is
/// a page-number token whose value matches the rendered `Page::number`, even
/// when the numbering format changes mid-document (U30 — `w:pgNumType` per
/// section). Returns the LAST page's format, which the final section carries.
#[must_use]
pub fn detect_multi_format_page_numbering(
    paged: &PagedDocument,
    margins: MarginsPt,
    maps: Option<&FurnitureMaps>,
) -> Option<PageNumberFormat> {
    let rendered = paged.pages();
    if rendered.len() < 2 {
        return None;
    }
    let mut last_fmt = None;
    for (index, page) in rendered.iter().enumerate() {
        let text = extract_footer_text_from_page(&page.frame, margins, furn_at(maps, index))?;
        let trimmed = text.trim();
        let fmt = classify_page_number(trimmed)?;
        if u64::from(page_number_value(trimmed, fmt)) != page.number {
            return None;
        }
        if index + 1 == rendered.len() {
            last_fmt = Some(fmt);
        }
    }
    last_fmt
}

pub(in crate::convert) struct NumberedFooter {
    pub part: HeaderFooter,
    pub distance: u32,
}

pub(in crate::convert) fn body_spans(frame: &Frame) -> Option<Vec<typst::syntax::Span>> {
    use typst::introspection::Tag;
    use typst_library::pdf::{ArtifactElem, ArtifactKind};

    let mut artifacts = Vec::new();
    let mut spans = Vec::new();
    let mut painted = false;
    super::super::frames::visit_frame_items(frame, false, &mut |_, item| match item {
        FrameItem::Tag(Tag::Start(elem, _))
            if elem.to_packed::<ArtifactElem>().is_some_and(|artifact| {
                matches!(
                    artifact.kind.as_option(),
                    Some(ArtifactKind::Header | ArtifactKind::Footer | ArtifactKind::Background)
                )
            }) =>
        {
            artifacts.extend(elem.location());
        }
        FrameItem::Tag(Tag::End(location, ..)) if artifacts.last() == Some(location) => {
            artifacts.pop();
        }
        FrameItem::Tag(Tag::Start(elem, _)) if artifacts.is_empty() => spans.push(elem.span()),
        FrameItem::Text(text) if artifacts.is_empty() && !text.text.trim().is_empty() => {
            painted = true;
            spans.extend(text.glyphs.iter().map(|glyph| glyph.span.0));
        }
        FrameItem::Image(..) | FrameItem::Shape(..) if artifacts.is_empty() => painted = true,
        _ => {}
    });
    painted.then_some(spans)
}

pub(in crate::convert) fn numbered_footer(
    page: &typst_layout::Page,
) -> Result<Option<NumberedFooter>, ()> {
    use typst::introspection::Tag;
    use typst_library::pdf::{ArtifactElem, ArtifactKind};

    let mut footer = None;
    let mut text = None;
    let mut complex = false;
    super::super::frames::visit_frame_items(&page.frame, false, &mut |pos, item| match item {
        FrameItem::Tag(Tag::Start(elem, _))
            if elem.to_packed::<ArtifactElem>().is_some_and(|artifact| {
                matches!(artifact.kind.as_option(), Some(ArtifactKind::Footer))
            }) =>
        {
            footer = elem.location();
        }
        FrameItem::Tag(Tag::End(location, ..)) if footer == Some(*location) => footer = None,
        FrameItem::Text(item) if footer.is_some() && !item.text.trim().is_empty() => {
            complex |= text.replace((pos, item)).is_some();
        }
        FrameItem::Image(..) | FrameItem::Shape(..) if footer.is_some() => complex = true,
        _ => {}
    });
    if complex {
        return Err(());
    }
    let Some((pos, text)) = text else {
        return Ok(None);
    };
    let Some(Numbering::Pattern(pattern)) = &page.numbering else {
        return Err(());
    };
    if numbering_page_format(page.numbering.as_ref()).is_none()
        || pattern
            .apply(None, &[page.number])
            .map_err(|_| ())?
            .as_str()
            != text.text.trim()
    {
        return Err(());
    }
    let extent = text.bbox();
    let center = pos.x + (extent.min.x + extent.max.x) / 2.0;
    let half_width = page.frame.width() / 2.0;
    let alignment = if (center - half_width).abs() < half_width * MARGIN_CENTER_TOLERANCE_RATIO {
        Alignment::Center
    } else if center > half_width {
        Alignment::Right
    } else {
        Alignment::Left
    };
    let distance = page.frame.height() - pos.y + text.font.metrics().descender.at(text.size);
    Ok(Some(NumberedFooter {
        part: make_numbered_footer(text, alignment),
        distance: pt_to_twips(distance.to_pt()),
    }))
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "rendered font sizes in half-points are small and non-negative"
)]
fn make_numbered_footer(
    text: &typst_library::text::TextItem,
    alignment: Alignment,
) -> HeaderFooter {
    let mut para = Paragraph::new();
    let size = (text.size.to_pt() * 2.0).round().max(1.0) as u32;
    para.alignment = Some(alignment);
    para.mark_size_half_pt = Some(size);
    para.spacing_before = Some(0);
    para.spacing_after = Some(0);
    para.first_line_indent = Some(FirstLineIndent {
        twips: 0,
        chars: Some(0),
    });
    para.inlines.push(InlineElement::FieldPage {
        display_text: text.text.trim().to_string(),
        bold: effective_weight(&text.font) >= 600,
        size_half_pt: Some(size),
    });
    HeaderFooter {
        paragraphs: vec![para],
    }
}

/// Get the numeric value of a page number string for a given format.
///
/// Returns 0 if the string cannot be parsed for the given format.
fn page_number_value(s: &str, fmt: PageNumberFormat) -> u32 {
    match fmt {
        PageNumberFormat::Decimal => s.parse::<u32>().unwrap_or(0),
        PageNumberFormat::LowerRoman => roman_value(s, false),
        PageNumberFormat::UpperRoman => roman_value(s, true),
        PageNumberFormat::LowerLetter => {
            if s.len() == 1 {
                let c = s.chars().next().unwrap();
                if c.is_ascii_lowercase() {
                    return u32::from(c) - u32::from('a') + 1;
                }
            }
            0
        }
        PageNumberFormat::UpperLetter => {
            if s.len() == 1 {
                let c = s.chars().next().unwrap();
                if c.is_ascii_uppercase() {
                    return u32::from(c) - u32::from('A') + 1;
                }
            }
            0
        }
    }
}

/// The page's furniture analysis from the maps, when present.
fn furn_at(maps: Option<&FurnitureMaps>, index: usize) -> Option<&PageFurniture> {
    maps.and_then(|m| m.per_page.get(index))
}

/// Extract margin-zone text from a single page frame (text in the top or
/// bottom margin zone), in frame order. Returns `None` when the zone is
/// empty.
fn extract_zone_text_from_page(
    frame: &Frame,
    zone: MarginZone,
    margins: MarginsPt,
    furn: Option<&PageFurniture>,
) -> Option<String> {
    let page_width = frame.width().to_pt();
    let page_height = frame.height().to_pt();

    let mut fragments = Vec::new();
    match furn {
        Some(furn) => collect_fragments_skip_furniture(frame, furn, &mut fragments),
        None => collect_text_fragments(frame, &mut fragments),
    }

    let (body_top, body_bottom) = find_body_zone(
        page_width,
        page_height,
        Some(margins.top),
        Some(margins.bottom),
    );

    let items: Vec<&TextFragment> = fragments
        .iter()
        .filter(|f| match zone {
            MarginZone::Top => f.y < body_top && f.y > 0.0,
            MarginZone::Bottom => f.y > body_bottom && f.y <= page_height,
        })
        .collect();

    if items.is_empty() {
        return None;
    }

    let text: String = items.iter().map(|f| f.text.as_str()).collect();
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Extract footer text from a single page frame (text in the bottom margin zone).
fn extract_footer_text_from_page(
    frame: &Frame,
    margins: MarginsPt,
    furn: Option<&PageFurniture>,
) -> Option<String> {
    extract_zone_text_from_page(frame, MarginZone::Bottom, margins, furn)
}

/// Normalize a margin-zone text for variant comparison: every run of ASCII
/// digits collapses to a single `#`, so a page number ("Page 1" vs
/// "Page 2") does not read as a header/footer variant. A header that
/// differs only in a number (e.g. a chapter number) is missed by the same
/// token — a deliberate false-negative bias: warnings must not fire on
/// ordinary paginated documents.
fn normalize_zone_text_for_comparison(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_digits = false;
    for c in text.chars() {
        if c.is_ascii_digit() {
            if !in_digits {
                out.push('#');
                in_digits = true;
            }
        } else {
            out.push(c);
            in_digits = false;
        }
    }
    out
}

/// In C1, check whether any later page's header or footer text differs
/// from page 1's (after digit normalization, so page numbers don't count).
/// C1 keeps only the first page's header/footer, so a variant means the
/// later pages' headers/footers are lost. Returns a span-less degradation
/// naming the first differing page, or `None` when all pages agree (or the
/// document has a single page).
#[must_use]
pub fn header_footer_variant(paged: &PagedDocument, margins: MarginsPt) -> Option<Degradation> {
    let doc_pages = paged.pages();
    if doc_pages.len() < 2 {
        return None;
    }
    for zone in [MarginZone::Top, MarginZone::Bottom] {
        let zone_name = match zone {
            MarginZone::Top => "header",
            MarginZone::Bottom => "footer",
        };
        let first = extract_zone_text_from_page(&doc_pages[0].frame, zone, margins, None)
            .map(|text| normalize_zone_text_for_comparison(&text));
        for (index, page) in doc_pages.iter().enumerate().skip(1) {
            let text = extract_zone_text_from_page(&page.frame, zone, margins, None)
                .map(|text| normalize_zone_text_for_comparison(&text));
            if text != first {
                return Some(Degradation::new(
                    None,
                    "header-footer-variant",
                    DegradationAction::Skipped,
                    format!(
                        "page {} {zone_name} differs from page 1; only the first page's {zone_name} is kept",
                        index + 1
                    ),
                ));
            }
        }
    }
    None
}

/// Classify a string as a page number format.
///
/// - "1", "2", ... → `Decimal`
/// - "i", "ii", "iii", "iv", ... → `LowerRoman`
/// - "I", "II", "III", "IV", ... → `UpperRoman`
/// - "a", "b", "c", ... → `LowerLetter`
/// - "A", "B", "C", ... → `UpperLetter`
fn classify_page_number(s: &str) -> Option<PageNumberFormat> {
    if s.is_empty() {
        return None;
    }

    // Decimal: pure digits
    if s.chars().all(|c| c.is_ascii_digit()) {
        return Some(PageNumberFormat::Decimal);
    }

    // Roman numerals (lowercase): i, ii, iii, iv, v, vi, vii, viii, ix, x, ...
    if is_lower_roman(s) {
        return Some(PageNumberFormat::LowerRoman);
    }

    // Roman numerals (uppercase): I, II, III, IV, V, ...
    if is_upper_roman(s) {
        return Some(PageNumberFormat::UpperRoman);
    }

    // Single letter: a-z or A-Z (page 1 = a, page 2 = b, etc.)
    if s.len() == 1 {
        let c = s.chars().next().unwrap();
        if c.is_ascii_lowercase() {
            return Some(PageNumberFormat::LowerLetter);
        }
        if c.is_ascii_uppercase() {
            return Some(PageNumberFormat::UpperLetter);
        }
    }

    None
}

/// Check if a string is a valid lowercase Roman numeral.
fn is_lower_roman(s: &str) -> bool {
    if s.is_empty()
        || !s
            .chars()
            .all(|c| matches!(c, 'i' | 'v' | 'x' | 'l' | 'c' | 'd' | 'm'))
    {
        return false;
    }
    // Validate by converting and checking it's a reasonable page number
    roman_value(s, false) > 0
}

/// Check if a string is a valid uppercase Roman numeral.
fn is_upper_roman(s: &str) -> bool {
    if s.is_empty()
        || !s
            .chars()
            .all(|c| matches!(c, 'I' | 'V' | 'X' | 'L' | 'C' | 'D' | 'M'))
    {
        return false;
    }
    roman_value(s, true) > 0
}

/// Compute the numeric value of a Roman numeral string.
fn roman_value(s: &str, uppercase: bool) -> u32 {
    let val = |c: char| -> u32 {
        match if uppercase { c.to_ascii_lowercase() } else { c } {
            'i' => 1,
            'v' => 5,
            'x' => 10,
            'l' => 50,
            'c' => 100,
            'd' => 500,
            'm' => 1000,
            _ => 0,
        }
    };

    let chars: Vec<char> = s.chars().collect();
    let mut total: u32 = 0;
    for i in 0..chars.len() {
        let curr = val(chars[i]);
        let next = if i + 1 < chars.len() {
            val(chars[i + 1])
        } else {
            0
        };
        if curr < next {
            total = total.wrapping_sub(curr);
        } else {
            total = total.wrapping_add(curr);
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classify_decimal() {
        assert_eq!(classify_page_number("1"), Some(PageNumberFormat::Decimal));
        assert_eq!(classify_page_number("42"), Some(PageNumberFormat::Decimal));
        assert_eq!(classify_page_number("100"), Some(PageNumberFormat::Decimal));
    }

    #[test]
    fn classify_lower_roman() {
        assert_eq!(
            classify_page_number("i"),
            Some(PageNumberFormat::LowerRoman)
        );
        assert_eq!(
            classify_page_number("ii"),
            Some(PageNumberFormat::LowerRoman)
        );
        assert_eq!(
            classify_page_number("iv"),
            Some(PageNumberFormat::LowerRoman)
        );
        assert_eq!(
            classify_page_number("xiv"),
            Some(PageNumberFormat::LowerRoman)
        );
    }

    #[test]
    fn classify_upper_roman() {
        assert_eq!(
            classify_page_number("I"),
            Some(PageNumberFormat::UpperRoman)
        );
        assert_eq!(
            classify_page_number("IV"),
            Some(PageNumberFormat::UpperRoman)
        );
        assert_eq!(
            classify_page_number("XII"),
            Some(PageNumberFormat::UpperRoman)
        );
    }

    #[test]
    fn classify_letters() {
        assert_eq!(
            classify_page_number("a"),
            Some(PageNumberFormat::LowerLetter)
        );
        assert_eq!(
            classify_page_number("z"),
            Some(PageNumberFormat::LowerLetter)
        );
        assert_eq!(
            classify_page_number("A"),
            Some(PageNumberFormat::UpperLetter)
        );
        assert_eq!(
            classify_page_number("Z"),
            Some(PageNumberFormat::UpperLetter)
        );
    }

    #[test]
    fn classify_non_page_numbers() {
        assert_eq!(classify_page_number(""), None);
        assert_eq!(classify_page_number("Draft"), None);
        assert_eq!(classify_page_number("Page 1"), None);
        assert_eq!(classify_page_number("hello"), None);
    }

    #[test]
    fn page_number_value_decimal() {
        assert_eq!(page_number_value("1", PageNumberFormat::Decimal), 1);
        assert_eq!(page_number_value("5", PageNumberFormat::Decimal), 5);
        assert_eq!(page_number_value("42", PageNumberFormat::Decimal), 42);
    }

    #[test]
    fn page_number_value_roman() {
        assert_eq!(page_number_value("i", PageNumberFormat::LowerRoman), 1);
        assert_eq!(page_number_value("ii", PageNumberFormat::LowerRoman), 2);
        assert_eq!(page_number_value("iii", PageNumberFormat::LowerRoman), 3);
        assert_eq!(page_number_value("iv", PageNumberFormat::LowerRoman), 4);
        assert_eq!(page_number_value("v", PageNumberFormat::LowerRoman), 5);
        assert_eq!(page_number_value("IX", PageNumberFormat::UpperRoman), 9);
        assert_eq!(page_number_value("X", PageNumberFormat::UpperRoman), 10);
    }

    #[test]
    fn page_number_value_letters() {
        assert_eq!(page_number_value("a", PageNumberFormat::LowerLetter), 1);
        assert_eq!(page_number_value("b", PageNumberFormat::LowerLetter), 2);
        assert_eq!(page_number_value("z", PageNumberFormat::LowerLetter), 26);
        assert_eq!(page_number_value("A", PageNumberFormat::UpperLetter), 1);
        assert_eq!(page_number_value("C", PageNumberFormat::UpperLetter), 3);
    }

    #[test]
    fn normalize_zone_text_masks_digit_runs() {
        assert_eq!(normalize_zone_text_for_comparison("Page 1"), "Page #");
        assert_eq!(
            normalize_zone_text_for_comparison("Page 12 of 300"),
            "Page # of #"
        );
        assert_eq!(normalize_zone_text_for_comparison("1"), "#");
        assert_eq!(normalize_zone_text_for_comparison(""), "");
        assert_eq!(
            normalize_zone_text_for_comparison("No digits here"),
            "No digits here"
        );
    }

    #[test]
    fn normalize_zone_text_keeps_text_differences() {
        assert_ne!(
            normalize_zone_text_for_comparison("Chapter 1: Intro"),
            normalize_zone_text_for_comparison("Chapter 2: Methods")
        );
        assert_eq!(
            normalize_zone_text_for_comparison("Chapter 1: Intro"),
            normalize_zone_text_for_comparison("Chapter 2: Intro")
        );
    }

    #[test]
    fn consecutive_check_logic() {
        // Decimal: 1 -> 2 is consecutive
        let v1 = page_number_value("1", PageNumberFormat::Decimal);
        let v2 = page_number_value("2", PageNumberFormat::Decimal);
        assert_eq!(v2, v1 + 1);

        // Decimal: 5 -> 5 is NOT consecutive (static text)
        let v1 = page_number_value("5", PageNumberFormat::Decimal);
        let v2 = page_number_value("5", PageNumberFormat::Decimal);
        assert_ne!(v2, v1 + 1);

        // Roman: i -> ii is consecutive
        let v1 = page_number_value("i", PageNumberFormat::LowerRoman);
        let v2 = page_number_value("ii", PageNumberFormat::LowerRoman);
        assert_eq!(v2, v1 + 1);

        // Letter: a -> b is consecutive
        let v1 = page_number_value("a", PageNumberFormat::LowerLetter);
        let v2 = page_number_value("b", PageNumberFormat::LowerLetter);
        assert_eq!(v2, v1 + 1);
    }

    #[test]
    fn int_to_roman_values() {
        assert_eq!(int_to_roman(0), "");
        assert_eq!(int_to_roman(1), "I");
        assert_eq!(int_to_roman(4), "IV");
        assert_eq!(int_to_roman(9), "IX");
        assert_eq!(int_to_roman(40), "XL");
        assert_eq!(int_to_roman(90), "XC");
        assert_eq!(int_to_roman(400), "CD");
        assert_eq!(int_to_roman(900), "CM");
        assert_eq!(int_to_roman(3999), "MMMCMXCIX");
    }

    #[test]
    fn int_to_letters_values() {
        assert_eq!(int_to_letters(0, false), "");
        assert_eq!(int_to_letters(1, false), "a");
        assert_eq!(int_to_letters(26, false), "z");
        assert_eq!(int_to_letters(27, false), "aa");
        assert_eq!(int_to_letters(28, false), "ab");
        assert_eq!(int_to_letters(52, false), "az");
        assert_eq!(int_to_letters(53, false), "ba");
        assert_eq!(int_to_letters(1, true), "A");
        assert_eq!(int_to_letters(27, true), "AA");
    }

    #[test]
    fn format_page_number_values() {
        assert_eq!(
            format_page_number(3, PageNumberFormat::Decimal),
            "3".to_string()
        );
        assert_eq!(
            format_page_number(4, PageNumberFormat::LowerRoman),
            "iv".to_string()
        );
        assert_eq!(
            format_page_number(9, PageNumberFormat::UpperRoman),
            "IX".to_string()
        );
        assert_eq!(
            format_page_number(27, PageNumberFormat::LowerLetter),
            "aa".to_string()
        );
    }

    fn rest(texts: &[&str], start_delta: u32, step: u32) -> Vec<(String, u32)> {
        texts
            .iter()
            .enumerate()
            .map(|(i, t)| (t.to_string(), start_delta + step * i as u32))
            .collect()
    }

    #[test]
    fn split_page_number_token_three_pages() {
        let r = rest(&["Page 4", "Page 5"], 1, 1);
        assert_eq!(
            split_page_number_token("Page 3", &r, None),
            Some(("Page ".to_string(), "3".to_string(), String::new()))
        );
        let r = rest(&["3 标题", "4 标题"], 1, 1);
        assert_eq!(
            split_page_number_token("2 标题", &r, None),
            Some((String::new(), "2".to_string(), " 标题".to_string()))
        );
        let r = rest(&["Chapter v", "Chapter vi"], 1, 1);
        assert_eq!(
            split_page_number_token("Chapter iv", &r, None),
            Some(("Chapter ".to_string(), "iv".to_string(), String::new()))
        );
        // Static text never splits, however many pages repeat it.
        let r = rest(&["Page 3", "Page 3"], 1, 1);
        assert_eq!(split_page_number_token("Page 3", &r, None), None);
        // A section number that stays put does not block the real page token.
        let r = rest(&["Section 2, page 4", "Section 2, page 5"], 1, 1);
        assert_eq!(
            split_page_number_token("Section 2, page 3", &r, None),
            Some((
                "Section 2, page ".to_string(),
                "3".to_string(),
                String::new()
            ))
        );
        // A lone letter is a page token when it increments on every page.
        let r = rest(&["b", "c"], 1, 1);
        assert_eq!(
            split_page_number_token("a", &r, None),
            Some((String::new(), "a".to_string(), String::new()))
        );
        // "Draft" is not a token: the D is glued to a letter.
        let r = rest(&["Draft", "Draft"], 1, 1);
        assert_eq!(split_page_number_token("Draft", &r, None), None);
    }

    #[test]
    fn split_page_number_token_parity_step() {
        // Same-parity pages advance the number by the physical page distance:
        // an odd-page header shows 1 on page 1 and 3 on page 3 (delta 2).
        let r = rest(&["Odd Title 3", "Odd Title 5"], 2, 2);
        assert_eq!(
            split_page_number_token("Odd Title 1", &r, None),
            Some(("Odd Title ".to_string(), "1".to_string(), String::new()))
        );
        // A number that does not advance by the group's physical distance
        // rejects the token (here it jumped by 2 while the delta says 1).
        let r = rest(&["Odd Title 3"], 1, 1);
        assert_eq!(split_page_number_token("Odd Title 1", &r, None), None);
    }

    #[test]
    fn split_page_number_token_two_pages() {
        let r = rest(&["Page 4"], 1, 1);
        assert_eq!(
            split_page_number_token("Page 3", &r, None),
            Some(("Page ".to_string(), "3".to_string(), String::new()))
        );
        // A non-consecutive second page rejects the token.
        let r = rest(&["Page 5"], 1, 1);
        assert_eq!(split_page_number_token("Page 3", &r, None), None);
    }

    #[test]
    fn split_page_number_token_single_page_declared_only() {
        // Declared format trusted, Decimal and Roman only.
        assert_eq!(
            split_page_number_token("Page 3", &[], Some(PageNumberFormat::Decimal)),
            Some(("Page ".to_string(), "3".to_string(), String::new()))
        );
        assert_eq!(
            split_page_number_token("Page i", &[], Some(PageNumberFormat::LowerRoman)),
            Some(("Page ".to_string(), "i".to_string(), String::new()))
        );
        // No declaration, no split.
        assert_eq!(split_page_number_token("Page 3", &[], None), None);
        // Declared format must match the token's format.
        assert_eq!(
            split_page_number_token("Page 3", &[], Some(PageNumberFormat::LowerRoman)),
            None
        );
        // A lone letter is too ambiguous for a single page, even declared.
        assert_eq!(
            split_page_number_token("a", &[], Some(PageNumberFormat::LowerLetter)),
            None
        );
        // Two candidates in the text: too ambiguous for a single page.
        assert_eq!(
            split_page_number_token("Section 2, page 3", &[], Some(PageNumberFormat::Decimal)),
            None
        );
    }

    #[test]
    fn page_number_format_from_declaration_values() {
        assert_eq!(
            page_number_format_from_declaration("1"),
            Some(PageNumberFormat::Decimal)
        );
        assert_eq!(
            page_number_format_from_declaration("i"),
            Some(PageNumberFormat::LowerRoman)
        );
        assert_eq!(
            page_number_format_from_declaration("I"),
            Some(PageNumberFormat::UpperRoman)
        );
        assert_eq!(
            page_number_format_from_declaration("a"),
            Some(PageNumberFormat::LowerLetter)
        );
        assert_eq!(
            page_number_format_from_declaration("A"),
            Some(PageNumberFormat::UpperLetter)
        );
        // Decorated, zero-padded, dotted, and function patterns have no
        // `w:pgNumType` equivalent.
        assert_eq!(page_number_format_from_declaration("1."), None);
        assert_eq!(page_number_format_from_declaration("(1)"), None);
        assert_eq!(page_number_format_from_declaration("1.1"), None);
        assert_eq!(page_number_format_from_declaration("01"), None);
        assert_eq!(page_number_format_from_declaration(""), None);
    }
}
