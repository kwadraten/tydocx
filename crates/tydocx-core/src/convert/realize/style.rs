use tydocx_ooxml::document::Run;
use typst_library::foundations::{Smart, StyleChain, Styles};
use typst_library::layout::Abs;
use typst_library::routines::Pair;
use typst_library::text::{
    DecoLine, FontBook, FontFlags, FontStyle, FontWeight, NumberType, NumberWidth, ScriptKind,
    TextElem, variant,
};
use typst_syntax::{Source, Span};

use crate::convert::highlight::{HighlightStyle, highlight_style};
use crate::convert::page::{self, extract_non_black_color};

mod font;

pub(super) use font::{raw_text_styles, runs_from_styles};

#[derive(Debug, Clone)]
pub struct StyleDefaults {
    pub size_half_pt: u32,
    pub inherited_size_half_pt: Option<u32>,
    pub body_font: String,
    pub font_east_asia: String,
    pub monospace_families: Vec<String>,
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "sizes are small and clamped to at least 1.0 first"
)]
pub(super) fn text_size_half_pt(styles: StyleChain<'_>) -> u32 {
    let size_pt = styles.resolve(TextElem::size).to_pt();
    (size_pt * 2.0).round().max(1.0) as u32
}

pub(super) fn additional_root_styles(pairs: &[Pair<'_>], initial: StyleChain<'_>) -> Styles {
    let inherited = Styles::root(&[], initial);
    let root = Styles::root(pairs, initial);
    let inherited_prefix = inherited
        .as_slice()
        .iter()
        .zip(root.as_slice())
        .take_while(|(initial, root)| initial == root)
        .count();
    root.into_iter().skip(inherited_prefix).collect()
}

pub(super) fn declared_root_fonts(
    styles: StyleChain<'_>,
    source: Option<&Source>,
) -> Option<Vec<String>> {
    if !styles.has(TextElem::font) {
        return None;
    }

    let mut fonts: Vec<String> = styles
        .get_ref(TextElem::font)
        .into_iter()
        .map(|family| family.as_str().to_owned())
        .collect();
    if let Some(source) = source {
        let text = source.text();
        let names = page::extract_show_template_names_from_source(text);
        if let Some(spelled) = page::extract_source_style_overrides(text, &names).text_font {
            for font in &mut fonts {
                if let Some(name) = spelled.iter().find(|name| name.to_lowercase() == *font) {
                    font.clone_from(name);
                }
            }
        }
    }
    Some(fonts)
}

pub fn defaults_from_root(styles: StyleChain<'_>) -> StyleDefaults {
    let body_font = typst_library::text::families(styles).next().map_or_else(
        || "Times New Roman".to_string(),
        |family| family.as_str().to_owned(),
    );
    let size_half_pt = text_size_half_pt(styles);
    StyleDefaults {
        size_half_pt,
        inherited_size_half_pt: styles.has(TextElem::size).then_some(size_half_pt),
        font_east_asia: body_font.clone(),
        body_font,
        monospace_families: Vec::new(),
    }
}

pub fn monospace_families(book: &FontBook, body_font: &str) -> Vec<String> {
    let mut families = Vec::new();
    for (name, infos) in book.families() {
        if name.eq_ignore_ascii_case(body_font) {
            continue;
        }
        if infos
            .filter_map(|index| book.info(index))
            .any(|info| info.flags.contains(FontFlags::MONOSPACE))
        {
            families.push(name.to_lowercase());
        }
    }
    families
}

pub fn run_from_styles(
    text: &str,
    styles: StyleChain<'_>,
    defaults: &StyleDefaults,
    span: Option<Span>,
) -> Run {
    let mut run = run_properties(text, styles, defaults, span);
    if let Some(first) = typst_library::text::families(styles).next() {
        let family = first.as_str();
        if !family.eq_ignore_ascii_case(&defaults.body_font) {
            run.font_ascii = Some(family.to_owned());
        }
        if contains_cjk(&run.text)
            && styles.has(TextElem::font)
            && !family.eq_ignore_ascii_case(&defaults.font_east_asia)
        {
            run.font_east_asia = Some(family.to_owned());
        }
        run.monospace = defaults.monospace_families.iter().any(|f| f == family);
    }
    run
}

fn run_properties(
    text: &str,
    styles: StyleChain<'_>,
    defaults: &StyleDefaults,
    span: Option<Span>,
) -> Run {
    let case = styles.get(TextElem::case);
    let text = match case {
        Some(case) => case.apply(text),
        None => text.to_owned(),
    };
    let mut run = Run::new(text);
    run.span = span;

    let variant = variant(styles);
    run.bold = variant.weight >= FontWeight::SEMIBOLD;
    run.italic = variant.style != FontStyle::Normal;

    let size_half_pt = text_size_half_pt(styles);
    if Some(size_half_pt) != defaults.inherited_size_half_pt {
        run.size_half_pt = Some(size_half_pt);
    }

    let tracking = styles.resolve(TextElem::tracking);
    if tracking > Abs::zero() {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "tracking is a small non-negative length"
        )]
        {
            run.tracking_twips = Some((tracking.to_pt() * 20.0).round().max(1.0) as u32);
        }
    }

    if let Some(color) = extract_non_black_color(&styles.get_cloned(TextElem::fill)) {
        run.color = Some(color);
    }

    for decoration in styles.get_cloned(TextElem::deco) {
        match decoration.line {
            DecoLine::Underline { .. } => run.underline = true,
            DecoLine::Strikethrough { .. } => run.strikethrough = true,
            DecoLine::Overline { .. } => run.overline = true,
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

    apply_opentype_props(&mut run, styles);

    run
}

/// `text(ligatures:/number-type:/number-width:/features:/baseline:)` → the
/// run's OOXML OpenType properties and baseline position.
fn apply_opentype_props(run: &mut Run, styles: StyleChain<'_>) {
    if !styles.get(TextElem::ligatures) {
        run.ligatures = Some("none");
    } else if styles.get(TextElem::discretionary_ligatures) {
        run.ligatures = Some("contextual");
    } else if styles.get(TextElem::historical_ligatures) {
        run.ligatures = Some("historical");
    }
    match styles.get(TextElem::number_type) {
        Smart::Custom(NumberType::Lining) => run.num_form = Some("lining"),
        Smart::Custom(NumberType::OldStyle) => run.num_form = Some("oldStyle"),
        _ => {}
    }
    match styles.get(TextElem::number_width) {
        Smart::Custom(NumberWidth::Proportional) => run.number_spacing = Some("proportional"),
        Smart::Custom(NumberWidth::Tabular) => run.number_spacing = Some("tabular"),
        _ => {}
    }
    for (tag, value) in &styles.get_cloned(TextElem::features).0 {
        if *value != 0 && matches!(tag.to_str_lossy().as_ref(), "smcp" | "c2sc") {
            run.smallcaps = true;
        }
    }
    let baseline = styles.resolve(TextElem::baseline);
    #[allow(
        clippy::cast_possible_truncation,
        reason = "baseline shifts stay far below i32 range"
    )]
    let baseline_half_pt = (baseline.to_pt() * 2.0).round() as i32;
    if baseline_half_pt != 0 {
        run.position_half_pt = Some(baseline_half_pt);
    }
}

pub fn contains_cjk(text: &str) -> bool {
    text.chars().any(page::is_cjk_char)
}

pub fn lower_runs_match(prev: &Run, next: &Run) -> bool {
    prev.format_key() == next.format_key()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::{DocxDocument, export_ir};
    use crate::{ConvertOptions, TydocxWorld};
    use std::path::Path;
    use typst::{Library, World};
    use typst_layout::PagedDocument;
    use typst_library::diag::FileResult;
    use typst_library::foundations::{Bytes, Datetime, Duration};
    use typst_library::layout::{Em, Frame, FrameItem};
    use typst_library::text::{
        Case, Coverage, Font, FontFamily, FontInfo, FontList, FontVariant, TextSize,
    };
    use typst_library::visualize::Paint;
    use typst_syntax::FileId;
    use typst_utils::LazyHash;

    struct HostWorld<'a> {
        base: &'a TydocxWorld,
        library: LazyHash<Library>,
    }

    impl World for HostWorld<'_> {
        fn library(&self) -> &LazyHash<Library> {
            &self.library
        }

        fn book(&self) -> &LazyHash<FontBook> {
            self.base.book()
        }

        fn main(&self) -> FileId {
            self.base.main()
        }

        fn source(&self, id: FileId) -> FileResult<Source> {
            self.base.source(id)
        }

        fn file(&self, id: FileId) -> FileResult<Bytes> {
            self.base.file(id)
        }

        fn font(&self, index: usize) -> Option<Font> {
            self.base.font(index)
        }

        fn today(&self, offset: Option<Duration>) -> Option<Datetime> {
            self.base.today(offset)
        }
    }

    fn first_text_size(frame: &Frame) -> Option<Abs> {
        frame.items().find_map(|(_, item)| match item {
            FrameItem::Text(text) => Some(text.size),
            FrameItem::Group(group) => first_text_size(&group.frame),
            _ => None,
        })
    }

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
        assert_eq!(defaults.inherited_size_half_pt, None);
        assert_eq!(defaults.body_font, "libertinus serif");
        assert_eq!(defaults.font_east_asia, "libertinus serif");
    }

    #[test]
    fn host_relative_text_size_is_applied_once() {
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/style_default.typ");
        let base = TydocxWorld::new(&fixture).expect("open unstyled fixture");
        for outside in [true, false] {
            let mut library = (**base.library()).clone();
            let styles = Styles::from(TextElem::size.set(TextSize(Em::new(2.0).into())).wrap());
            library.styles = if outside { styles.outside() } else { styles };
            let world = HostWorld {
                base: &base,
                library: LazyHash::new(library),
            };
            let paged = typst::compile::<PagedDocument>(&world)
                .output
                .expect("native Typst compile");
            let docx = typst::compile::<DocxDocument>(&world)
                .output
                .expect("DOCX compile");
            let outcome = export_ir(&docx, Some(&paged), &base, &ConvertOptions::default());
            // Typst folds 2em over its 11pt default. An outside host style
            // also appears in Styles::root, but it must not fold twice.
            assert_eq!(
                first_text_size(&paged.pages()[0].frame),
                Some(Abs::pt(22.0))
            );
            assert_eq!(outcome.document.style.body_size_half_pt, 44);
        }
    }

    #[test]
    fn defaults_from_root_does_not_assume_a_separate_east_asian_family() {
        let mut styles = root_styles();
        styles.set(
            TextElem::font,
            FontList(vec![
                FontFamily::new("Libertinus Serif"),
                FontFamily::new("Noto Serif CJK SC"),
            ]),
        );
        let defaults = defaults_from_root(StyleChain::new(&styles));
        assert_eq!(defaults.body_font, "libertinus serif");
        assert_eq!(defaults.font_east_asia, "libertinus serif");
    }

    #[test]
    fn run_from_styles_cjk_run_gets_no_spurious_east_asia_override() {
        let defaults = StyleDefaults {
            size_half_pt: 22,
            inherited_size_half_pt: None,
            body_font: "libertinus serif".to_owned(),
            font_east_asia: "segoe ui emoji".to_owned(),
            monospace_families: Vec::new(),
        };
        let styles = root_styles();
        let run = run_from_styles("中文", StyleChain::new(&styles), &defaults, None);
        assert_eq!(run.font_east_asia, None);
    }

    #[test]
    fn run_from_styles_matching_declared_body_size_stays_implicit() {
        let mut styles = root_styles();
        styles.set(TextElem::size, TextSize(Abs::pt(13.0).into()));
        let defaults = defaults_from_root(StyleChain::new(&styles));
        let run = run_from_styles("hello", StyleChain::new(&styles), &defaults, None);
        assert_eq!(run.text, "hello");
        assert!(!run.bold);
        assert!(!run.italic);
        assert_eq!(run.size_half_pt, None);
        assert_eq!(run.font_ascii, None);
        assert_eq!(run.color, None);
    }

    #[test]
    fn guessed_body_size_cannot_remove_a_run_size() {
        let styles = root_styles();
        let defaults = defaults_from_root(StyleChain::new(&styles));
        let run = run_from_styles("hello", StyleChain::new(&styles), &defaults, None);
        // The Paged histogram (or source fallback) may choose a different
        // Normal size even when this run matches the collection-time default.
        assert_eq!(run.size_half_pt, Some(22));
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
        let run = run_from_styles("hi", StyleChain::new(&styles), &defaults, None);
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
        let run = run_from_styles("abc", StyleChain::new(&styles), &defaults, None);
        assert_eq!(run.text, "ABC");
        assert!(run.smallcaps);
    }

    fn font_info(family: &str, flags: FontFlags) -> FontInfo {
        FontInfo {
            family: family.to_string(),
            variant: FontVariant::default(),
            flags,
            axes: Vec::new(),
            coverage: Coverage::from_vec(Vec::new()),
        }
    }

    #[test]
    fn monospace_families_flags_not_name_substrings() {
        let book = FontBook::from_infos([
            font_info("Libertinus Serif", FontFlags::SERIF),
            font_info("Foxit Fixed", FontFlags::MONOSPACE),
            font_info("Faux Mono Pro", FontFlags::SERIF),
        ]);
        let families = monospace_families(&book, "libertinus serif");
        assert_eq!(families, vec!["foxit fixed"]);
    }

    #[test]
    fn monospace_families_excludes_body_font() {
        let book = FontBook::from_infos([
            font_info("DejaVu Sans Mono", FontFlags::MONOSPACE),
            font_info("Libertinus Serif", FontFlags::SERIF),
        ]);
        assert!(monospace_families(&book, "dejavu sans mono").is_empty());
    }

    #[test]
    fn run_from_styles_mono_body_family_is_not_flagged() {
        let mut styles = root_styles();
        styles.set(
            TextElem::font,
            FontList(vec![FontFamily::new("DejaVu Sans Mono")]),
        );
        let defaults = StyleDefaults {
            size_half_pt: 22,
            inherited_size_half_pt: None,
            body_font: "dejavu sans mono".to_owned(),
            font_east_asia: "dejavu sans mono".to_owned(),
            monospace_families: Vec::new(),
        };
        let run = run_from_styles("body", StyleChain::new(&styles), &defaults, None);
        assert!(!run.monospace);
        assert_eq!(run.font_ascii, None);
    }

    #[test]
    fn run_from_styles_flags_book_verified_mono_family() {
        let mut styles = root_styles();
        styles.set(
            TextElem::font,
            FontList(vec![FontFamily::new("Foxit Fixed")]),
        );
        let defaults = StyleDefaults {
            size_half_pt: 22,
            inherited_size_half_pt: None,
            body_font: "libertinus serif".to_owned(),
            font_east_asia: "libertinus serif".to_owned(),
            monospace_families: vec!["foxit fixed".to_owned()],
        };
        let run = run_from_styles("code", StyleChain::new(&styles), &defaults, None);
        assert!(run.monospace);
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
