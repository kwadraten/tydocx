//! Owned styled semantic IR for tydocx.
//!
//! Footnotes are matched by document ordinal (marker order = entry order):
//! a `Location` is meaningful only inside one process, so no location is
//! persisted or compared across the collect / geometry boundary.

#![warn(clippy::pedantic)]

mod cell_rect;
pub mod degradation;
pub mod geometry;
pub mod grid_plan;
pub mod lower;
pub mod mode;
pub mod types;
pub mod units;

pub use degradation::{
    Degradation, DegradationAction, MAX_PRINTED_WARNINGS, cap_warnings, deduplicate_degradations,
    format_degradation,
};
pub use geometry::GeometryInput;
pub use lower::{LowerCtx, lower};
pub use mode::CompatMode;
pub use types::*;
pub use units::{pt_to_emu, pt_to_twips};
