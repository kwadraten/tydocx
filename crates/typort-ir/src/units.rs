//! Unit conversions shared by the IR and its lowering.
//!
//! All functions are pure.

/// Points in one twip (1/20 pt).
pub const TWIPS_PER_PT: f64 = 20.0;

/// English Metric Units in one point (914400 EMU per inch, 72 pt per inch).
pub const EMU_PER_PT: f64 = 12_700.0;

/// Convert points to twips, rounding to the nearest integer.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "page geometry is small and non-negative; clamped to zero first"
)]
pub fn pt_to_twips(pt: f64) -> u32 {
    (pt * TWIPS_PER_PT).round().max(0.0) as u32
}

/// Convert points to EMU, rounding to the nearest integer.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "page geometry is small and non-negative; clamped to zero first"
)]
pub fn pt_to_emu(pt: f64) -> u64 {
    (pt * EMU_PER_PT).round().max(0.0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pt_to_twips_rounds() {
        assert_eq!(pt_to_twips(11.0), 220);
        assert_eq!(pt_to_twips(0.5), 10);
        assert_eq!(pt_to_twips(10.5), 210);
    }

    #[test]
    fn pt_to_emu_rounds() {
        assert_eq!(pt_to_emu(1.0), 12_700);
        assert_eq!(pt_to_emu(0.5), 6_350);
    }

    #[test]
    fn negative_clamps_to_zero() {
        assert_eq!(pt_to_twips(-1.0), 0);
        assert_eq!(pt_to_emu(-1.0), 0);
    }
}
