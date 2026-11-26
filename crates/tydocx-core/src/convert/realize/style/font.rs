use tydocx_ooxml::document::Run;
use typst_library::foundations::{Content, ShowSet, StyleChain, Styles, Transformation};
use typst_library::text::{
    FontBook, FontFamily, FontFlags, FontInfo, FontVariant, RawElem, TextElem, TextSize,
    is_default_ignorable, variant,
};
use typst_syntax::Span;
use unicode_segmentation::UnicodeSegmentation;

use super::{StyleDefaults, run_from_styles, run_properties};
use crate::convert::page::is_cjk_char;

pub(in crate::convert::realize) fn raw_text_styles(
    content: &Content,
    styles: StyleChain<'_>,
) -> Styles {
    let Some(raw) = content.to_packed::<RawElem>() else {
        return Styles::new();
    };
    let mut map = Styles::new();
    for recipe in styles.recipes() {
        if let Transformation::Style(transform) = recipe.transform()
            && recipe
                .selector()
                .is_some_and(|selector| selector.matches(content, Some(styles)))
        {
            map.apply(transform.clone());
        }
    }
    map.apply(raw.show_set(styles));
    let resolved = styles.chain(&map);
    let mut text = Styles::new();
    text.set(TextElem::font, resolved.get_cloned(TextElem::font));
    text.set(TextElem::fallback, resolved.get(TextElem::fallback));
    text.set(
        TextElem::size,
        TextSize(resolved.resolve(TextElem::size).into()),
    );
    text
}

struct Candidates<'a> {
    book: &'a FontBook,
    variant: FontVariant,
    declared: Vec<(&'a FontFamily, Option<&'a FontInfo>)>,
    fallbacks: Vec<&'a FontInfo>,
    fallback_enabled: bool,
}

impl<'a> Candidates<'a> {
    fn new(book: &'a FontBook, styles: StyleChain<'a>) -> Self {
        let variant = variant(styles);
        let info = |family: &FontFamily| {
            book.select(family.as_str(), variant)
                .and_then(|i| book.info(i))
        };
        let declared: Vec<_> = styles
            .get_ref(TextElem::font)
            .into_iter()
            .map(|family| (family, info(family)))
            .collect();
        let fallbacks = typst_library::text::families(styles)
            .skip(declared.len())
            .filter_map(info)
            .collect();
        Self {
            book,
            variant,
            declared,
            fallbacks,
            fallback_enabled: styles.get(TextElem::fallback),
        }
    }

    fn pick(&self, cluster: &str, east_asia: bool) -> Option<&'a str> {
        let chars = || cluster.chars().filter(|&c| !is_default_ignorable(c));
        let allows = |family: &FontFamily| {
            family
                .covers()
                .is_none_or(|pattern| chars().all(|c| pattern.is_match(c.encode_utf8(&mut [0; 4]))))
        };
        let covers = |info: &FontInfo| chars().all(|c| info.coverage.contains(u32::from(c)));
        let eligible = || self.declared.iter().filter(|(family, _)| allows(family));
        eligible()
            .find_map(|(_, info)| info.filter(|info| covers(info)))
            .map(|info| info.family.as_str())
            .or_else(|| self.unverified(allows, east_asia))
            .or_else(|| self.fallback(cluster, covers))
            .or_else(|| eligible().next().map(|(family, _)| family.as_str()))
    }

    fn unverified(&self, allows: impl Fn(&FontFamily) -> bool, east_asia: bool) -> Option<&'a str> {
        let mut missing = self
            .declared
            .iter()
            .filter(|(family, info)| info.is_none() && allows(family));
        let (first, _) = missing.next()?;
        if east_asia
            && first.covers().is_none()
            && self
                .declared
                .first()
                .is_some_and(|(family, _)| family == first)
            && let Some((next, _)) = missing.next()
        {
            return Some(next.as_str());
        }
        Some(first.as_str())
    }

    fn fallback(&self, cluster: &str, covers: impl Fn(&FontInfo) -> bool) -> Option<&'a str> {
        if !self.fallback_enabled {
            return None;
        }
        self.fallbacks
            .iter()
            .copied()
            .find(|info| covers(info))
            .or_else(|| {
                let first = self.declared.iter().find_map(|(_, info)| *info);
                self.book
                    .select_fallback(first, self.variant, cluster)
                    .and_then(|i| self.book.info(i))
                    .filter(|info| covers(info) && !info.flags.contains(FontFlags::MATH))
            })
            .map(|info| info.family.as_str())
    }

    fn has_split_preferences(&self) -> bool {
        self.declared.len() > 1
            || self
                .declared
                .iter()
                .any(|(family, _)| family.covers().is_some())
    }
}

#[derive(Clone, Copy, Default)]
struct Slots<'a> {
    ascii: Option<&'a str>,
    east_asia: Option<&'a str>,
}

impl<'a> Slots<'a> {
    fn for_cluster(c: char, family: Option<&'a str>, restricted: bool) -> Self {
        if !c.is_ascii() && !c.is_alphanumeric() && !is_cjk_char(c) && restricted {
            Self {
                ascii: family,
                east_asia: family,
            }
        } else if is_cjk_char(c) {
            Self {
                ascii: None,
                east_asia: family,
            }
        } else {
            Self {
                ascii: family,
                east_asia: None,
            }
        }
    }

    fn compatible(&self, other: &Self) -> bool {
        let compatible = |a: Option<&str>, b: Option<&str>| {
            a.zip(b).is_none_or(|(a, b)| a.eq_ignore_ascii_case(b))
        };
        compatible(self.ascii, other.ascii) && compatible(self.east_asia, other.east_asia)
    }

    fn merge(&mut self, other: Self) {
        self.ascii = self.ascii.or(other.ascii);
        self.east_asia = self.east_asia.or(other.east_asia);
    }

    fn apply(&self, run: &mut Run, defaults: &StyleDefaults, force_ascii: bool) {
        run.font_ascii = self
            .ascii
            .filter(|name| force_ascii || !name.eq_ignore_ascii_case(&defaults.body_font))
            .map(str::to_owned);
        run.font_east_asia = self.east_asia.map(str::to_owned);
        run.monospace = !force_ascii
            && self.east_asia.is_none()
            && self.ascii.is_some_and(|name| {
                defaults
                    .monospace_families
                    .iter()
                    .any(|f| f.eq_ignore_ascii_case(name))
            });
    }
}

pub(in crate::convert::realize) fn runs_from_styles(
    text: &str,
    styles: StyleChain<'_>,
    defaults: &StyleDefaults,
    span: Option<Span>,
    book: &FontBook,
) -> Vec<Run> {
    if !styles.has(TextElem::font) || text.trim().is_empty() {
        return vec![run_from_styles(text, styles, defaults, span)];
    }

    let mut prototype = run_properties(text, styles, defaults, span);
    let text = std::mem::take(&mut prototype.text);
    let candidates = Candidates::new(book, styles);
    let restricted = candidates.has_split_preferences();
    let primary = candidates
        .declared
        .first()
        .map(|(family, _)| family.as_str());
    let mut runs = Vec::new();
    let mut slots = Slots::default();
    let mut start = 0;
    let mut push = |start, end, slots: &Slots<'_>| {
        let mut run = prototype.clone();
        run.text.push_str(&text[start..end]);
        let force_ascii = restricted
            || slots
                .ascii
                .zip(primary)
                .is_some_and(|(a, b)| !a.eq_ignore_ascii_case(b));
        slots.apply(&mut run, defaults, force_ascii);
        runs.push(run);
    };
    for (i, cluster) in text.grapheme_indices(true) {
        let Some(c) = cluster
            .chars()
            .find(|&c| !c.is_whitespace() && !is_default_ignorable(c))
        else {
            continue;
        };
        let next = Slots::for_cluster(c, candidates.pick(cluster, is_cjk_char(c)), restricted);
        if slots.compatible(&next) {
            slots.merge(next);
        } else {
            push(start, i, &slots);
            start = i;
            slots = next;
        }
    }
    push(start, text.len(), &slots);
    runs
}

#[cfg(test)]
mod tests {
    use super::*;
    use typst_library::foundations::Styles;
    use typst_library::text::FontList;

    #[test]
    fn uninstalled_pair_can_still_name_the_east_asian_slot() {
        // On a host with neither face, keep the conventional explicit
        // Latin/CJK pair editable instead of putting the Latin name in both
        // slots. Installed coverage always wins over this inference.
        let mut styles = Styles::new();
        styles.set(
            TextElem::font,
            FontList(vec![
                FontFamily::new("TyDocx Missing Latin"),
                FontFamily::new("TyDocx Missing CJK"),
            ]),
        );
        let book = FontBook::new();
        let candidates = Candidates::new(&book, StyleChain::new(&styles));
        assert_eq!(candidates.pick("A", false), Some("tydocx missing latin"));
        assert_eq!(candidates.pick("字", true), Some("tydocx missing cjk"));
    }
}
