//! Owned styled semantic IR for typort.
//!
//! The IR is the product of one `typst-realize` pass over the document
//! (`Target::Paged`): a `Vec<(Content, StyleChain)>` collected into owned,
//! lifetime-free types, plus the companion-Paged facts the lowering needs
//! (`GeometryInput`). It is then lowered purely to a `typort_ooxml::Document`.
//!
//! The crate depends on no Typst runtime type beyond `typst-syntax`'s `Span`
//! (used for degradation locations). It never sees a `World`, an `Engine`, or
//! a `PagedDocument`: all Typst types are resolved at the collection
//! boundary, in `typort-core`.
//!
//! Design notes (from the step-2 probe, `~/workspace/realize-probe/report.md`):
//! - Strong / emphasis / superscript / subscript / highlight / underline /
//!   overline / strike / smallcaps are **run attributes, not IR variants**:
//!   the Paged show rules merge them into `TextElem` attributes and
//!   decorations before the collector ever sees them, so restoring element
//!   variants would be fabrication. `Run` (from `typort-ooxml`) carries them.
//! - `IrBlock::Table` holds a fully resolved `CellGrid` (the fragment
//!   re-realization gives per-cell fill / align / inset / stroke); the grid
//!   element itself is not `Locatable`, so it is taken from the pairs'
//!   `MultiLayouter` captures, never from an introspector query.
//! - Footnotes are matched by document ordinal (marker order = entry order):
//!   `Location` is meaningful only inside one process, so no location is
//!   persisted or compared across the collect / geometry boundary.

#![warn(clippy::pedantic)]

pub mod degradation;
pub mod geometry;
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
