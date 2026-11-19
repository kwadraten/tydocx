//! Small statistical helpers shared by the paged-style heuristics.

use std::cmp::Reverse;

/// Return the most frequent key, preferring the smallest key when counts tie.
pub(super) fn dominant_key<'a, K: Ord + ?Sized>(
    counts: impl IntoIterator<Item = (&'a K, usize)>,
) -> Option<&'a K> {
    counts
        .into_iter()
        .max_by_key(|(key, count)| (*count, Reverse(*key)))
        .map(|(key, _)| key)
}

/// Sort values and return the upper middle value used by the existing detectors.
pub(super) fn median(values: &mut [f64]) -> Option<f64> {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    values.get(values.len() / 2).copied()
}

/// Return the population standard deviation, or `None` for an empty sample.
pub(super) fn standard_deviation(values: &[f64]) -> Option<f64> {
    let count = u32::try_from(values.len()).ok().map(f64::from)?;
    let mean = values.iter().sum::<f64>() / count;
    let variance = values
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>()
        / count;
    Some(variance.sqrt())
}
