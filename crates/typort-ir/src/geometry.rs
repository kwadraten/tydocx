use typort_ooxml::document::{HeaderFooter, PageNumberFormat};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GeometryInput {
    /// (width, height) in twips.
    pub page_size: (u32, u32),
    pub page_numbering: Option<PageNumberFormat>,
    /// Supplement text (for example `"page"`), when known.
    pub supplement: Option<String>,
    pub header: Option<HeaderFooter>,
    pub footer: Option<HeaderFooter>,
    pub page_number_footer: bool,
}
