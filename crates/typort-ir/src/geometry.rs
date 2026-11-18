use typort_ooxml::document::{HeaderFooter, PageNumberFooters, PageNumberFormat};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GeometryInput {
    /// (width, height) in twips.
    pub page_size: (u32, u32),
    pub page_numbering: Option<PageNumberFormat>,
    /// `w:start` restart on the final section's `w:pgNumType`.
    pub page_number_start: Option<u32>,
    /// Supplement text (for example `"page"`), when known.
    pub supplement: Option<String>,
    pub header: Option<HeaderFooter>,
    pub header_even: Option<HeaderFooter>,
    pub header_first: Option<HeaderFooter>,
    pub footer: Option<HeaderFooter>,
    pub footer_even: Option<HeaderFooter>,
    pub footer_first: Option<HeaderFooter>,
    /// Generated PAGE-field footer slots (default/even/first).
    pub page_number_footers: PageNumberFooters,
    /// `w:evenAndOddHeaders` — odd/even margin-zone content differed.
    pub even_and_odd_headers: bool,
    /// `w:titlePg` — page 1's margin-zone content differed from the rest.
    pub title_page: bool,
}
