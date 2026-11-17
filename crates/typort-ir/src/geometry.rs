//! Companion-Paged facts the lowering consumes, as an explicit boundary
//! struct.
//!
//! The lowering needs only page-level facts. Frame-level facts from the
//! probe's sketch (`frame_items`, `footnote_entries`, `running_headers`)
//! are resolved **into the IR by the collector** (`typort-core`): footnote
//! entry bodies become `IrFootnote`s, image display sizes become
//! `IrImage` dimensions, figure rasters attach to their `IrFigure`. The
//! lowering never sees a frame, a location, or a page number it did not
//! already receive as plain data — so this struct carries only what the
//! lowering itself consumes.

use typort_ooxml::document::{HeaderFooter, PageNumberFormat};

/// Page-level companion facts for the lowering.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GeometryInput {
    /// First-page size in twips `(width, height)`.
    pub page_size: (u32, u32),
    /// Declared page-number format (`#set page(numbering:)`), when present.
    pub page_numbering: Option<PageNumberFormat>,
    /// Page-number supplement text (for example `"page"`), when known.
    pub supplement: Option<String>,
    /// Default header, from the first page's margin zones.
    pub header: Option<HeaderFooter>,
    /// Default footer, from the first page's margin zones.
    pub footer: Option<HeaderFooter>,
    /// Whether the footer carries a PAGE field.
    pub page_number_footer: bool,
}
