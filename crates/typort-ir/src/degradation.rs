//! Degradation reporting for the C1 compatibility mode.
//!
//! A `Degradation` records one construct the converter could not carry over
//! faithfully: what it was, what happened to it, and where in the source it
//! came from. The lowering collects them; the CLI prints them after a
//! successful write.
//!
//! This module is a pure re-export of the contract `typort-core`'s
//! `compat` module already implements; it lives here so the IR crate owns
//! the vocabulary its lowering produces. The two implementations are kept
//! identical by construction (same field order, same dedup rule) and the
//! `typort-core` side will migrate to this one when the legacy path is
//! deleted (see `docs/fallback-registry.md`).

use typst_syntax::Span;

/// What happened to a construct that could not be carried over faithfully.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DegradationAction {
    /// The construct was dropped entirely (text is never dropped this way).
    Skipped,
    /// The construct was replaced by a close approximation.
    Approximated,
    /// The construct was written as literal text instead of its semantics.
    Literalized,
}

/// One degradation record.
///
/// `construct` is a stable identifier (for example `"floating-graphic"`),
/// never prose, so the CLI can deduplicate and tests can assert on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Degradation {
    /// Source span of the construct, when known. Span-less degradations
    /// carry a page hint in the message instead of a fabricated span.
    pub span: Option<Span>,
    /// Stable construct identifier.
    pub construct: &'static str,
    /// What happened to it.
    pub action: DegradationAction,
    /// Human-readable explanation.
    pub message: String,
}

/// Maximum number of degradation warnings printed by the CLI.
pub const MAX_PRINTED_WARNINGS: usize = 50;

/// Deduplicate degradations, keeping first-occurrence order.
///
/// Same `(construct, span)` is reported once. Span-less degradations with
/// the same `(construct, action)` are reported on their first occurrence
/// with `（及同类 N 项）` appended to the message, where N counts the
/// further occurrences.
#[must_use]
pub fn deduplicate_degradations(degradations: Vec<Degradation>) -> Vec<Degradation> {
    let mut out: Vec<Degradation> = Vec::new();
    let mut counts: Vec<(Degradation, usize)> = Vec::new();
    for d in degradations {
        if let Some(entry) = counts
            .iter_mut()
            .find(|(e, _)| e.construct == d.construct && e.span == d.span && e.action == d.action)
        {
            entry.1 += 1;
        } else {
            counts.push((d, 1));
        }
    }
    for (mut d, total) in counts {
        if d.span.is_none() && total > 1 {
            d.message = format!("{}（及同类 {} 项）", d.message, total - 1);
        }
        out.push(d);
    }
    out
}

/// Format one degradation for the CLI's stderr output.
#[must_use]
pub fn format_degradation(d: &Degradation, location: Option<(usize, usize)>) -> String {
    match location {
        Some((line, col)) => format!("warning: [{line}:{col}] {}: {}", d.construct, d.message),
        None => format!("warning: {}: {}", d.construct, d.message),
    }
}

/// Cap the number of warnings to print.
#[must_use]
pub fn cap_warnings(degradations: &[Degradation]) -> &[Degradation] {
    let end = degradations.len().min(MAX_PRINTED_WARNINGS);
    &degradations[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deg(construct: &'static str, span: Option<Span>, action: DegradationAction) -> Degradation {
        Degradation {
            span,
            construct,
            action,
            message: "m".to_owned(),
        }
    }

    #[test]
    fn dedup_spanned_keeps_first() {
        let span = Span::detached();
        let out = deduplicate_degradations(vec![
            deg("a", Some(span), DegradationAction::Skipped),
            deg("a", Some(span), DegradationAction::Skipped),
        ]);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn dedup_spanless_counts_extras() {
        let out = deduplicate_degradations(vec![
            deg("a", None, DegradationAction::Skipped),
            deg("a", None, DegradationAction::Skipped),
            deg("a", None, DegradationAction::Skipped),
        ]);
        assert_eq!(out.len(), 1);
        assert!(out[0].message.contains("及同类 2 项"));
    }

    #[test]
    fn dedup_keeps_first_occurrence_order() {
        let out = deduplicate_degradations(vec![
            deg("b", None, DegradationAction::Skipped),
            deg("a", None, DegradationAction::Approximated),
            deg("b", None, DegradationAction::Skipped),
        ]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].construct, "b");
        assert_eq!(out[1].construct, "a");
    }

    #[test]
    fn format_with_and_without_location() {
        let d = deg("a", None, DegradationAction::Skipped);
        assert_eq!(
            format_degradation(&d, Some((3, 10))),
            "warning: [3:10] a: m"
        );
        assert_eq!(format_degradation(&d, None), "warning: a: m");
    }

    #[test]
    fn cap_warnings_truncates() {
        let all: Vec<Degradation> = (0..60)
            .map(|_| deg("a", None, DegradationAction::Skipped))
            .collect();
        assert_eq!(cap_warnings(&all).len(), MAX_PRINTED_WARNINGS);
    }
}
