//! Degradation reporting for the C1 compatibility mode.
//!
//! This module duplicates the contract `typort-core`'s `compat` module
//! already implements; the two are kept identical by construction, and the
//! `typort-core` side migrates to this one when the legacy path is deleted.

use typst_syntax::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DegradationAction {
    Skipped,
    Approximated,
    Literalized,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Degradation {
    pub span: Option<Span>,
    pub construct: &'static str,
    pub action: DegradationAction,
    pub message: String,
}

pub const MAX_PRINTED_WARNINGS: usize = 50;

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

#[must_use]
pub fn format_degradation(d: &Degradation, location: Option<(usize, usize)>) -> String {
    match location {
        Some((line, col)) => format!("warning: [{line}:{col}] {}: {}", d.construct, d.message),
        None => format!("warning: {}: {}", d.construct, d.message),
    }
}

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
