//! Document-global `set` values resolved from `TagElem` pairs' `StyleChain`s —
//! tag chains are the ambient document-scope state, so element-local styling
//! never contaminates a global read.

use typort_ooxml::document::VerticalAlign;
use typst_library::foundations::{Smart, StyleChain};
use typst_library::introspection::TagElem;
use typst_library::layout::{AlignElem, ColumnsElem, Length, PageElem, Rel, Sides, VAlignment};
use typst_library::model::{FootnoteEntry, Numbering, ParElem, ParLine};
use typst_library::routines::Pair;
use typst_library::text::{Lang, TextElem};
use typst_library::visualize::LineElem;

use super::page::{self, SourceStyleOverrides};

/// Split a `Length` into `(twips, em)` — the decomposition the AST pass
/// produced: absolute-only → twips (an explicit `0pt` is a declared
/// `Some(0)`, matching `margin_amount`), em-only → the em factor. A mixed
/// `pt + em` sum (unparseable in the AST too) yields nothing.
fn length_amount(length: &Length) -> (Option<u32>, Option<f64>) {
    let abs = length.abs.to_pt();
    let em = length.em.get();
    if em != 0.0 {
        if abs != 0.0 {
            return (None, None);
        }
        return (None, Some(em));
    }
    (Some(page::pt_to_twips(abs)), None)
}

/// `Rel<Length>` whose ratio part must be zero — ratio margins/gutters were
/// unrepresentable in the AST extraction and keep the same fallback.
fn rel_length_amount(rel: &Rel<Length>) -> (Option<u32>, Option<f64>) {
    if rel.rel.get() != 0.0 {
        return (None, None);
    }
    length_amount(&rel.abs)
}

fn margin_side(side: Option<&Smart<Rel<Length>>>) -> (Option<u32>, Option<f64>) {
    match side {
        Some(Smart::Custom(rel)) => rel_length_amount(rel),
        _ => (None, None),
    }
}

/// The `Alignment` → `VerticalAlign` mapping mirrors `vertical_align_of` in
/// the AST pass (`top`/`horizon`/`bottom`; `left + horizon` reads the y
/// component).
fn vertical_align(styles: StyleChain<'_>) -> Option<VerticalAlign> {
    styles
        .get_cloned(AlignElem::alignment)
        .y()
        .map(|align| match align {
            VAlignment::Top => VerticalAlign::Top,
            VAlignment::Horizon => VerticalAlign::Center,
            VAlignment::Bottom => VerticalAlign::Bottom,
        })
}

/// The stroke thickness of a `#set footnote.entry(separator: line(stroke:))`
/// line, in eighths of a point — the AST pass's `stroke_eighths`. The
/// separator content is a `LineElem` (or a small sequence containing one).
fn separator_stroke_eighths(
    content: &typst_library::foundations::Content,
    styles: StyleChain<'_>,
) -> Option<u32> {
    let line = content.to_packed::<LineElem>()?;
    let stroke = line.stroke.get_cloned(styles);
    let Smart::Custom(thickness) = stroke.thickness else {
        return None;
    };
    let pt = thickness.abs.to_pt();
    if pt <= 0.0 {
        return None;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "separator strokes are small positive lengths"
    )]
    Some((pt * 8.0).round() as u32)
}

/// Resolve every overridable property from the realize-pair chains.
pub fn chain_overrides(pairs: &[Pair<'_>]) -> SourceStyleOverrides {
    let defaults = StyleChain::default();
    let mut out = SourceStyleOverrides::default();
    let mut numberings: Vec<Numbering> = Vec::new();

    for (content, styles) in pairs {
        if content.to_packed::<TagElem>().is_none() {
            continue;
        }
        let styles = *styles;
        let ovr = &mut out;

        page_props(styles, ovr, defaults);
        if let Some(numbering) = styles.get_cloned(PageElem::numbering)
            && !numberings.contains(&numbering)
        {
            numberings.push(numbering);
        }
        if !ovr.page_furniture
            && (styles.get_cloned(PageElem::foreground).is_some()
                || styles.get_cloned(PageElem::background).is_some())
        {
            ovr.page_furniture = true;
        }
        text_props(styles, ovr, defaults);
        par_props(styles, ovr, defaults);
        footnote_props(styles, ovr, defaults);
    }
    out.numbering_changed = numberings.len() > 1;
    out
}

/// `#set page(...)` values and the header/footer distances.
fn page_props(styles: StyleChain<'_>, ovr: &mut SourceStyleOverrides, defaults: StyleChain<'_>) {
    if ovr.margin_top.is_none()
        && ovr.margin_top_em.is_none()
        && let Smart::Custom(margin) = styles.get_cloned(PageElem::margin)
    {
        apply_margin(&margin.sides, ovr);
        ovr.margins_mirrored = margin.two_sided.unwrap_or(false);
    }
    if ovr.columns.is_none() {
        let columns = styles.get(PageElem::columns);
        if columns != defaults.get(PageElem::columns) {
            ovr.columns = u32::try_from(columns.get()).ok();
        }
    }
    if ovr.header_twips.is_none() && ovr.header_em.is_none() {
        let (twips, em) = rel_length_amount(&styles.get(PageElem::header_ascent));
        ovr.header_twips = twips;
        ovr.header_em = em;
    }
    if ovr.footer_twips.is_none() && ovr.footer_em.is_none() {
        let (twips, em) = rel_length_amount(&styles.get(PageElem::footer_descent));
        ovr.footer_twips = twips;
        ovr.footer_em = em;
    }
    if ovr.line_numbering.is_none() {
        if styles.get_cloned(ParLine::numbering).is_some() {
            ovr.line_numbering = Some(true);
        }
        if let Smart::Custom(clearance) = styles.get(ParLine::number_clearance) {
            let (twips, em) = length_amount(&clearance);
            ovr.line_number_distance_twips = twips;
            ovr.line_number_distance_em = em.or_else(|| twips.map(|t| f64::from(t) / 220.0));
        }
    }
    if ovr.vertical_align.is_none() && styles.has(AlignElem::alignment) {
        ovr.vertical_align = vertical_align(styles);
    }
    if ovr.column_gutter_twips.is_none() && ovr.column_gutter_em.is_none() {
        let (twips, em) = rel_length_amount(&styles.get(ColumnsElem::gutter));
        ovr.column_gutter_twips = twips;
        ovr.column_gutter_em = em;
    }
}

/// `#set text(...)` values — size, language, hyphenation (fonts stay with
/// the AST fill-in).
fn text_props(styles: StyleChain<'_>, ovr: &mut SourceStyleOverrides, defaults: StyleChain<'_>) {
    if ovr.text_size_half_pt.is_none() {
        let size = styles.get(TextElem::size);
        if size != defaults.get(TextElem::size)
            && let (Some(twips), _) = length_amount(&size.0)
        {
            ovr.text_size_half_pt = Some(twips / 10);
        }
    }
    if ovr.text_lang.is_none() {
        let lang = styles.get(TextElem::lang);
        if lang != defaults.get(TextElem::lang) && lang != Lang::ENGLISH {
            ovr.text_lang = Some(lang.as_str().to_string());
        }
    }
    if ovr.text_region.is_none()
        && let Some(region) = styles.get_cloned(TextElem::region)
    {
        ovr.text_region = Some(region.as_str().to_string());
    }
    if ovr.text_hyphenate.is_none()
        && let Smart::Custom(false) = styles.get(TextElem::hyphenate)
    {
        ovr.text_hyphenate = Some(false);
    }
}

/// `#set par(...)` values.
fn par_props(styles: StyleChain<'_>, ovr: &mut SourceStyleOverrides, defaults: StyleChain<'_>) {
    if ovr.first_line_indent_twips.is_none() && ovr.first_line_indent_em.is_none() {
        let indent = styles.get(ParElem::first_line_indent);
        if indent != defaults.get(ParElem::first_line_indent) {
            let (twips, em) = length_amount(&indent.amount());
            ovr.first_line_indent_twips = twips;
            ovr.first_line_indent_em = em;
            if indent.all() {
                ovr.first_line_indent_all = Some(true);
            }
        }
    }
    if ovr.par_leading_twips.is_none() && ovr.par_leading_em.is_none() {
        let leading = styles.get(ParElem::leading);
        if leading != defaults.get(ParElem::leading) {
            let (twips, em) = length_amount(&leading);
            ovr.par_leading_twips = twips;
            ovr.par_leading_em = em;
        }
    }
    if ovr.par_spacing_twips.is_none() && ovr.par_spacing_em.is_none() {
        let spacing = styles.get(ParElem::spacing);
        if spacing != defaults.get(ParElem::spacing) {
            let (twips, em) = length_amount(&spacing);
            ovr.par_spacing_twips = twips;
            ovr.par_spacing_em = em;
        }
    }
    if ovr.justify.is_none() {
        let justify = styles.get(ParElem::justify);
        if justify != defaults.get(ParElem::justify) {
            ovr.justify = Some(justify);
        }
    }
}

/// `#set footnote.entry(...)` values.
fn footnote_props(
    styles: StyleChain<'_>,
    ovr: &mut SourceStyleOverrides,
    defaults: StyleChain<'_>,
) {
    if ovr.footnote_clearance_twips.is_none() && ovr.footnote_clearance_em.is_none() {
        let clearance = styles.get(FootnoteEntry::clearance);
        if clearance != defaults.get(FootnoteEntry::clearance) {
            let (twips, em) = length_amount(&clearance);
            ovr.footnote_clearance_twips = twips;
            ovr.footnote_clearance_em = em;
        }
    }
    if ovr.footnote_gap_twips.is_none() && ovr.footnote_gap_em.is_none() {
        let gap = styles.get(FootnoteEntry::gap);
        if gap != defaults.get(FootnoteEntry::gap) {
            let (twips, em) = length_amount(&gap);
            ovr.footnote_gap_twips = twips;
            ovr.footnote_gap_em = em;
        }
    }
    if ovr.footnote_indent_twips.is_none() && ovr.footnote_indent_em.is_none() {
        let indent = styles.get(FootnoteEntry::indent);
        if indent != defaults.get(FootnoteEntry::indent) {
            let (twips, em) = length_amount(&indent);
            ovr.footnote_indent_twips = twips;
            ovr.footnote_indent_em = em;
        }
    }
    if ovr.footnote_separator_stroke_eighths.is_none() {
        let separator = styles.get_cloned(FootnoteEntry::separator);
        if separator != defaults.get_cloned(FootnoteEntry::separator) {
            ovr.footnote_separator_stroke_eighths = separator_stroke_eighths(&separator, styles);
        }
    }
}

fn apply_margin(sides: &Sides<Option<Smart<Rel<Length>>>>, ovr: &mut SourceStyleOverrides) {
    let (twips, em) = margin_side(sides.top.as_ref());
    ovr.margin_top = twips;
    ovr.margin_top_em = em;
    let (twips, em) = margin_side(sides.bottom.as_ref());
    ovr.margin_bottom = twips;
    ovr.margin_bottom_em = em;
    let (twips, em) = margin_side(sides.left.as_ref());
    ovr.margin_left = twips;
    ovr.margin_left_em = em;
    let (twips, em) = margin_side(sides.right.as_ref());
    ovr.margin_right = twips;
    ovr.margin_right_em = em;
}

#[cfg(test)]
mod tests {
    use typst_library::foundations::Styles;
    use typst_library::layout::{Abs, Alignment, Em, HAlignment};

    use super::*;

    #[test]
    fn length_amount_splits_absolute_and_em() {
        assert_eq!(
            length_amount(&Length::from(Abs::pt(12.0))),
            (Some(240), None)
        );
        assert_eq!(
            length_amount(&Length::from(Em::new(1.5))),
            (None, Some(1.5))
        );
        // A mixed `pt + em` length is unrepresentable, matching the AST pass.
        assert_eq!(
            length_amount(&(Length::from(Abs::pt(4.0)) + Length::from(Em::new(1.0)))),
            (None, None)
        );
        assert_eq!(length_amount(&Length::zero()), (Some(0), None));
    }

    #[test]
    fn vertical_align_reads_declared_alignment() {
        let mut styles = Styles::new();
        styles.set(
            AlignElem::alignment,
            Alignment::from(HAlignment::Center + VAlignment::Horizon),
        );
        let chain = StyleChain::new(&styles);
        assert!(chain.has(AlignElem::alignment));
        assert_eq!(vertical_align(chain), Some(VerticalAlign::Center));
        // The implicit default alignment still reads Top, but `has` stays
        // false — the caller gates on it so an undeclared value is never
        // emitted as `w:vAlign`.
        let empty_styles = Styles::new();
        let empty = StyleChain::new(&empty_styles);
        assert!(!empty.has(AlignElem::alignment));
    }

    #[test]
    fn line_number_clearance_keeps_the_ast_em_approximation() {
        let mut styles = Styles::new();
        styles.set(
            ParLine::number_clearance,
            Smart::Custom(Length::from(Abs::pt(11.0))),
        );
        let chain = StyleChain::new(&styles);
        let mut ovr = SourceStyleOverrides::default();
        page_props(chain, &mut ovr, StyleChain::default());
        assert_eq!(ovr.line_number_distance_twips, Some(220));
        assert_eq!(ovr.line_number_distance_em, Some(1.0));
    }
}
