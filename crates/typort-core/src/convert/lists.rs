use std::fmt::Write as _;

use codex::numeral_systems::NamedNumeralSystem;
use typort_ooxml::document::LevelOverride;
use typst::foundations::StyleChain;
use typst::model::Numbering;
use typst_library::model::{EnumElem, ListElem, ListMarker};
use typst_library::text::TextElem;

use crate::compat::{CompatMode, Degradation, DegradationAction};

use super::{
    HtmlDocument, HtmlElement, HtmlNode, InlineFmt, InlineOptions, ListInfo, Location, Paragraph,
    Tag, WalkCtx, collect_inlines, element_at_location, find_first_element, find_tag_end,
    get_attr_value, tag_name, tag_span, walk_tags,
};

/// The OOXML `numFmt` + `lvlText` for one numbering level, derived from a
/// Typst numbering pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelDef {
    /// OOXML `w:numFmt` value (`"decimal"`, `"lowerLetter"`, ...).
    pub num_fmt: &'static str,
    /// OOXML `w:lvlText` (e.g. `"%1)"` for an `"a)"` enum).
    pub lvl_text: String,
}

/// Whether a level definition equals the shared abstract definition for that
/// level (decimal `"%{n}."`), i.e. no `w:lvlOverride` is needed for it.
fn is_default_level_def(def: &LevelDef, level: u32) -> bool {
    def.num_fmt == "decimal" && def.lvl_text == format!("%{}.", level + 1)
}

/// Derive the OOXML level definition for piece `level` (0-based) of a Typst
/// numbering pattern.
///
/// Typst's multi-piece patterns apply to nested enums piece by piece
/// (`"1.a)"` numbers outer items `"1)"` and inner items `"a)"`), so the
/// caller passes the piece index (nesting depth relative to the enum that
/// introduced the pattern); deeper levels cycle the last piece, mirroring
/// `NumberingPattern::apply`. `full` selects Typst's `full` rendering, where
/// a nested label includes its parent numbers (`"%1.%2)"`).
///
/// Returns `None` for closure numberings (no static pattern to map) and for
/// numeral systems with no OOXML equivalent (Greek, Hebrew, ...); those
/// levels keep the shared decimal definition.
#[must_use]
pub fn numbering_level_def(numbering: &Numbering, level: u32, full: bool) -> Option<LevelDef> {
    let Numbering::Pattern(pattern) = numbering else {
        return None;
    };
    let pieces = &pattern.pieces;
    if pieces.is_empty() {
        return None;
    }
    let idx = (level as usize).min(pieces.len() - 1);
    let num_fmt = match pieces[idx].1 {
        NamedNumeralSystem::Arabic => "decimal",
        NamedNumeralSystem::LowerLatin => "lowerLetter",
        NamedNumeralSystem::UpperLatin => "upperLetter",
        NamedNumeralSystem::LowerRoman => "lowerRoman",
        NamedNumeralSystem::UpperRoman => "upperRoman",
        _ => return None,
    };
    let mut lvl_text = String::new();
    if full {
        // Each segment is `prefix + "%{n}"`; the pattern suffix closes it.
        for j in 0..=level as usize {
            let (prefix, _) = &pieces[j.min(pieces.len() - 1)];
            lvl_text.push_str(prefix);
            let _ = write!(lvl_text, "%{}", j + 1);
        }
        lvl_text.push_str(&pattern.suffix);
    } else {
        lvl_text.push_str(&pieces[0].0);
        let _ = write!(lvl_text, "%{}", level + 1);
        lvl_text.push_str(&pattern.suffix);
    }
    Some(LevelDef { num_fmt, lvl_text })
}

/// A resolved enum numbering pattern with its `full` flag, threaded from the
/// list's Tag location down to the per-level override collection.
#[derive(Debug, Clone)]
pub struct EnumNumbering {
    pub numbering: Numbering,
    pub full: bool,
}

/// The default list marker (Typst's `ListElem.marker` default): bullet,
/// triangular bullet, en-dash, cycling by depth. The field default is
/// materialized, so `as_option` is always `Some` — compare the value
/// against this instead of testing presence.
fn default_list_marker() -> ListMarker {
    ListMarker::Content(vec![
        TextElem::packed('\u{2022}'),
        TextElem::packed('\u{2023}'),
        TextElem::packed('\u{2013}'),
    ])
}

/// Resolve an enum's numbering pattern from its introspection location.
/// Returns `None` when the element can't be queried (the list keeps the
/// shared decimal definition).
pub(super) fn enum_numbering_at(
    html_doc: &HtmlDocument,
    location: Location,
) -> Option<EnumNumbering> {
    let elem = element_at_location::<EnumElem>(html_doc, location)?;
    // Settable fields: `as_option` reads the value stored on the realized
    // element (`None` = the field was never set, keep the default).
    let numbering = elem.numbering.as_option().as_ref()?.clone();
    let full = *elem.full.as_option().as_ref().unwrap_or(&false);
    Some(EnumNumbering { numbering, full })
}

/// Find the enum element's location for a `<ol>`/`<ul>` at `idx` in `nodes`:
/// the nearest preceding `Tag::Start` named `"enum"`. The HTML tree carries
/// the enum's Tag marker as a sibling immediately before its `<ol>`, both at
/// the top level (inside the walked slice) and in table cells (inside the
/// `<td>`), so a backwards scan finds it; a stale marker from a previous
/// list can't intervene because lists are properly nested.
pub(super) fn enum_location_before(nodes: &[HtmlNode], idx: usize) -> Option<Location> {
    for node in nodes[..idx.min(nodes.len())].iter().rev() {
        if let HtmlNode::Tag(tag) = node
            && let Tag::Start(content, _) = tag
            && content.elem().name() == "enum"
            && let Some(location) = content.location()
        {
            return Some(location);
        }
    }
    None
}

pub(super) fn handle_list_tag(
    children: &[HtmlNode],
    i: usize,
    tag: &Tag,
    ordered: bool,
    ctx: &mut WalkCtx,
) -> usize {
    let location = tag.location();
    let end = find_tag_end(children, i, location);
    let numbering = enum_numbering_at(ctx.html_doc, location);
    // U24/U25 (C1 only): a custom list marker and a reversed enum have no
    // OOXML equivalent; the list falls back to the default marker/order.
    if ctx.compat == CompatMode::C1 {
        if ordered {
            if let Some(enum_elem) = element_at_location::<EnumElem>(ctx.html_doc, location)
                && let Some(true) = enum_elem.reversed.as_option().as_ref()
            {
                ctx.degradations.push(Degradation::new(
                    tag_span(tag),
                    "enum-reversed",
                    DegradationAction::Literalized,
                    "reversed enum is not representable; items are numbered in order",
                ));
            }
        } else if let Some(list_elem) = element_at_location::<ListElem>(ctx.html_doc, location)
            && list_elem.marker.get_ref(StyleChain::default()) != &default_list_marker()
        {
            ctx.degradations.push(Degradation::new(
                tag_span(tag),
                "list-marker",
                DegradationAction::Literalized,
                "custom list marker is not representable; the default bullet is used",
            ));
        }
    }
    handle_list(&children[i..=end], ordered, numbering, ctx);
    end
}

/// Handle a `list` or `enum` Tag: find the HTML `<ul>` or `<ol>` element in the inner
/// children and parse it.
pub(super) fn handle_list(
    slice: &[HtmlNode],
    ordered: bool,
    numbering: Option<EnumNumbering>,
    ctx: &mut WalkCtx,
) {
    let list_tag = if ordered { "ol" } else { "ul" };
    if let Some(list) = find_first_element(slice, &|element| tag_name(element) == list_tag) {
        convert_html_list(list, ordered, numbering, ctx);
        return;
    }
    // Fallback: walk inner children normally
    let inner = &slice[1..slice.len().saturating_sub(1)];
    walk_tags(inner, ctx);
}

/// Convert an HTML `<ol>` or `<ul>` element into list paragraphs.
pub(super) fn convert_html_list(
    elem: &HtmlElement,
    ordered: bool,
    numbering: Option<EnumNumbering>,
    ctx: &mut WalkCtx,
) {
    let paras =
        convert_html_list_to_paragraphs(elem, ordered, numbering, ctx, InlineFmt::default());
    for para in paras {
        ctx.doc.add_paragraph(para);
    }
}

/// Convert an HTML `<ol>` or `<ul>` element into list paragraphs, returned
/// instead of appended to the document — so table cells can reuse the same
/// list handling for cell paragraphs (upstream #31).
pub(super) fn convert_html_list_to_paragraphs(
    elem: &HtmlElement,
    ordered: bool,
    numbering: Option<EnumNumbering>,
    ctx: &mut WalkCtx,
    fmt: InlineFmt,
) -> Vec<Paragraph> {
    // typst-html carries `#enum(start: N)` as `<ol start="N">`; Word needs it
    // back as the numbering instance's level-0 startOverride.
    let start = get_attr_value(elem, "start")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(1);
    let list_id = ctx.doc.allocate_list_id(ordered, start);
    // Collect one level definition per nesting level the walk actually
    // reaches (first writer wins per level); levels whose definition equals
    // the shared abstract one need no override.
    let state = numbering.map(|n| PatternState {
        numbering: n.numbering,
        full: n.full,
        origin: 0,
    });
    let mut walker = ListWalker {
        html_doc: ctx.html_doc,
        fmt,
        list_id,
        level_defs: Vec::new(),
    };
    let mut paras = Vec::new();
    walker.walk(elem, &mut paras, 0, state.as_ref());
    let overrides: Vec<LevelOverride> = walker
        .level_defs
        .into_iter()
        .enumerate()
        .filter_map(|(level, def)| {
            let level = u32::try_from(level).ok()?;
            let def = def?;
            if is_default_level_def(&def, level) {
                return None;
            }
            Some(LevelOverride {
                ilvl: level,
                num_fmt: def.num_fmt,
                lvl_text: def.lvl_text,
            })
        })
        .collect();
    if !overrides.is_empty() {
        if ctx.compat == CompatMode::C1 {
            // M1: C1 keeps the shared decimal definition; the enum's
            // numbering pattern is not linked. Span-less: the overrides
            // come from the whole nested walk, not one element — the §4.1
            // dedup collapses a document's enums to the first plus a count.
            let first = overrides.first().map_or("?", |o| o.lvl_text.as_str());
            ctx.degradations.push(Degradation::new(
                None,
                "enum-numbering",
                DegradationAction::Literalized,
                format!("enum numbering pattern ({first}) fell back to decimal"),
            ));
        } else {
            ctx.doc.set_list_overrides(list_id, overrides);
        }
    }
    paras
}

/// The numbering pattern in force at one nesting depth, with the depth at
/// which it was introduced: a nested enum with a different `set` rule resets
/// the piece indexing at its own depth, while an inherited pattern keeps
/// indexing from the outermost enum.
#[derive(Debug, Clone)]
pub(super) struct PatternState {
    numbering: Numbering,
    full: bool,
    origin: u32,
}

/// Shared state for one `<ol>`/`<ul>` walk: the document context, the list's
/// numbering instance, and the per-level definitions the walk collects.
/// (Bundled in a struct rather than threaded as positional parameters, per
/// the repo's argument-threading rule.)
struct ListWalker<'a> {
    html_doc: &'a HtmlDocument,
    fmt: InlineFmt,
    list_id: u32,
    level_defs: Vec<Option<LevelDef>>,
}

impl ListWalker<'_> {
    /// Walk one `<ol>`/`<ul>` level, appending its items' paragraphs to
    /// `sink` and recording this level's numbering definition.
    fn walk(
        &mut self,
        elem: &HtmlElement,
        sink: &mut Vec<Paragraph>,
        level: u32,
        state: Option<&PatternState>,
    ) {
        let html_doc = self.html_doc;
        let fmt = self.fmt;
        let list_id = self.list_id;
        // Record this level's definition from the pattern in force (first
        // writer wins: sibling sub-enums at one depth share a single OOXML
        // level).
        if level as usize >= self.level_defs.len() {
            self.level_defs.resize(level as usize + 1, None);
        }
        if self.level_defs[level as usize].is_none()
            && let Some(state) = state
            && let Some(def) = numbering_level_def(
                &state.numbering,
                level.saturating_sub(state.origin),
                state.full,
            )
        {
            self.level_defs[level as usize] = Some(def);
        }
        let is_sublist = |c: &HtmlNode| {
            matches!(c, HtmlNode::Element(el) if {
                let t = tag_name(el);
                t == "ul" || t == "ol"
            })
        };
        for child in &elem.children {
            if let HtmlNode::Element(li) = child
                && tag_name(li) == "li"
            {
                let mut para = Paragraph::new();
                para.list_info = Some(ListInfo { id: list_id, level });
                // Route the item's direct inline content (everything but nested
                // sub-lists) through the standard inline collector, so equation
                // Tags produce OMML and their sibling MathML `<math>` elements are
                // skipped — the bespoke per-child loop this replaces descended
                // into `<math>` and leaked its glyphs as literal text. Contiguous
                // ranges (not per-node slices) preserve the sibling context the
                // footnote-id lookup needs.
                let mut range_start = 0;
                for idx in 0..=li.children.len() {
                    if idx == li.children.len() || is_sublist(&li.children[idx]) {
                        if range_start < idx {
                            collect_inlines(
                                &li.children[range_start..idx],
                                &mut para,
                                None,
                                InlineOptions::generic(fmt, Some(html_doc)),
                            );
                        }
                        range_start = idx + 1;
                    }
                }
                if !para.inlines.is_empty() {
                    sink.push(para);
                }
                for (sub_idx, li_child) in li.children.iter().enumerate() {
                    if let HtmlNode::Element(sub) = li_child
                        && is_sublist(li_child)
                    {
                        // A nested enum with its own `set` rule resets the piece
                        // indexing at this depth; otherwise the parent pattern
                        // keeps applying piece by piece.
                        let mut sub_state = state.map(|s| PatternState {
                            numbering: s.numbering.clone(),
                            full: s.full,
                            origin: s.origin,
                        });
                        if let Some(location) = enum_location_before(&li.children, sub_idx)
                            && let Some(nested) = enum_numbering_at(html_doc, location)
                            && (sub_state.as_ref().is_none_or(|s| {
                                s.numbering != nested.numbering || s.full != nested.full
                            }))
                        {
                            sub_state = Some(PatternState {
                                numbering: nested.numbering,
                                full: nested.full,
                                origin: level + 1,
                            });
                        }
                        self.walk(sub, sink, level + 1, sub_state.as_ref());
                    }
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    use typst::model::NumberingPattern;

    fn pattern(text: &str) -> Numbering {
        Numbering::Pattern(NumberingPattern::from_str(text).unwrap())
    }

    #[test]
    fn letter_pattern_maps_to_lower_letter() {
        let def = numbering_level_def(&pattern("a)"), 0, false).unwrap();
        assert_eq!(def.num_fmt, "lowerLetter");
        assert_eq!(def.lvl_text, "%1)");
        // Deeper levels cycle the last piece.
        let def = numbering_level_def(&pattern("a)"), 2, false).unwrap();
        assert_eq!(def.num_fmt, "lowerLetter");
        assert_eq!(def.lvl_text, "%3)");
    }

    #[test]
    fn roman_pattern_maps_to_lower_roman() {
        let def = numbering_level_def(&pattern("i."), 0, false).unwrap();
        assert_eq!(def.num_fmt, "lowerRoman");
        assert_eq!(def.lvl_text, "%1.");
    }

    #[test]
    fn multi_piece_pattern_indexes_by_level() {
        // "1.1": outer "1", inner "1".
        let def = numbering_level_def(&pattern("1.1"), 0, false).unwrap();
        assert_eq!((def.num_fmt, def.lvl_text.as_str()), ("decimal", "%1"));
        let def = numbering_level_def(&pattern("1.1"), 1, false).unwrap();
        assert_eq!((def.num_fmt, def.lvl_text.as_str()), ("decimal", "%2"));
        // "1.a)": outer decimal "1)", inner lower-letter "a)".
        let def = numbering_level_def(&pattern("1.a)"), 0, false).unwrap();
        assert_eq!((def.num_fmt, def.lvl_text.as_str()), ("decimal", "%1)"));
        let def = numbering_level_def(&pattern("1.a)"), 1, false).unwrap();
        assert_eq!((def.num_fmt, def.lvl_text.as_str()), ("lowerLetter", "%2)"));
    }

    #[test]
    fn full_numbering_includes_parent_numbers() {
        let def = numbering_level_def(&pattern("1.a)"), 1, true).unwrap();
        assert_eq!(def.lvl_text, "%1.%2)");
    }

    #[test]
    fn parenthesised_roman_keeps_its_prefix_and_suffix() {
        let def = numbering_level_def(&pattern("(I)"), 0, false).unwrap();
        assert_eq!(def.num_fmt, "upperRoman");
        assert_eq!(def.lvl_text, "(%1)");
    }

    #[test]
    fn unmappable_system_falls_back_to_none() {
        // Greek has no OOXML numFmt equivalent.
        assert_eq!(numbering_level_def(&pattern("α."), 0, false), None);
    }

    #[test]
    fn default_pattern_needs_no_override() {
        let def = numbering_level_def(&pattern("1."), 0, false).unwrap();
        assert!(is_default_level_def(&def, 0));
        let def = numbering_level_def(&pattern("1."), 3, false).unwrap();
        assert!(is_default_level_def(&def, 3));
        let def = numbering_level_def(&pattern("a)"), 0, false).unwrap();
        assert!(!is_default_level_def(&def, 0));
    }
}
