//! `StyleChain` -> `Run` helpers for the realize collector.
//!
//! The collector reads per-run formatting from the `StyleChain` of each
//! realized pair (the Paged show rules already merged strong/emphasis,
//! super/subscript, smallcaps and decorations into `TextElem` attributes),
//! so this module never walks the HTML tag stream. Per-run overrides are
//! recorded only when they differ from the document defaults (read from the
//! realized pairs in `collect_document`, falling back to the root
//! `StyleChain`), so body text stays implicit — exactly as the legacy
//! path's Paged-derived overrides behave.

use typort_ir::{Degradation, DegradationAction};
use typort_ooxml::document::Run;
use typst_library::foundations::StyleChain;
use typst_library::text::{
    DecoLine, FontFamily, FontStyle, FontWeight, ScriptKind, TextElem, variant,
};
use typst_syntax::Span;

use crate::convert::highlight::{HighlightStyle, highlight_style};
use crate::convert::page::extract_non_black_color;

/// Document-wide style defaults, read from the realized pairs in
/// `collect_document` (falling back to the root `StyleChain`). Per-run
/// overrides are recorded only when they differ from these.
#[derive(Debug, Clone)]
pub struct StyleDefaults {
    /// Body text size in half-points.
    pub size_half_pt: u32,
    /// Body font family (lowercased, as `FontFamily` stores it).
    pub body_font: String,
    /// Body CJK font family: the last family of the body font list (the
    /// CJK fallback, conventionally last — see `run_from_styles`).
    pub font_east_asia: String,
}

/// Read the document defaults from the root `StyleChain`.
pub fn defaults_from_root(styles: StyleChain<'_>) -> StyleDefaults {
    let size_pt = styles.resolve(TextElem::size).to_pt();
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "sizes are small and clamped to at least 1.0 first"
    )]
    let size_half_pt = (size_pt * 2.0).round().max(1.0) as u32;
    let families: Vec<&str> = typst_library::text::families(styles)
        .map(FontFamily::as_str)
        .collect();
    let body_font = families.first().map_or_else(
        || "Times New Roman".to_string(),
        |family| (*family).to_string(),
    );
    // The East Asia font is the last DECLARED family, not the last family
    // of `families()`: that iterator appends Typst's emoji fallback tail
    // ("twitter color emoji", ..., "segoe ui emoji"), so its last entry
    // is always an emoji face and carries no CJK signal. With no declared
    // family the body font is the sensible East Asia default.
    let declared: Vec<&str> = styles
        .get_ref(TextElem::font)
        .into_iter()
        .map(FontFamily::as_str)
        .collect();
    let font_east_asia = declared
        .last()
        .map_or_else(|| body_font.clone(), |family| (*family).to_string());
    StyleDefaults {
        size_half_pt,
        body_font,
        font_east_asia,
    }
}

/// Build a run for `text` under `styles`.
///
/// `span` is the pair content's source span, carried so source-AST
/// post-passes (hanging indent, small caps) can locate the run in the
/// source; synthesized text passes `None`.
/// `Case` is applied to the text itself (as Typst does when shaping);
/// strong/emphasis, super/subscript, smallcaps and decorations are read
/// from the folded `TextElem` properties. The font family comes from the
/// `StyleChain` font list: the first family for Latin runs, the last family
/// for runs containing CJK characters (the list is ordered by preference and
/// the CJK fallback is conventionally last). This is a heuristic — the
/// authoritative per-run font is the Paged `TextItem` font, which the
/// legacy path uses; see `docs/fallback-registry.md`.
///
/// `degradations` is the sink for aspects the run model cannot express:
/// overline has no run property, so the decoration is dropped and recorded
/// there (construct `"overline"`); the text itself is kept.
pub fn run_from_styles(
    text: &str,
    styles: StyleChain<'_>,
    defaults: &StyleDefaults,
    span: Option<Span>,
    degradations: &mut Vec<Degradation>,
) -> Run {
    let case = styles.get(TextElem::case);
    let text = match case {
        Some(case) => case.apply(text),
        None => text.to_owned(),
    };
    let mut run = Run::new(text);
    run.span = span;

    let variant = variant(styles);
    run.bold = variant.weight >= FontWeight::BOLD;
    run.italic = variant.style != FontStyle::Normal;

    let size_pt = styles.resolve(TextElem::size).to_pt();
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "sizes are small and clamped to at least 1.0 first"
    )]
    let size_half_pt = (size_pt * 2.0).round().max(1.0) as u32;
    if size_half_pt != defaults.size_half_pt {
        run.size_half_pt = Some(size_half_pt);
    }

    let families: Vec<&str> = typst_library::text::families(styles)
        .map(FontFamily::as_str)
        .collect();
    if let Some(first) = families.first()
        && !first.eq_ignore_ascii_case(&defaults.body_font)
    {
        run.font_ascii = Some((*first).to_string());
    }
    if contains_cjk(&run.text)
        && let Some(last) = families.last()
        && !last.eq_ignore_ascii_case(&defaults.font_east_asia)
    {
        run.font_east_asia = Some((*last).to_string());
    }
    if families
        .first()
        .is_some_and(|name| name.contains("mono") || name.contains("courier"))
    {
        run.monospace = true;
    }

    if let Some(color) = extract_non_black_color(&styles.get_cloned(TextElem::fill)) {
        run.color = Some(color);
    }

    for decoration in styles.get_cloned(TextElem::deco) {
        match decoration.line {
            DecoLine::Underline { .. } => run.underline = true,
            DecoLine::Strikethrough { .. } => run.strikethrough = true,
            // Overline has no run property in the OOXML model (M9: it is
            // drawn as a line shape above the text, not a run toggle), so
            // the decoration is dropped here and recorded as a
            // degradation — the text itself is kept. (The legacy path
            // differs: it silently rewrites overline to underline.)
            DecoLine::Overline { .. } => degradations.push(Degradation {
                span,
                construct: "overline",
                action: DegradationAction::Skipped,
                message: "overline dropped; text kept without decoration".to_string(),
            }),
            DecoLine::Highlight { fill, .. } => match highlight_style(fill.as_ref()) {
                HighlightStyle::Named(name) => run.highlight_color = Some(name.to_string()),
                HighlightStyle::Shading(hex) => run.shading = Some(hex),
                HighlightStyle::None => {}
            },
        }
    }

    if styles.get(TextElem::smallcaps).is_some() {
        run.smallcaps = true;
    }
    if let Some(shift) = styles.get(TextElem::shift_settings) {
        match shift.kind {
            ScriptKind::Super => run.superscript = true,
            ScriptKind::Sub => run.subscript = true,
        }
    }

    run
}

/// Whether `text` contains any CJK, hiragana, katakana or hangul character.
pub fn contains_cjk(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(c,
            '\u{3040}'..='\u{309F}' // hiragana
            | '\u{30A0}'..='\u{30FF}' // katakana
            | '\u{3400}'..='\u{4DBF}' // CJK extension A
            | '\u{4E00}'..='\u{9FFF}' // CJK unified ideographs
            | '\u{AC00}'..='\u{D7AF}') // hangul syllables
    })
}

/// Whether two runs may be merged into one: their formatting keys match
/// (`Run::format_key`, the writer's own merge-eligibility key — every
/// field the writer serializes into `<w:rPr>`, excluding text, span and
/// line breaks).
pub fn lower_runs_match(prev: &Run, next: &Run) -> bool {
    prev.format_key() == next.format_key()
}

#[cfg(test)]
mod tests {
    use super::*;
    use typst_library::foundations::{StyleChain, Styles};
    use typst_library::text::{Case, TextElem};
    use typst_library::visualize::Paint;

    fn root_styles() -> Styles {
        Styles::new()
    }

    #[test]
    fn contains_cjk_spots_cjk_and_latin() {
        assert!(contains_cjk("中文"));
        assert!(contains_cjk("かな"));
        assert!(contains_cjk("한글"));
        assert!(!contains_cjk("hello 123"));
    }

    #[test]
    fn defaults_from_root_reads_size_and_font() {
        let styles = root_styles();
        let defaults = defaults_from_root(StyleChain::new(&styles));
        assert_eq!(defaults.size_half_pt, 22);
        assert_eq!(defaults.body_font, "libertinus serif");
        assert_eq!(defaults.font_east_asia, "libertinus serif");
    }

    #[test]
    fn run_from_styles_body_text_stays_implicit() {
        let styles = root_styles();
        let defaults = defaults_from_root(StyleChain::new(&styles));
        let run = run_from_styles(
            "hello",
            StyleChain::new(&styles),
            &defaults,
            None,
            &mut Vec::new(),
        );
        assert_eq!(run.text, "hello");
        assert!(!run.bold);
        assert!(!run.italic);
        assert_eq!(run.size_half_pt, None);
        assert_eq!(run.font_ascii, None);
        assert_eq!(run.color, None);
    }

    #[test]
    fn run_from_styles_bold_size_and_color() {
        let mut styles = root_styles();
        styles.set(TextElem::weight, FontWeight::BOLD);
        styles.set(
            TextElem::size,
            typst_library::text::TextSize(typst_library::layout::Abs::pt(14.0).into()),
        );
        styles.set(
            TextElem::fill,
            Paint::Solid(typst_library::visualize::Color::from_u8(255, 0, 0, 255)),
        );
        let defaults = defaults_from_root(StyleChain::new(&root_styles()));
        let run = run_from_styles(
            "hi",
            StyleChain::new(&styles),
            &defaults,
            None,
            &mut Vec::new(),
        );
        assert!(run.bold);
        assert_eq!(run.size_half_pt, Some(28));
        assert_eq!(run.color.as_deref(), Some("FF0000"));
    }

    #[test]
    fn run_from_styles_smallcaps_case() {
        let mut styles = root_styles();
        styles.set(TextElem::case, Some(Case::Upper));
        styles.set(
            TextElem::smallcaps,
            Some(typst_library::text::Smallcaps::Minuscules),
        );
        let defaults = defaults_from_root(StyleChain::new(&root_styles()));
        let run = run_from_styles(
            "abc",
            StyleChain::new(&styles),
            &defaults,
            None,
            &mut Vec::new(),
        );
        assert_eq!(run.text, "ABC");
        assert!(run.smallcaps);
    }

    #[test]
    fn lower_runs_match_ignores_text_but_not_format() {
        let mut plain = Run::new("a");
        let mut bold = Run::new("b");
        bold.bold = true;
        assert!(lower_runs_match(&plain, &Run::new("c")));
        assert!(!lower_runs_match(&plain, &bold));
        plain.size_half_pt = Some(28);
        assert!(!lower_runs_match(&plain, &Run::new("d")));
    }
}
