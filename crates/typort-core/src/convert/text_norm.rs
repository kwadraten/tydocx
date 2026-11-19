//! Text normalisation shared by the HTML walk.

use typort_ooxml::document::{InlineElement, Paragraph};

use super::page;

pub(super) fn strip_cjk_spaces(para: &mut Paragraph) {
    let mut remove_indices = Vec::new();
    for i in 1..para.inlines.len().saturating_sub(1) {
        let InlineElement::Text(run) = &para.inlines[i] else {
            continue;
        };
        if run.text.trim() != "" {
            continue;
        }
        let prev = &para.inlines[i - 1];
        let next = &para.inlines[i + 1];
        let prev_ends_cjk = matches!(prev, InlineElement::Text(r)
            if r.text.chars().last().is_some_and(page::is_cjk_char));
        let next_starts_cjk = matches!(next, InlineElement::Text(r)
            if r.text.chars().next().is_some_and(page::is_cjk_char));
        let prev_is_math = matches!(prev, InlineElement::Math { .. });
        let next_is_math = matches!(next, InlineElement::Math { .. });
        // A space adjacent to CJK on one side carries no meaning when the other
        // side is CJK text or an inline equation — Chinese needs no separator from
        // a neighbouring character or formula. (A space between Latin text and an
        // equation IS kept: Typst trims the source space and Word needs it back,
        // e.g. "the value x is".)
        if (prev_ends_cjk && (next_starts_cjk || next_is_math)) || (prev_is_math && next_starts_cjk)
        {
            remove_indices.push(i);
        }
    }
    for idx in remove_indices.into_iter().rev() {
        para.inlines.remove(idx);
    }
}
