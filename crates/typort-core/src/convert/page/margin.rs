use typort_ooxml::document::{
    Alignment, HeaderFooter, PageNumberFormat, PageSettings, Paragraph, Run,
};
use typst::layout::{Frame, FrameItem};
use typst_layout::PagedDocument;

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
}

/// Collect text fragments with absolute positions from a frame.
pub(super) fn collect_text_fragments(frame: &Frame, items: &mut Vec<TextFragment>) {
    super::super::frames::visit_frame_items(frame, false, &mut |position, item| {
        if let FrameItem::Text(text_item) = item {
            let text = text_item.text.to_string();
            if !text.is_empty() {
                items.push(TextFragment {
                    y: position.y.to_pt(),
                    x: position.x.to_pt(),
                    text,
                });
            }
        }
    });
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
}

impl MarginsPt {
    /// Read the resolved margins from the document's `PageSettings`.
    #[must_use]
    pub fn from_settings(settings: &PageSettings) -> Self {
        Self {
            top: f64::from(settings.margin_top) / 20.0,
            bottom: f64::from(settings.margin_bottom) / 20.0,
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

/// Extract header content from the top margin area of the first page.
///
/// A page-number token inside the zone (e.g. "Page 3", "2 标题") is split
/// out as a PAGE field; the rest stays literal text. `declared` is the
/// `#set page(numbering:)` format, trusted for a single-page document where
/// consecutiveness cannot be verified.
#[must_use]
pub fn extract_header(
    paged: &PagedDocument,
    margins: MarginsPt,
    declared: Option<PageNumberFormat>,
) -> Option<HeaderFooter> {
    extract_margin_zone(paged, MarginZone::Top, margins, declared)
}

/// Extract footer content from the bottom margin area of the first page.
#[must_use]
pub fn extract_footer(
    paged: &PagedDocument,
    margins: MarginsPt,
    declared: Option<PageNumberFormat>,
) -> Option<HeaderFooter> {
    extract_margin_zone(paged, MarginZone::Bottom, margins, declared)
}

#[derive(Clone, Copy)]
enum MarginZone {
    Top,
    Bottom,
}

fn extract_margin_zone(
    paged: &PagedDocument,
    zone: MarginZone,
    margins: MarginsPt,
    declared: Option<PageNumberFormat>,
) -> Option<HeaderFooter> {
    let doc_pages = paged.pages();
    let page = doc_pages.first()?;
    let page_width = page.frame.width().to_pt();

    let fragments = zone_fragments(&page.frame, zone, margins);
    if fragments.is_empty() {
        return None;
    }

    let text1: String = fragments.iter().map(|f| f.text.as_str()).collect();
    let text2 = doc_pages.get(1).map(|p| {
        zone_fragments(&p.frame, zone, margins)
            .iter()
            .map(|f| f.text.as_str())
            .collect::<String>()
    });
    let text3 = doc_pages.get(2).map(|p| {
        zone_fragments(&p.frame, zone, margins)
            .iter()
            .map(|f| f.text.as_str())
            .collect::<String>()
    });

    let mut para = Paragraph::new();
    if let Some((prefix, token, suffix)) =
        split_page_number_token(&text1, text2.as_deref(), text3.as_deref(), declared)
    {
        if !prefix.is_empty() {
            para.push_run(Run::new(&prefix));
        }
        para.add_field_page(token);
        if !suffix.is_empty() {
            para.push_run(Run::new(&suffix));
        }
    } else {
        for item in &fragments {
            para.push_run(Run::new(&item.text));
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

    Some(HeaderFooter {
        paragraphs: vec![para],
    })
}

/// Text fragments in one page frame's margin zone, in reading (x) order.
fn zone_fragments(frame: &Frame, zone: MarginZone, margins: MarginsPt) -> Vec<TextFragment> {
    let page_width = frame.width().to_pt();
    let page_height = frame.height().to_pt();

    let mut fragments = Vec::new();
    collect_text_fragments(frame, &mut fragments);

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
/// Verification: with three or more pages the token must increment by exactly
/// one on both page 1→2 and page 2→3; with exactly two pages one +1 step is
/// required. With a single page consecutiveness cannot be verified, so only
/// the declared format is trusted — and only for Decimal and Roman (a lone
/// letter is too ambiguous) — and the token must be the only candidate in the
/// text.
///
/// Residual risk (accepted): a section number that happens to increment by
/// exactly one per page is indistinguishable from a page number by this
/// check; with three pages the check still accepts it.
fn split_page_number_token(
    page1: &str,
    page2: Option<&str>,
    page3: Option<&str>,
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
    match (page2, page3) {
        (Some(p2), Some(p3)) => candidates
            .iter()
            .find(|c| {
                let Some(v2) = c.value.checked_add(1) else {
                    return false;
                };
                let Some(v3) = c.value.checked_add(2) else {
                    return false;
                };
                let prefix = &page1[..c.start];
                let suffix = &page1[c.end..];
                p2 == format!("{prefix}{}{suffix}", format_page_number(v2, c.fmt))
                    && p3 == format!("{prefix}{}{suffix}", format_page_number(v3, c.fmt))
            })
            .map(split),
        (Some(p2), None) => candidates
            .iter()
            .find(|c| {
                let Some(v2) = c.value.checked_add(1) else {
                    return false;
                };
                let prefix = &page1[..c.start];
                let suffix = &page1[c.end..];
                p2 == format!("{prefix}{}{suffix}", format_page_number(v2, c.fmt))
            })
            .map(split),
        (None, _) => {
            if candidates.len() != 1 {
                return None;
            }
            let c = &candidates[0];
            if declared != Some(c.fmt) {
                return None;
            }
            if !matches!(
                c.fmt,
                PageNumberFormat::Decimal
                    | PageNumberFormat::LowerRoman
                    | PageNumberFormat::UpperRoman
            ) {
                return None;
            }
            Some(split(c))
        }
    }
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
) -> Option<PageNumberFormat> {
    if paged.pages().is_empty() {
        return None;
    }

    // Extract footer text from the first page
    let first_footer = extract_footer_text_from_page(&paged.pages()[0].frame, margins)?;
    let first_trimmed = first_footer.trim();

    // Try to classify the text as a page number format
    let fmt = classify_page_number(first_trimmed)?;

    // If we have a second page, verify consecutiveness to avoid false positives
    if paged.pages().len() >= 2 {
        let second_footer = extract_footer_text_from_page(&paged.pages()[1].frame, margins);
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

/// Extract footer text from a single page frame (text in the bottom margin zone).
fn extract_footer_text_from_page(frame: &Frame, margins: MarginsPt) -> Option<String> {
    let page_width = frame.width().to_pt();
    let page_height = frame.height().to_pt();

    let mut fragments = Vec::new();
    collect_text_fragments(frame, &mut fragments);

    let (_body_top, body_bottom) = find_body_zone(
        page_width,
        page_height,
        Some(margins.top),
        Some(margins.bottom),
    );

    let footer_items: Vec<&TextFragment> = fragments
        .iter()
        .filter(|f| f.y > body_bottom && f.y <= page_height)
        .collect();

    if footer_items.is_empty() {
        return None;
    }

    let text: String = footer_items.iter().map(|f| f.text.as_str()).collect();
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
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

    #[test]
    fn split_page_number_token_three_pages() {
        assert_eq!(
            split_page_number_token("Page 3", Some("Page 4"), Some("Page 5"), None),
            Some(("Page ".to_string(), "3".to_string(), String::new()))
        );
        assert_eq!(
            split_page_number_token("2 标题", Some("3 标题"), Some("4 标题"), None),
            Some((String::new(), "2".to_string(), " 标题".to_string()))
        );
        assert_eq!(
            split_page_number_token("Chapter iv", Some("Chapter v"), Some("Chapter vi"), None),
            Some(("Chapter ".to_string(), "iv".to_string(), String::new()))
        );
        // Static text never splits, however many pages repeat it.
        assert_eq!(
            split_page_number_token("Page 3", Some("Page 3"), Some("Page 3"), None),
            None
        );
        // A section number that stays put does not block the real page token.
        assert_eq!(
            split_page_number_token(
                "Section 2, page 3",
                Some("Section 2, page 4"),
                Some("Section 2, page 5"),
                None
            ),
            Some((
                "Section 2, page ".to_string(),
                "3".to_string(),
                String::new()
            ))
        );
        // A lone letter is a page token when it increments on every page.
        assert_eq!(
            split_page_number_token("a", Some("b"), Some("c"), None),
            Some((String::new(), "a".to_string(), String::new()))
        );
        // "Draft" is not a token: the D is glued to a letter.
        assert_eq!(
            split_page_number_token("Draft", Some("Draft"), Some("Draft"), None),
            None
        );
    }

    #[test]
    fn split_page_number_token_two_pages() {
        assert_eq!(
            split_page_number_token("Page 3", Some("Page 4"), None, None),
            Some(("Page ".to_string(), "3".to_string(), String::new()))
        );
        // A non-consecutive second page rejects the token.
        assert_eq!(
            split_page_number_token("Page 3", Some("Page 5"), None, None),
            None
        );
    }

    #[test]
    fn split_page_number_token_single_page_declared_only() {
        // Declared format trusted, Decimal and Roman only.
        assert_eq!(
            split_page_number_token("Page 3", None, None, Some(PageNumberFormat::Decimal)),
            Some(("Page ".to_string(), "3".to_string(), String::new()))
        );
        assert_eq!(
            split_page_number_token("Page i", None, None, Some(PageNumberFormat::LowerRoman)),
            Some(("Page ".to_string(), "i".to_string(), String::new()))
        );
        // No declaration, no split.
        assert_eq!(split_page_number_token("Page 3", None, None, None), None);
        // Declared format must match the token's format.
        assert_eq!(
            split_page_number_token("Page 3", None, None, Some(PageNumberFormat::LowerRoman)),
            None
        );
        // A lone letter is too ambiguous for a single page, even declared.
        assert_eq!(
            split_page_number_token("a", None, None, Some(PageNumberFormat::LowerLetter)),
            None
        );
        // Two candidates in the text: too ambiguous for a single page.
        assert_eq!(
            split_page_number_token(
                "Section 2, page 3",
                None,
                None,
                Some(PageNumberFormat::Decimal)
            ),
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
