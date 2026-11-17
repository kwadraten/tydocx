use std::collections::HashMap;

pub use typort_ooxml::document::{
    Alignment, AnchoredLine, CitationSource, DocumentStyle, FootnoteFormat, HangingIndent,
    PageNumberFormat, PageSettings, ParagraphStyle, Run, TableBorders, TableCellMargins,
    VerticalAlign,
};

#[derive(Debug, Clone, PartialEq)]
pub struct IrDocument {
    pub info: IrDocumentInfo,
    pub page: PageSettings,
    pub style: DocumentStyle,
    pub body: Vec<IrBlock>,
    pub footnotes: Vec<IrFootnote>,
    pub citation_sources: Vec<CitationSource>,
    /// Text declared in `#set page(foreground:)` / `background:`. Rendered on
    /// every page by Typst but dropped by Word conversion; C1 literalizes it
    /// into the body with a `page-foreground` degradation.
    pub page_furniture: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IrDocumentInfo {
    pub title: Option<String>,
    pub author: Option<String>,
    pub lang: Option<String>,
    pub region: Option<String>,
    /// `document(description:)` → `dc:description`.
    pub description: Option<String>,
    /// `document(keywords:)` → comma-joined `cp:keywords`.
    pub keywords: Vec<String>,
    /// `document(date: datetime(...))` → ISO 8601 `dcterms:created`.
    pub created: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IrFootnote {
    pub ordinal: u32,
    pub rendered: String,
    pub body: Vec<IrParagraph>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrShapeKind {
    Rectangle,
    Curve,
    /// `#line(...)` — a geometric line segment, not the `#line` rule
    /// (`line()` here is the 2-point shape; `#divider`/`#line` the rule is a
    /// paragraph border instead).
    Line,
}

impl std::fmt::Display for IrShapeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Rectangle => "rectangle",
            Self::Curve => "curve",
            Self::Line => "line",
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum IrBlock {
    Paragraph(IrParagraph),
    Heading {
        level: u8,
        /// The number Typst rendered for this heading, if any. Emitted as
        /// literal text whenever the numbering is not mapped to a `w:numPr`.
        numbering_text: Option<String>,
        /// How the heading's numbering maps to Word numbering: `Mappable`
        /// when the pattern is a set of Arabic/Latin/Roman pieces, or
        /// `Unmappable` when Typst's numbering is a function or uses a numeral
        /// system OOXML cannot express. `None` when no numbering is declared.
        numbering: Option<IrHeadingNumbering>,
        body: Vec<IrInline>,
        label: Option<String>,
        alignment: Option<Alignment>,
        span: Option<typst_syntax::Span>,
    },
    Table(IrTable),
    Grid(IrGrid),
    Figure {
        body: Vec<IrBlock>,
        caption: Vec<IrInline>,
        caption_alignment: Option<Alignment>,
        kind: IrFigureKind,
        supplement: Option<String>,
        label: Option<String>,
        caption_first: bool,
        custom_placement: bool,
        /// `figure(gap:)` override in twips — spacing between body and caption.
        gap_twips: Option<u32>,
    },
    List(IrList),
    Enum(IrEnum),
    Equation(IrEquation),
    Quote {
        body: Vec<IrBlock>,
    },
    Raw(IrRaw),
    Outline {
        title: Vec<IrInline>,
        entries: Vec<IrOutlineEntry>,
        depth: u8,
        custom_target: bool,
        custom_indent: bool,
    },
    Bibliography {
        entries: Vec<IrBibEntry>,
    },
    Place {
        float: bool,
        body: Vec<IrBlock>,
    },
    PageBreak {
        weak: bool,
        to: Option<IrSectionBreakType>,
    },
    Shape {
        kind: IrShapeKind,
        span: Option<typst_syntax::Span>,
        body: Vec<IrBlock>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrSectionBreakType {
    Odd,
    Even,
}

/// Only properties the OOXML `Paragraph` can carry are represented here;
/// document-wide facts live on `DocumentStyle`.
#[derive(Debug, Clone, Default, PartialEq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "flat bag of orthogonal paragraph flags; a state machine would not model them"
)]
pub struct IrParagraph {
    pub inlines: Vec<IrInline>,
    pub style: Option<ParagraphStyle>,
    pub alignment: Option<Alignment>,
    pub suppress_indent: bool,
    pub hanging_indent: Option<HangingIndent>,
    pub left_indent: Option<u32>,
    pub right_indent: Option<u32>,
    pub keep_lines: bool,
    pub keep_next: bool,
    pub code_block: bool,
    pub horizontal_rule: bool,
    pub tab_stops: Vec<u32>,
    pub spacing_before: Option<u32>,
    /// `w:spacing w:after` override (twips) — `block(above:|below:|spacing:)`.
    pub spacing_after: Option<u32>,
    pub bookmark: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IrInline {
    Text(Run),
    Link {
        url: Option<String>,
        anchor: Option<String>,
        body: Vec<IrInline>,
    },
    Ref {
        target_label: String,
        display_text: String,
        page_ref: bool,
    },
    Cite {
        keys: Vec<String>,
        rendered: String,
        target: Option<String>,
    },
    FootnoteMarker {
        ordinal: u32,
        rendered: String,
    },
    LineBreak,
    Space,
    Image(IrImage),
    Math {
        omml: String,
    },
    PageBreak,
    ColumnBreak,
    Tab,
    PageField {
        display_text: String,
    },
    Shape {
        kind: IrShapeKind,
        span: Option<typst_syntax::Span>,
        body: Vec<IrInline>,
    },
    /// A measured overline: a floating line shape above this position in the
    /// text, geometry taken from Typst's own rendered deco line. Inserted by
    /// the paged-frame patch when the run's `overline` flag can be matched to
    /// a rendered `OverlineElem` bracket.
    Overline(AnchoredLine),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IrImage {
    pub bytes: Vec<u8>,
    pub source_hash: u64,
    pub width_emu: u64,
    pub height_emu: u64,
    pub mime: Option<String>,
    pub span: Option<typst_syntax::Span>,
    pub vector: bool,
    pub non_cover_fit: bool,
    pub alt: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IrTable {
    pub rows: Vec<IrTableRow>,
    pub column_widths_pct: Option<Vec<u32>>,
    pub borders: Option<TableBorders>,
    pub gutter: bool,
    pub cell_margins: Option<TableCellMargins>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IrTableRow {
    pub cells: Vec<IrTableCell>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IrTableCell {
    pub body: Vec<IrBlock>,
    pub colspan: u32,
    pub rowspan: u32,
    pub vmerge: IrVMerge,
    pub fill_hex: Option<String>,
    pub vertical_align: Option<VerticalAlign>,
    pub inset_twips: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IrVMerge {
    #[default]
    None,
    Restart,
    Continue,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IrGrid {
    pub rows: Vec<IrTableRow>,
    pub column_widths_pct: Option<Vec<u32>>,
    pub borders: Option<TableBorders>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IrList {
    pub items: Vec<IrListItem>,
    pub depth: u8,
    pub custom_marker: bool,
    /// `list(indent:)` override in twips.
    pub indent_twips: Option<u32>,
    /// `list(body-indent:)` override in twips.
    pub body_indent_twips: Option<u32>,
    /// `list(spacing:)` override in twips — gap between items.
    pub spacing_twips: Option<u32>,
    /// Per-level `list(marker:)` literal strings (e.g. `"–"`), when the marker
    /// array holds only literal text; `None` = default marker resolution.
    pub marker_texts: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IrListItem {
    pub marker: Option<String>,
    pub body: Vec<IrBlock>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IrEnum {
    pub items: Vec<IrEnumItem>,
    pub numbering: IrNumberingPattern,
    pub start: u32,
    pub depth: u8,
    pub reversed: bool,
    /// `enum(indent:)` override in twips.
    pub indent_twips: Option<u32>,
    /// `enum(body-indent:)` override in twips.
    pub body_indent_twips: Option<u32>,
    /// `enum(spacing:)` override in twips.
    pub spacing_twips: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IrEnumItem {
    pub number_text: Option<String>,
    pub body: Vec<IrBlock>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IrNumberingPattern {
    pub pieces: Vec<IrNumberingPiece>,
    pub suffix: String,
    pub full: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IrNumberingPiece {
    pub prefix: String,
    pub system: IrNumeralSystem,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IrLevelDef {
    pub num_fmt: String,
    pub lvl_text: String,
}

/// How a heading's Typst numbering maps onto Word numbering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IrHeadingNumbering {
    /// A static `NumberingPattern` whose pieces all map to an OOXML
    /// `w:numFmt`. Emitted as `w:numPr` + a numbering instance.
    Mappable(IrNumberingPattern),
    /// A numbering function, or a pattern containing a numeral system OOXML
    /// cannot express (Chinese, symbols, …). Emitted as literal text + a
    /// `heading-numbering` degradation.
    Unmappable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrNumeralSystem {
    Arabic,
    LowerLatin,
    UpperLatin,
    LowerRoman,
    UpperRoman,
}

impl IrNumeralSystem {
    /// OOXML `w:numFmt` value for this system.
    #[must_use]
    pub fn num_fmt(self) -> &'static str {
        match self {
            Self::Arabic => "decimal",
            Self::LowerLatin => "lowerLetter",
            Self::UpperLatin => "upperLetter",
            Self::LowerRoman => "lowerRoman",
            Self::UpperRoman => "upperRoman",
        }
    }

    /// Render a counter value the way the Typst/OOXML numeral system does.
    #[must_use]
    pub fn render(self, number: u64) -> String {
        match self {
            Self::Arabic => number.to_string(),
            Self::LowerLatin => to_latin(number, false),
            Self::UpperLatin => to_latin(number, true),
            Self::LowerRoman => to_roman(number, false),
            Self::UpperRoman => to_roman(number, true),
        }
    }
}

fn to_latin(mut number: u64, upper: bool) -> String {
    let mut out = String::new();
    while number > 0 {
        number -= 1;
        let c = char::from_u32(u32::from(b'a') + u32::try_from(number % 26).unwrap_or(0))
            .unwrap_or('a');
        out.push(if upper { c.to_ascii_uppercase() } else { c });
        number /= 26;
    }
    out.chars().rev().collect()
}

fn to_roman(mut number: u64, upper: bool) -> String {
    const TABLE: &[(u64, &str)] = &[
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut out = String::new();
    for (value, numeral) in TABLE {
        while number >= *value {
            out.push_str(numeral);
            number -= value;
        }
    }
    if upper { out.to_ascii_uppercase() } else { out }
}

#[derive(Debug, Clone, PartialEq)]
pub struct IrEquation {
    pub omml: String,
    pub number: Option<String>,
    pub bookmark: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IrRaw {
    pub lines: Vec<Vec<Run>>,
    pub language: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IrOutlineEntry {
    pub level: u8,
    pub title: Vec<IrInline>,
    pub target: Option<String>,
    pub page_number: Option<String>,
}

/// `bookmark` is the entry's bookmark (`"bib-n"`), not the citation key:
/// keys are not recoverable from rendered entry order in the general case.
#[derive(Debug, Clone, PartialEq)]
pub struct IrBibEntry {
    pub bookmark: String,
    pub marker: Option<String>,
    pub body: Vec<IrInline>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrFigureKind {
    Image,
    Table,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IrRaster {
    pub bytes: Vec<u8>,
    pub width_emu: u64,
    pub height_emu: u64,
}

pub type ImageSizes = HashMap<u64, (u64, u64)>;
