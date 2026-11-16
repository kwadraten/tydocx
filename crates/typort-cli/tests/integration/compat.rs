//! Phase 0 plumbing: the three compat modes convert identically and report
//! no degradations (no mode gate exists yet). The default options are C2.

use crate::common::{fixture_outcome_with_options, fixture_package_with_options};
use typort_core::{CompatMode, ConvertOptions};

#[test]
fn all_compat_modes_convert_identically() {
    let modes = [
        ConvertOptions::default(),
        ConvertOptions::new(CompatMode::C1),
        ConvertOptions::new(CompatMode::C2),
        ConvertOptions::new(CompatMode::C3),
    ];
    let first = fixture_package_with_options("complex_paper", &modes[0])
        .part_text("word/document.xml")
        .to_owned();
    for options in &modes[1..] {
        let package = fixture_package_with_options("complex_paper", options);
        let xml = package.part_text("word/document.xml");
        assert_eq!(
            first, xml,
            "mode {:?} must convert identically to the default in Phase 0",
            options.compat
        );
    }
}

#[test]
fn no_mode_reports_degradations_in_phase_zero() {
    for mode in [CompatMode::C1, CompatMode::C2, CompatMode::C3] {
        let outcome = fixture_outcome_with_options("complex_paper", &ConvertOptions::new(mode));
        assert!(
            outcome.degradations.is_empty(),
            "mode {mode:?} should report no degradations in Phase 0, got {:?}",
            outcome.degradations
        );
    }
}
