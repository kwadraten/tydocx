use super::{
    BlockElement, Document, HangingIndent, InlineElement, ParagraphStyle, TydocxWorld, page,
};
use crate::compat::{Degradation, DegradationAction};

/// Apply authoritative values from source AST, overriding heuristic guesses.
/// Resolve the declared first-line indent (Typst default: 0pt) onto the style.
///
/// An em-based indent additionally yields a char-based `firstLineChars`
/// (`round(em × 100)`) that Word prefers, with the twips kept as a fallback.
/// Absolute (pt/cm) indents emit only twips (`first_line_indent_chars = None`).
pub(super) fn apply_first_line_indent(
    ovr: &page::SourceStyleOverrides,
    doc: &mut Document,
    body_pt: f64,
) {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "non-negative em rounded to hundredths of a char"
    )]
    let indent = if let Some(em) = ovr.first_line_indent_em {
        doc.style.first_line_indent_chars = Some((em * 100.0).round() as u32);
        Some(page::pt_to_twips(em * body_pt))
    } else {
        doc.style.first_line_indent_chars = None;
        ovr.first_line_indent_twips
    };
    doc.style.first_line_indent_twips = indent.unwrap_or(0);
}

/// Split the source `#set text(font: …)` list into ASCII and East-Asian body
/// defaults.
///
/// The legacy convention `font: ("Times New Roman", "SimSun")` assumes the first
/// entry is Latin and the second CJK. That positional split breaks for a CJK-only
/// fallback list like `("NSimSun", "Noto Serif SC")`, where BOTH entries are CJK
/// and the second is just a glyph-coverage fallback that may never render. So the
/// source list is authoritative over WHICH declared name to emit, but geometry
/// (already detected into `doc.style` before this runs) decides WHICH entry
/// actually fired.
///
/// The cross-check only applies when a `PagedDocument` was compiled (`has_geometry`).
/// Without it, `doc.style` still holds the Default 宋体/Times New Roman, which
/// won't match declared names, so the HTML-only path keeps the legacy positional
/// split.
pub(super) fn apply_body_font_split(fonts: &[String], doc: &mut Document, has_geometry: bool) {
    if fonts.len() < 2 {
        if let Some(f) = fonts.first() {
            doc.style.body_font_ascii.clone_from(f);
            doc.style.body_font_east_asia.clone_from(f);
        }
        return;
    }
    if !has_geometry {
        doc.style.body_font_ascii.clone_from(&fonts[0]);
        doc.style.body_font_east_asia.clone_from(&fonts[1]);
        return;
    }

    // ASCII: the declared name that matches the rendered Latin font, else fonts[0].
    let ascii = fonts
        .iter()
        .find(|f| f.eq_ignore_ascii_case(&doc.style.body_font_ascii))
        .unwrap_or(&fonts[0])
        .clone();

    // EAST-ASIA: prefer the declared name geometry says actually fired — the
    // first list entry equal to the rendered CJK font. That keeps `NSimSun` from
    // a `("NSimSun", "Noto Serif SC")` list rather than the never-rendered
    // `Noto Serif SC` fallback.
    //
    // If NO declared entry matches the rendered CJK font, Typst substituted a
    // face outside the list (the typst#6205 case, e.g. `SimSun` previews as a
    // system Mincho). The author's declared CJK name should still win over that
    // substitution, so fall back to the first declared entry that isn't the Latin
    // (ASCII) slot — the legacy positional choice — not the rendered fallback.
    let east_asia = fonts
        .iter()
        .find(|f| f.eq_ignore_ascii_case(&doc.style.body_font_east_asia))
        .or_else(|| fonts.iter().find(|f| !f.eq_ignore_ascii_case(&ascii)))
        .unwrap_or(&fonts[1])
        .clone();

    doc.style.body_font_ascii = ascii;
    doc.style.body_font_east_asia = east_asia;
}

/// Apply the declared `#set page(numbering:)` format: the declared value
/// wins over glyph guessing (which runs later in `apply_headers_and_footers`);
/// an unmappable pattern falls back to glyph detection and is reported.
fn apply_page_numbering_format(
    ovr: &page::SourceStyleOverrides,
    doc: &mut Document,
    degradations: &mut Vec<Degradation>,
) {
    let Some(decl) = &ovr.page_numbering else {
        return;
    };
    match decl {
        page::PageNumberingDecl::Pattern(pattern) => {
            match page::page_number_format_from_declaration(pattern) {
                Some(fmt) => doc.page_numbering = Some(fmt),
                None => degradations.push(Degradation::new(
                    None,
                    "page-numbering-pattern",
                    DegradationAction::Literalized,
                    format!(
                        "page numbering pattern \"{pattern}\" has no OOXML equivalent; the number format is guessed from the rendered glyphs instead"
                    ),
                )),
            }
        }
        page::PageNumberingDecl::Unmappable => degradations.push(Degradation::new(
            None,
            "page-numbering-pattern",
            DegradationAction::Literalized,
            "page numbering is declared as a function or computed value, which has no OOXML equivalent; the number format is guessed from the rendered glyphs instead",
        )),
    }
}

/// Gather authoritative style overrides from the source AST: the main file plus
/// every file reachable from it through `#import`/`#include` chains. Paths
/// resolve the way Typst resolves them — relative to the importing file's
/// directory, or to the project root when they start with `/` — via
/// [`collect_reachable_source_texts`], which also visits each file once
/// (cycles are safe) and skips unreadable files silently. Only document-global
/// `set` rules count — those at a file's top level or inside the closure named
/// by the document's `#show:` template (a `set text(size:)` buried in a `#block`
/// or a non-template helper closure is local and ignored). Imported files reuse
/// the main file's template names so a template library that defines the closure
/// honors its own globals.
pub(super) fn gather_source_overrides(world: &TydocxWorld) -> page::SourceStyleOverrides {
    let main_text = world.main_source().text();
    let template_names = page::extract_show_template_names_from_source(main_text);
    let texts = page::collect_reachable_source_texts(world.root(), &world.main_dir(), main_text);
    merge_reachable_overrides(&texts, &template_names)
}

/// Whether the document resets the footnote counter in a page header (the
/// opt-in for per-page footnote numbering). Checks the main source and every
/// file reachable from it, reusing the same template names as
/// [`gather_source_overrides`].
pub(super) fn detect_footnote_restart_each_page(world: &TydocxWorld) -> bool {
    let main_text = world.main_source().text();
    let template_names = page::extract_show_template_names_from_source(main_text);
    let texts = page::collect_reachable_source_texts(world.root(), &world.main_dir(), main_text);
    texts
        .iter()
        .any(|text| page::footnote_counter_reset_in_page_header(text, &template_names))
}

/// Merge `SourceStyleOverrides` from every reachable source text, main file
/// first: the main file's values win, imported files only fill gaps.
fn merge_reachable_overrides(
    texts: &[String],
    template_names: &[String],
) -> page::SourceStyleOverrides {
    let mut overrides = page::SourceStyleOverrides::default();
    for text in texts {
        let file_overrides = page::extract_source_style_overrides(text, template_names);
        overrides.merge_from(&file_overrides);
    }
    overrides
}

/// `par.line(numbering:)` line numbering, page vertical alignment, and
/// `columns(gutter:)` — the section-level page chrome declared in source.
fn apply_page_chrome(ovr: &page::SourceStyleOverrides, doc: &mut Document, body_pt: f64) {
    // `number-clearance` is the distance; Word's default (240 twips) applies
    // when the source leaves it `auto`.
    if ovr.line_numbering == Some(true) {
        doc.page_settings.line_number_distance_twips =
            Some(ovr.line_number_distance_twips.unwrap_or(240));
        if let Some(em) = ovr.line_number_distance_em {
            doc.page_settings.line_number_distance_twips = Some(page::pt_to_twips(em * body_pt));
        }
    } else if ovr.line_numbering == Some(false) {
        doc.page_settings.line_number_distance_twips = None;
    }

    if let Some(align) = ovr.vertical_align {
        doc.page_settings.vertical_align = Some(align);
    }

    if let Some(v) = ovr.column_gutter_twips {
        doc.page_settings.column_spacing = Some(v);
    }
    if let Some(em) = ovr.column_gutter_em {
        doc.page_settings.column_spacing = Some(page::pt_to_twips(em * body_pt));
    }
}

/// Page margins and header/footer edge distances — em values resolve against
/// the body size; absolute values were already converted in the AST.
fn apply_page_edges(ovr: &page::SourceStyleOverrides, doc: &mut Document, body_pt: f64) {
    if let Some(v) = ovr.margin_top {
        doc.page_settings.margin_top = v;
    }
    if let Some(em) = ovr.margin_top_em {
        doc.page_settings.margin_top = page::pt_to_twips(em * body_pt);
    }
    if let Some(v) = ovr.margin_bottom {
        doc.page_settings.margin_bottom = v;
    }
    if let Some(em) = ovr.margin_bottom_em {
        doc.page_settings.margin_bottom = page::pt_to_twips(em * body_pt);
    }
    if let Some(v) = ovr.margin_left {
        doc.page_settings.margin_left = v;
    }
    if let Some(em) = ovr.margin_left_em {
        doc.page_settings.margin_left = page::pt_to_twips(em * body_pt);
    }
    if let Some(v) = ovr.margin_right {
        doc.page_settings.margin_right = v;
    }
    if let Some(em) = ovr.margin_right_em {
        doc.page_settings.margin_right = page::pt_to_twips(em * body_pt);
    }
    if let Some(v) = ovr.header_twips {
        doc.page_settings.header_twips = Some(v);
    }
    if let Some(em) = ovr.header_em {
        doc.page_settings.header_twips = Some(page::pt_to_twips(em * body_pt));
    }
    if let Some(v) = ovr.footer_twips {
        doc.page_settings.footer_twips = Some(v);
    }
    if let Some(em) = ovr.footer_em {
        doc.page_settings.footer_twips = Some(page::pt_to_twips(em * body_pt));
    }
}

/// `footnote.entry(clearance:/gap:/indent:/separator:)` → `w:footnotePr`.
fn apply_footnote_entry(ovr: &page::SourceStyleOverrides, doc: &mut Document, body_pt: f64) {
    if let Some(v) = ovr.footnote_clearance_twips {
        doc.style.footnote_entry.clearance_twips = Some(v);
    }
    if let Some(em) = ovr.footnote_clearance_em {
        doc.style.footnote_entry.clearance_twips = Some(page::pt_to_twips(em * body_pt));
    }
    if let Some(v) = ovr.footnote_gap_twips {
        doc.style.footnote_entry.gap_twips = Some(v);
    }
    if let Some(em) = ovr.footnote_gap_em {
        doc.style.footnote_entry.gap_twips = Some(page::pt_to_twips(em * body_pt));
    }
    if let Some(v) = ovr.footnote_indent_twips {
        doc.style.footnote_entry.indent_twips = Some(v);
    }
    if let Some(em) = ovr.footnote_indent_em {
        doc.style.footnote_entry.indent_twips = Some(page::pt_to_twips(em * body_pt));
    }
    if let Some(eighths) = ovr.footnote_separator_stroke_eighths {
        doc.style.footnote_entry.separator_stroke_eighths = Some(eighths);
    }
    doc.style.footnote_entry.justify = ovr.footnote_par_justify;
}

pub(super) fn apply_source_overrides(
    ovr: &page::SourceStyleOverrides,
    doc: &mut Document,
    has_geometry: bool,
    degradations: &mut Vec<Degradation>,
) {
    // Columns
    if let Some(cols) = ovr.columns {
        doc.page_settings.columns = Some(cols);
    }

    // Page numbering format: the declared value wins over glyph guessing.
    apply_page_numbering_format(ovr, doc, degradations);

    // Body text font — split into ASCII and East-Asian defaults.
    if let Some(fonts) = &ovr.text_font {
        apply_body_font_split(fonts, doc, has_geometry);
    }

    // Body text size
    if let Some(sz) = ovr.text_size_half_pt {
        doc.style.body_size_half_pt = sz;
    }

    apply_language_override(ovr, doc);

    if ovr.text_hyphenate == Some(false) {
        doc.style.suppress_auto_hyphens = true;
    }

    // Resolve em-based values using actual body size
    let body_pt = f64::from(doc.style.body_size_half_pt) / 2.0;

    apply_page_edges(ovr, doc, body_pt);
    apply_page_chrome(ovr, doc, body_pt);
    apply_footnote_entry(ovr, doc, body_pt);

    apply_first_line_indent(ovr, doc, body_pt);
    if let Some(all) = ovr.first_line_indent_all {
        doc.style.first_line_indent_all = all;
    }

    // Leading (in pt) — needed below for paragraph spacing calculation.
    let leading_pt = if let Some(em) = ovr.par_leading_em {
        em * body_pt
    } else if let Some(twips) = ovr.par_leading_twips {
        f64::from(twips) / 20.0
    } else {
        0.65 * body_pt
    };

    // Body paragraph spacing: Typst's par.spacing replaces leading in the gap
    // between paragraphs. Word adds w:after on top of line pitch.
    // To compensate: w:after = max(0, par_spacing - leading).
    let par_spacing_pt = if let Some(em) = ovr.par_spacing_em {
        em * body_pt
    } else if let Some(twips) = ovr.par_spacing_twips {
        f64::from(twips) / 20.0
    } else {
        1.2 * body_pt
    };
    let after_extra = if par_spacing_pt > leading_pt {
        page::pt_to_twips(par_spacing_pt - leading_pt)
    } else {
        0
    };
    doc.style.body_spacing_before = 0;
    doc.style.body_spacing_after = after_extra;

    // Line spacing: cap_height (from font metrics) + leading (from source AST).
    // Typst's line pitch = cap_height × font_size + leading, where cap_height
    // is the default top-edge metric (not ascender). We emit this as
    // w:lineRule="atLeast" in twips for precise control.
    {
        let cap_height_pt = doc.style.body_cap_height_ratio * body_pt;
        let line_pitch_pt = cap_height_pt + leading_pt;
        doc.style.line_spacing = page::pt_to_twips(line_pitch_pt);
    }

    // Paragraph justification
    if let Some(justify) = ovr.justify {
        doc.style.body_alignment = if justify {
            "both".to_string()
        } else {
            "left".to_string()
        };
    }

    // Heading spacing: Typst uses block-level margin collapsing.
    // In Typst, gap = descent + max(heading_above, par_spacing, leading) + ascent.
    // In Word, gap = line_pitch + body.after + heading.before.
    // Since body.after = max(0, par_spacing - leading), heading.before should
    // add just the excess of heading.above beyond (body.after + leading).
    {
        let scales = [1.4_f64, 1.2, 1.0, 1.0, 1.0];
        for (level, &scale) in scales.iter().enumerate() {
            let heading_pt = f64::from(doc.style.heading_sizes[level]) / 2.0;
            let above_em = if level == 0 { 1.8 } else { 1.44 } / scale;
            let below_em = 0.75 / scale;
            let above_pt = above_em * heading_pt;
            let below_pt = below_em * heading_pt;
            let effective_after = f64::from(after_extra) / 20.0 + leading_pt;
            doc.style.heading_spacing_before[level] = if above_pt > effective_after {
                page::pt_to_twips(above_pt - effective_after)
            } else {
                0
            };
            let below_effective = below_pt.max(par_spacing_pt);
            doc.style.heading_spacing_after[level] = if below_effective > leading_pt {
                page::pt_to_twips(below_effective - leading_pt)
            } else {
                0
            };
        }
    }
}

/// Apply the document language from `#set text(lang:, region:)`, overriding the
/// CJK-presence heuristic. CJK languages drive Word's East-Asian tag; all others
/// drive the Latin tag (the `w:lang` w:val / w:eastAsia split). No-op when the
/// source declares no language.
///
/// A declared CJK language also marks the document as CJK content: the
/// East-Asian hint must be emitted even when the sampled pages render no CJK
/// glyph (e.g. a CJK template whose body is Latin-only) — the declaration,
/// not the glyphs, decides the document script.
pub(super) fn apply_language_override(ovr: &page::SourceStyleOverrides, doc: &mut Document) {
    let Some(lang) = &ovr.text_lang else {
        return;
    };
    let tag = page::lang_region_to_bcp47(lang, ovr.text_region.as_deref());
    if page::is_cjk_language(lang) {
        doc.style.lang_east_asia = tag;
        doc.style.has_cjk_content = true;
    } else {
        doc.style.lang_latin = tag;
    }
}
/// Set the document title from the first heading's text.
/// Extract document metadata (title, author) from `#set document(...)` if present,
/// falling back to the first heading text for the title.
/// Apply `#set par(hanging-indent: …)` from the source AST to the paragraphs it
/// governs. Each rule applies from its byte offset onward; a paragraph adopts a
/// hanging indent when the last rule at or before its earliest run is non-zero.
/// Runs whose spans don't resolve into the main source (imported helper output,
/// detached content) are skipped automatically. Imported document-template set
/// rules are resolved separately and apply to the main-source body spans.
pub(super) fn apply_hanging_indent_from_source(world: &TydocxWorld, doc: &mut Document) {
    let source = world.main_source();
    let rules = page::collect_par_hanging_indent_rules(world);
    if rules.is_empty() {
        return;
    }
    let body_size_pt = f64::from(doc.style.body_size_half_pt) / 2.0;
    for element in &mut doc.body.elements {
        // BibliographyBlock owns its hanging indent (the doc-bibliography path);
        // only plain body paragraphs are governed here. List items, headings,
        // code blocks, and rule paragraphs carry their own indent model (a list
        // item's own list hanging indent must win, not be clobbered by this), so
        // they are skipped.
        let BlockElement::Paragraph(p) = element else {
            continue;
        };
        if p.list_info.is_some()
            || p.code_block
            || p.horizontal_rule
            || matches!(p.style, Some(ParagraphStyle::Heading(_)))
        {
            continue;
        }
        // The paragraph's source position is its earliest run that resolves into
        // the main source.
        let Some(offset) = p
            .inlines
            .iter()
            .filter_map(|inline| match inline {
                InlineElement::Text(run) => run.span,
                _ => None,
            })
            // `Source::range` now takes a decomposed `(SpanNumber, Option<SubRange>)`;
            // `WorldExt::range` does that decomposition for a `Span`. Keep the
            // main-source-only behavior by skipping spans from other files
            // (imported templates), which previously yielded `None` here.
            .filter(|span| span.id() == Some(source.id()))
            .filter_map(|span| typst_library::WorldExt::range(world, span).map(|r| r.start))
            .min()
        else {
            continue;
        };
        // The active rule is the last one at or before this paragraph. Only turn
        // the indent ON (a reset rule leaves it off); never clear one set
        // elsewhere.
        let active = rules.partition_point(|r| r.offset <= offset);
        let rule = rules[..active]
            .iter()
            .rev()
            .find(|rule| rule.scope_end.is_none_or(|end| offset < end));
        if let Some(rule) = rule.filter(|rule| rule.nonzero) {
            let relative_twips = rule.em.map_or(0, |em| page::pt_to_twips(body_size_pt * em));
            p.hanging_indent = Some(HangingIndent::Twips(
                relative_twips.saturating_add(rule.twips.unwrap_or(0)),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_reachable_overrides_main_wins_imports_fill_gaps() {
        // The main file's size wins; the imported file's indent (absent from
        // the main file) fills the gap.
        let main = "#set text(size: 12pt)".to_string();
        let imported = "#set text(size: 20pt)\n#set par(first-line-indent: 2em)".to_string();
        let merged = merge_reachable_overrides(&[main, imported], &[]);
        assert_eq!(merged.text_size_half_pt, Some(24));
        assert_eq!(merged.first_line_indent_em, Some(2.0));
    }

    #[test]
    fn merge_reachable_overrides_empty_stays_default() {
        let merged = merge_reachable_overrides(&[], &[]);
        assert_eq!(merged.text_size_half_pt, None);
        assert_eq!(merged.first_line_indent_em, None);
    }
}
