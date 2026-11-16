//! Recovery of layout-only content omitted by Typst's HTML export.

mod deduplication;
mod horizontal_rules;
mod insertion;
mod lines;
mod shapes;
mod table_rules;

pub(super) use horizontal_rules::{build_element_page_map, insert_horizontal_rules_from_paged};
pub(super) use table_rules::detect_three_line_tables;

use std::collections::HashSet;

use deduplication::DeduplicationCorpus;
use insertion::insert_missing_at_position;
use lines::{count_title_lines, extract_lines_from_all_pages};
use shapes::detect_dropped_shapes;
use typort_ooxml::document::Document;
use typst_layout::PagedDocument;

use crate::compat::{CompatMode, Degradation};

/// Minimum rendered line length eligible for recovery.
const MIN_RECOVERED_LINE_CHARS: usize = 2;

pub(super) fn recover_missing_content(
    paged: &PagedDocument,
    doc: &mut Document,
    compat: CompatMode,
    degradations: &mut Vec<Degradation>,
) {
    let margins = super::page::MarginsPt::from_settings(&doc.page_settings);
    let all_page_lines = extract_lines_from_all_pages(paged, margins);
    if all_page_lines.is_empty() {
        return;
    }

    let title_line_count = count_title_lines(&all_page_lines, doc);
    let corpus = DeduplicationCorpus::from_document(doc);

    let mut missing = Vec::new();
    for (i, line) in all_page_lines.iter().enumerate() {
        if i < title_line_count {
            continue;
        }
        if line.text.chars().count() < MIN_RECOVERED_LINE_CHARS {
            continue;
        }
        if line.all_math_font {
            continue;
        }
        if corpus.contains(line, doc) {
            continue;
        }
        missing.push(line.clone());
    }

    if !missing.is_empty() {
        insert_missing_at_position(doc, &missing, &all_page_lines);
    }

    // In C1, each recovered line that was inside a dropped construct reports
    // the dropped aspect (the text itself survives as flow text). Lines
    // from the same element share its span, so the §4.1 dedup collapses
    // them to one warning.
    if compat == CompatMode::C1 {
        for line in &missing {
            if let Some(attr) = &line.attribution {
                degradations.push(Degradation::new(
                    attr.span,
                    attr.construct,
                    attr.action,
                    attr.message.to_string(),
                ));
            }
        }
        // M5: pure shapes (no text) are invisible to the line path; scan
        // for them directly. Shapes whose enclosing element already
        // reported its text as dropped are skipped, so a rect with
        // recovered text is reported once, as shape-text.
        let reported_spans: HashSet<typst_syntax::Span> = missing
            .iter()
            .filter_map(|line| line.attribution.as_ref())
            .filter(|attr| matches!(attr.construct, "shape-text" | "box-decoration"))
            .filter_map(|attr| attr.span)
            .collect();
        degradations.extend(detect_dropped_shapes(paged, &reported_spans));
    }
}
