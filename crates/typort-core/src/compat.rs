//! Compat modes (C1–C3) and degradation reporting.
//!
//! Pure types and functions only: every input is an explicit argument, the
//! same inputs always produce the same output, and nothing here touches the
//! filesystem, the world, or any global state. Resolving a degradation's span
//! to a file (which needs the world) is the caller's job, at the boundary;
//! these functions only build, deduplicate, and format what they are given.
//!
//! The design lives in `docs/compat-modes.md` (§1 modes, §4.1 warning
//! channel and dedup, §4.2 skip semantics, §7 option shape). Phase 1 adds
//! the mode gates: C1 skips aspects the intersection cannot express and
//! reports each skip as a degradation; C2/C3 keep the current output.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use typst_syntax::Span;

use typort_ooxml::document::Document;

/// One of the three compat modes (`docs/compat-modes.md` §1).
///
/// The modes are capabilities in strict inclusion, not three implementations:
/// C1 emits the intersection, C2 adds graphical tricks, C3 adds field codes
/// on top of C2. The default is C2.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompatMode {
    /// Minimal compatibility: only the Typst↔OOXML intersection, the rest
    /// warned and skipped per aspect.
    C1,
    /// Graphical compatibility (default): tables, shapes, and other tricks
    /// for visual consistency, semantics first.
    #[default]
    C2,
    /// Dynamic compatibility: field codes for dynamic content, on top of C2.
    C3,
}

impl CompatMode {
    /// Parse a CLI short name (`c1` / `c2` / `c3`); nothing else is
    /// accepted — the long names in the design doc are prose, not CLI input.
    #[must_use]
    pub fn from_short_name(name: &str) -> Option<Self> {
        match name {
            "c1" => Some(Self::C1),
            "c2" => Some(Self::C2),
            "c3" => Some(Self::C3),
            _ => None,
        }
    }

    /// The CLI short name for this mode.
    #[must_use]
    pub fn short_name(self) -> &'static str {
        match self {
            Self::C1 => "c1",
            Self::C2 => "c2",
            Self::C3 => "c3",
        }
    }
}

/// Options for one conversion run. New options go here instead of growing
/// the `convert` / `export` signatures.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ConvertOptions {
    /// Which compat mode to convert in. Default is C2.
    pub compat: CompatMode,
}

impl ConvertOptions {
    /// Options selecting `compat`.
    #[must_use]
    pub fn new(compat: CompatMode) -> Self {
        Self { compat }
    }
}

/// What happened to the dropped aspect of a construct (§4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DegradationAction {
    /// The aspect was dropped; the rest of the element survives.
    Skipped,
    /// The aspect was kept approximately (e.g. a vector rasterized to PNG).
    Approximated,
    /// The value was replaced by a literal (e.g. a numbering pattern fell
    /// back to decimal).
    Literalized,
}

/// One dropped aspect, reported instead of silently lost (§4.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Degradation {
    /// Where the construct sits in the source, when known. Span-less
    /// degradations (e.g. found by the recovery layer) carry a page hint in
    /// the message instead; a span is never fabricated.
    pub span: Option<Span>,
    /// What was degraded, e.g. `"floating-graphic"`. Stable identifiers, not
    /// prose: the CLI prints them verbatim and the dedup keys on them.
    pub construct: &'static str,
    /// How the aspect was degraded.
    pub action: DegradationAction,
    /// Human-readable detail (what was lost, what survives instead).
    pub message: String,
}

impl Degradation {
    /// One degradation report.
    #[must_use]
    pub fn new(
        span: Option<Span>,
        construct: &'static str,
        action: DegradationAction,
        message: impl Into<String>,
    ) -> Self {
        Self {
            span,
            construct,
            action,
            message: message.into(),
        }
    }
}

/// The result of one conversion run: the document plus everything that was
/// degraded while building it. C1 mode gates append here; C2/C3 conversions
/// leave the list empty.
#[derive(Debug, Clone, PartialEq)]
pub struct ConvertOutcome {
    /// The converted document.
    pub document: Document,
    /// Degradations reported during conversion, in first-occurrence order.
    pub degradations: Vec<Degradation>,
}

impl ConvertOutcome {
    /// Pair a converted document with its degradation report.
    #[must_use]
    pub fn new(document: Document, degradations: Vec<Degradation>) -> Self {
        Self {
            document,
            degradations,
        }
    }
}

/// Deduplicate a degradation list per §4.1: the same (construct, span) is
/// reported once; span-less warnings of the same kind — same construct and
/// action — are reported on their first occurrence with a count of the rest
/// appended to the message (`（及同类 N 项）`, N = further occurrences).
/// First-occurrence order is preserved.
#[must_use]
pub fn deduplicate_degradations(degradations: Vec<Degradation>) -> Vec<Degradation> {
    let mut seen_spanned: HashSet<(&'static str, Span)> = HashSet::new();
    let mut spanless_first: HashMap<(&'static str, DegradationAction), usize> = HashMap::new();
    let mut spanless_counts: HashMap<(&'static str, DegradationAction), usize> = HashMap::new();
    let mut out: Vec<Degradation> = Vec::new();
    for degradation in degradations {
        if let Some(span) = degradation.span {
            if seen_spanned.insert((degradation.construct, span)) {
                out.push(degradation);
            }
        } else {
            let key = (degradation.construct, degradation.action);
            *spanless_counts.entry(key).or_insert(0) += 1;
            if let std::collections::hash_map::Entry::Vacant(entry) = spanless_first.entry(key) {
                entry.insert(out.len());
                out.push(degradation);
            }
        }
    }
    for (key, &index) in &spanless_first {
        if let Some(&total) = spanless_counts.get(key) {
            let extra = total - 1;
            if extra > 0 {
                let _ = write!(out[index].message, "（及同类 {extra} 项）");
            }
        }
    }
    out
}

/// Render one degradation as a CLI warning line. `location` is
/// `file:line:col` when the span resolved (resolved by the caller at the
/// boundary); `None` when it did not — the warning itself is never lost.
#[must_use]
pub fn format_degradation(degradation: &Degradation, location: Option<&str>) -> String {
    match location {
        Some(location) => format!(
            "warning: {location}: {}: {}",
            degradation.construct, degradation.message
        ),
        None => format!(
            "warning: {}: {}",
            degradation.construct, degradation.message
        ),
    }
}

/// Total number of degradation warnings printed for one conversion (§4.1).
/// Dedup runs first, so this only bites when many distinct constructs
/// degrade.
pub const MAX_PRINTED_WARNINGS: usize = 50;

/// The short name of a degradation action, as written to the JSON sidecar
/// report (`--degradation-report`).
#[must_use]
pub fn action_name(action: DegradationAction) -> &'static str {
    match action {
        DegradationAction::Skipped => "skipped",
        DegradationAction::Approximated => "approximated",
        DegradationAction::Literalized => "literalized",
    }
}

/// One degradation with its span resolved to a `file:line:col` string (or
/// `None` when the span points nowhere), ready for the JSON sidecar report.
/// The span resolution needs the world, so it happens at the CLI boundary;
/// everything here stays pure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DegradationReportEntry {
    /// `file:line:col` when the span resolved, else `None`.
    pub location: Option<String>,
    /// Stable construct identifier, as in [`Degradation::construct`].
    pub construct: &'static str,
    /// How the aspect was degraded.
    pub action: DegradationAction,
    /// Human-readable detail, as in [`Degradation::message`].
    pub message: String,
}

impl DegradationReportEntry {
    /// Pair a degradation with its already-resolved location.
    #[must_use]
    pub fn from_resolved(location: Option<String>, degradation: &Degradation) -> Self {
        Self {
            location,
            construct: degradation.construct,
            action: degradation.action,
            message: degradation.message.clone(),
        }
    }
}

/// Escape a string for inclusion in a JSON string literal: quotes,
/// backslashes, and control characters are escaped; everything else
/// (including non-ASCII text) passes through unchanged.
fn escape_json_string(value: &str, out: &mut String) {
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{c:04x}", c = c as u32);
            }
            c => out.push(c),
        }
    }
}

/// Render the deduped degradation list as the JSON sidecar report
/// (`--degradation-report`, §4.1): `{"degradations": [...]}` with one
/// object per warning. The sidecar carries the full deduped list (the
/// stderr print cap in [`cap_warning_lines`] is a display concern only).
#[must_use]
pub fn degradation_report_json(entries: &[DegradationReportEntry]) -> String {
    let mut out = String::from("{\"degradations\": [");
    for (i, entry) in entries.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str("{\"location\": ");
        match &entry.location {
            Some(location) => {
                out.push('"');
                escape_json_string(location, &mut out);
                out.push('"');
            }
            None => out.push_str("null"),
        }
        out.push_str(", \"construct\": \"");
        escape_json_string(entry.construct, &mut out);
        out.push_str("\", \"action\": \"");
        out.push_str(action_name(entry.action));
        out.push_str("\", \"message\": \"");
        escape_json_string(&entry.message, &mut out);
        out.push_str("\"}");
    }
    out.push_str("]}");
    out
}

/// Split already-formatted warning lines into the lines to print and an
/// optional summary line, enforcing the §4.1 print cap.
#[must_use]
pub fn cap_warning_lines(lines: &[String]) -> (&[String], Option<String>) {
    if lines.len() <= MAX_PRINTED_WARNINGS {
        (lines, None)
    } else {
        let hidden = lines.len() - MAX_PRINTED_WARNINGS;
        (
            &lines[..MAX_PRINTED_WARNINGS],
            Some(format!(
                "... and {hidden} more degradation warnings not shown"
            )),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use typst::syntax::{LinkedNode, RootedPath, Side, Source, VirtualPath, VirtualRoot};

    fn test_source(text: &str) -> Source {
        let vpath = VirtualPath::new("/compat-test.typ").unwrap();
        let id = RootedPath::new(VirtualRoot::Project, vpath).intern();
        Source::new(id, text.to_string())
    }

    /// The span of the leaf at `offset` in `source`.
    fn leaf_span(source: &Source, offset: usize) -> Span {
        LinkedNode::new(source.root())
            .leaf_at(offset, Side::After)
            .unwrap()
            .span()
    }

    fn degradation(
        span: Option<Span>,
        construct: &'static str,
        action: DegradationAction,
        message: &str,
    ) -> Degradation {
        Degradation::new(span, construct, action, message)
    }

    #[test]
    fn compat_mode_short_names_round_trip() {
        for (name, mode) in [
            ("c1", CompatMode::C1),
            ("c2", CompatMode::C2),
            ("c3", CompatMode::C3),
        ] {
            assert_eq!(CompatMode::from_short_name(name), Some(mode));
            assert_eq!(mode.short_name(), name);
        }
    }

    #[test]
    fn compat_mode_rejects_other_names() {
        for name in ["minimal", "graphical", "dynamic", "C1", "c4", ""] {
            assert_eq!(
                CompatMode::from_short_name(name),
                None,
                "{name:?} must not parse as a compat mode"
            );
        }
    }

    #[test]
    fn default_mode_is_c2() {
        assert_eq!(CompatMode::default(), CompatMode::C2);
        assert_eq!(ConvertOptions::default().compat, CompatMode::C2);
        assert_eq!(ConvertOptions::new(CompatMode::C1).compat, CompatMode::C1);
    }

    #[test]
    fn dedup_collapses_same_construct_and_span() {
        let source = test_source("= Title\n\nBody text\n");
        let span = leaf_span(&source, 10);
        let list = vec![
            degradation(
                Some(span),
                "floating-graphic",
                DegradationAction::Skipped,
                "first",
            ),
            degradation(
                Some(span),
                "floating-graphic",
                DegradationAction::Skipped,
                "second",
            ),
        ];
        let deduped = deduplicate_degradations(list);
        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].message, "first");
    }

    #[test]
    fn dedup_keeps_different_spans_and_constructs() {
        let source = test_source("= Title\n\nBody text\n");
        let first = leaf_span(&source, 2);
        let second = leaf_span(&source, 10);
        assert_ne!(first, second, "test spans must differ");
        let list = vec![
            degradation(
                Some(first),
                "floating-graphic",
                DegradationAction::Skipped,
                "one",
            ),
            degradation(
                Some(second),
                "floating-graphic",
                DegradationAction::Skipped,
                "two",
            ),
            degradation(
                Some(first),
                "grid-layout",
                DegradationAction::Skipped,
                "three",
            ),
        ];
        let deduped = deduplicate_degradations(list);
        assert_eq!(deduped.len(), 3);
    }

    #[test]
    fn dedup_spanless_reports_first_with_count() {
        let list = vec![
            degradation(
                None,
                "floating-graphic",
                DegradationAction::Skipped,
                "lost position",
            ),
            degradation(
                None,
                "floating-graphic",
                DegradationAction::Skipped,
                "lost position",
            ),
            degradation(
                None,
                "floating-graphic",
                DegradationAction::Skipped,
                "lost position",
            ),
        ];
        let deduped = deduplicate_degradations(list);
        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].message, "lost position（及同类 2 项）");
    }

    #[test]
    fn dedup_spanless_single_has_no_count_suffix() {
        let list = vec![degradation(
            None,
            "floating-graphic",
            DegradationAction::Skipped,
            "lost position",
        )];
        let deduped = deduplicate_degradations(list);
        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].message, "lost position");
    }

    #[test]
    fn dedup_spanless_splits_on_construct_and_action() {
        let list = vec![
            degradation(None, "floating-graphic", DegradationAction::Skipped, "a"),
            degradation(
                None,
                "floating-graphic",
                DegradationAction::Approximated,
                "b",
            ),
            degradation(None, "grid-layout", DegradationAction::Skipped, "c"),
        ];
        let deduped = deduplicate_degradations(list);
        assert_eq!(deduped.len(), 3);
    }

    #[test]
    fn dedup_preserves_first_occurrence_order() {
        let source = test_source("= Title\n\nBody text\n");
        let span = leaf_span(&source, 10);
        let list = vec![
            degradation(None, "grid-layout", DegradationAction::Skipped, "g"),
            degradation(
                Some(span),
                "floating-graphic",
                DegradationAction::Skipped,
                "f",
            ),
            degradation(None, "grid-layout", DegradationAction::Skipped, "g again"),
        ];
        let deduped = deduplicate_degradations(list);
        assert_eq!(deduped.len(), 2);
        assert_eq!(deduped[0].construct, "grid-layout");
        assert_eq!(deduped[1].construct, "floating-graphic");
    }

    #[test]
    fn format_degradation_with_and_without_location() {
        let item = degradation(
            None,
            "floating-graphic",
            DegradationAction::Skipped,
            "lost position",
        );
        assert_eq!(
            format_degradation(&item, Some("doc.typ:3:1")),
            "warning: doc.typ:3:1: floating-graphic: lost position"
        );
        assert_eq!(
            format_degradation(&item, None),
            "warning: floating-graphic: lost position"
        );
    }

    #[test]
    fn cap_warning_lines_passes_through_under_cap() {
        let lines: Vec<String> = (0..MAX_PRINTED_WARNINGS)
            .map(|i| format!("warning: {i}"))
            .collect();
        let (printed, summary) = cap_warning_lines(&lines);
        assert_eq!(printed.len(), MAX_PRINTED_WARNINGS);
        assert_eq!(summary, None);
    }

    #[test]
    fn cap_warning_lines_summarizes_over_cap() {
        let lines: Vec<String> = (0..MAX_PRINTED_WARNINGS + 3)
            .map(|i| format!("warning: {i}"))
            .collect();
        let (printed, summary) = cap_warning_lines(&lines);
        assert_eq!(printed.len(), MAX_PRINTED_WARNINGS);
        assert_eq!(
            summary,
            Some("... and 3 more degradation warnings not shown".to_string())
        );
    }

    #[test]
    fn action_names_are_stable_identifiers() {
        assert_eq!(action_name(DegradationAction::Skipped), "skipped");
        assert_eq!(action_name(DegradationAction::Approximated), "approximated");
        assert_eq!(action_name(DegradationAction::Literalized), "literalized");
    }

    #[test]
    fn degradation_report_json_empty() {
        assert_eq!(degradation_report_json(&[]), "{\"degradations\": []}");
    }

    #[test]
    fn degradation_report_json_with_and_without_location() {
        let entries = [
            DegradationReportEntry {
                location: Some("doc.typ:3:1".to_string()),
                construct: "overline",
                action: DegradationAction::Skipped,
                message: "dropped; text kept".to_string(),
            },
            DegradationReportEntry {
                location: None,
                construct: "grid-layout",
                action: DegradationAction::Skipped,
                message: "page 2 grid".to_string(),
            },
        ];
        assert_eq!(
            degradation_report_json(&entries),
            "{\"degradations\": [{\"location\": \"doc.typ:3:1\", \"construct\": \"overline\", \"action\": \"skipped\", \"message\": \"dropped; text kept\"}, {\"location\": null, \"construct\": \"grid-layout\", \"action\": \"skipped\", \"message\": \"page 2 grid\"}]}"
        );
    }

    #[test]
    fn degradation_report_json_escapes_special_characters() {
        let entries = [DegradationReportEntry {
            location: None,
            construct: "shape",
            action: DegradationAction::Approximated,
            message: "a \"quoted\" \\ path\nand a tab\tthen \u{1} control".to_string(),
        }];
        assert_eq!(
            degradation_report_json(&entries),
            "{\"degradations\": [{\"location\": null, \"construct\": \"shape\", \"action\": \"approximated\", \"message\": \"a \\\"quoted\\\" \\\\ path\\nand a tab\\tthen \\u0001 control\"}]}"
        );
    }
}
