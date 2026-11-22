//! The single body-size detection rule shared by the document-style and
//! run-style detectors (#27; the third consumer, `recovery/lines.rs`, was
//! deleted in step 4).
//!
//! Both detectors sample the first [`BODY_SIZE_SAMPLE_PAGES`] pages,
//! weight sizes by glyph count (not byte length — CJK text would otherwise
//! count triple), bucket sizes in half-points, and pick the dominant bucket
//! (ties prefer the smaller size, via [`super::stats::dominant_key`]).
//!
//! Before this module, each detector had its own sampling, weighting, and
//! bucketing — first 3 pages / glyph count / half-points in `style.rs`,
//! all pages / byte length / half-points in `run_style.rs`, first page /
//! byte length / tenths of a point in the deleted `recovery/lines.rs` — so
//! one document could yield three different body sizes, and the run-style
//! baseline could disagree with the document style it is supposed to match.

use std::collections::HashMap;

use super::stats::dominant_key;
use super::units::pt_to_half_pt;

/// Pages sampled by every body-size detector.
pub(in crate::convert) const BODY_SIZE_SAMPLE_PAGES: usize = 3;

/// Half-point fallback when no rendered body size is available.
pub(in crate::convert) const DEFAULT_BODY_SIZE_HALF_PT: u32 = 21;

/// Dominant body size in half-points from rendered text samples, each a
/// (size in points, glyph count) pair. Returns `None` when there are no samples.
pub(in crate::convert) fn dominant_body_size_half_pt(
    samples: impl IntoIterator<Item = (f64, usize)>,
) -> Option<u32> {
    let mut counts: HashMap<u32, usize> = HashMap::new();
    for (size_pt, glyphs) in samples {
        *counts.entry(pt_to_half_pt(size_pt)).or_default() += glyphs;
    }
    dominant_key(counts.iter().map(|(size, count)| (size, *count))).copied()
}

#[cfg(test)]
mod tests {
    use super::dominant_body_size_half_pt;

    #[test]
    fn glyph_weighting_not_byte_weighting() {
        // 100 CJK glyphs at 12pt weigh 300 bytes but 100 glyphs; 150 ASCII
        // glyphs at 10.5pt weigh 150 bytes but 150 glyphs. Byte weighting
        // would pick 12pt (300 > 150); glyph weighting picks 10.5pt
        // (150 > 100) — the body size, not the CJK-inflated one.
        let samples = [(12.0, 100), (10.5, 150)];
        assert_eq!(dominant_body_size_half_pt(samples), Some(21));
    }

    #[test]
    fn half_point_bucketing_merges_nearby_sizes() {
        // 10.4pt and 10.6pt both round to 21 half-points, so their glyphs
        // pool into one bucket that beats the 12pt bucket.
        let samples = [(10.4, 60), (10.6, 60), (12.0, 100)];
        assert_eq!(dominant_body_size_half_pt(samples), Some(21));
    }

    #[test]
    fn tie_prefers_smaller_size() {
        // Equal glyph counts: the body text is the baseline, and emphasis /
        // headings are larger, so the smaller size wins the tie.
        let samples = [(10.5, 100), (12.0, 100)];
        assert_eq!(dominant_body_size_half_pt(samples), Some(21));
    }

    #[test]
    fn empty_samples_give_none() {
        let samples: [(f64, usize); 0] = [];
        assert_eq!(dominant_body_size_half_pt(samples), None);
    }
}
