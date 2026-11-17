//! Owned styled semantic IR types.
//!
//! Algebraic data types: the document is a tree of `IrBlock`s, each block
//! holding `IrInline`s. Every type is owned (no lifetimes) and holds only
//! plain data — all Typst types (`Content`, `StyleChain`, `CellGrid`, …)
//! are resolved at the collection boundary in `typort-core`.
//!
//! Reused from `typort-ooxml` where the shape is identical, so the IR does
//! not grow a parallel universe: `Run`, `Alignment`, `DocumentStyle`,
//! `PageSettings`, `PageNumberFormat`, `CitationSource`, `TableBorders`,
//! `VerticalAlign`, `FootnoteFormat`.

use std::collections::HashMap;

pub use typort_ooxml::document::{
    Alignment, CitationSource, DocumentStyle, FootnoteFormat, HangingIndent, PageNumberFormat,
    PageSettings, ParagraphStyle, Run, TableBorders, VerticalAlign,
};

/// The whole document, as collected from one realize pass plus the
/// companion-Paged boundary facts.
#[derive(Debug, Clone, PartialEq)]
pub struct IrDocument {
    /// Document metadata from `#set document(...)` and the first heading.
    pub info: IrDocumentInfo,
    /// Page setup (size + margins) from the first Paged page and the
    /// source-AST overrides.
    pub page: PageSettings,
    /// Document-wide style (body font/size, heading sizes, spacing, …).
    pub style: DocumentStyle,
    /// Body blocks, in document order.
    pub body: Vec<IrBlock>,
    /// Footnotes, in document (marker) order. Matched to markers by ordinal,
    /// never by `Location`.
    pub footnotes: Vec<IrFootnote>,
    /// Bibliography sources, re-parsed from the source `.bib`/`.yaml`.
    pub citation_sources: Vec<CitationSource>,
}

/// Document metadata.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IrDocumentInfo {
    pub title: Option<String>,
    pub author: Option<String>,
    /// Declared language (`#set text(lang:)`), when present.
    pub lang: Option<String>,
    /// Declared region (`#set text(region:)`), when present.
    pub region: Option<String>,
}

/// One footnote: the rendered marker number and the entry body.
#[derive(Debug, Clone, PartialEq)]
pub struct IrFootnote {
    /// The marker number as rendered by Typst (1-based ordinal).
    pub ordinal: u32,
    /// The marker text as rendered (for example `"1"` or `"①"`).
    pub rendered: String,
    /// Entry body, as paragraphs of inlines.
    pub body: Vec<IrParagraph>,
}

/// The geometry family of a pure shape, for degradation messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrShapeKind {
    /// Axis-aligned rectangles (`#rect`, `#square`, `#ellipse` bodies
    /// aside, `ellipse`/`circle` map to `Curve` since Word has no such
    /// element in the C1 intersection either).
    Rectangle,
    /// Everything else: ellipses, circles, paths, polygons, curves.
    Curve,
}

impl std::fmt::Display for IrShapeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Rectangle => "rectangle",
            Self::Curve => "curve",
        })
    }
}

/// A block-level element.
#[derive(Debug, Clone, PartialEq)]
pub enum IrBlock {
    /// A body paragraph.
    Paragraph(IrParagraph),
    /// A heading. `numbering_text` is the rendered number (for example
    /// `"1.1"`) when the heading carries numbering; the body is the title
    /// inlines only. `label` is the heading's label for cross-references.
    Heading {
        level: u8,
        numbering_text: Option<String>,
        body: Vec<IrInline>,
        label: Option<String>,
        /// Paragraph alignment from the heading element's own style
        /// chain (e.g. a `show heading` rule centering it); `None`
        /// means the default (left).
        alignment: Option<Alignment>,
    },
    /// A table, with a fully resolved cell grid (fragment re-realization).
    Table(IrTable),
    /// A `#grid`, with a resolved cell grid (from the pairs' `MultiLayouter`).
    Grid(IrGrid),
    /// A figure: body block(s), caption, kind, supplement, and the
    /// figure's label for cross-references. `caption_first` records
    /// whether the author placed the caption above the body.
    /// `caption_alignment` is the caption paragraph's alignment from its
    /// style chain (the figure caption's centered show rule).
    Figure {
        body: Vec<IrBlock>,
        caption: Vec<IrInline>,
        caption_alignment: Option<Alignment>,
        kind: IrFigureKind,
        supplement: Option<String>,
        label: Option<String>,
        caption_first: bool,
        /// The figure declared a `placement:` argument — dropped under C1.
        custom_placement: bool,
    },
    /// An unordered list.
    List(IrList),
    /// An ordered (enum) list, with its numbering pattern.
    Enum(IrEnum),
    /// A block equation, with pre-rendered OMML.
    Equation(IrEquation),
    /// A block quote. The body is the quote's paragraphs (the attribution,
    /// when present, is its own right-aligned paragraph inside the body,
    /// exactly as Typst renders it).
    Quote { body: Vec<IrBlock> },
    /// A raw (code) block: one run list per line.
    Raw(IrRaw),
    /// An outline: title inlines, entries, and the maximum depth.
    Outline {
        title: Vec<IrInline>,
        entries: Vec<IrOutlineEntry>,
        depth: u8,
        /// The outline declared a non-heading `target:` — a Word TOC can
        /// only list headings, so C1 reports the aspect as approximated.
        custom_target: bool,
        /// The outline declared a custom `indent:` — dropped under C1.
        custom_indent: bool,
    },
    /// A bibliography: rendered entries, each with its citation key.
    Bibliography { entries: Vec<IrBibEntry> },
    /// A placed element (`#place`). `float` elements are anchored
    /// (C2 trick); non-floating placement is a C1 degradation at lowering.
    Place { float: bool, body: Vec<IrBlock> },
    /// A page break. `weak` is Typst's `weak: true` (a flow boundary, not
    /// a hard break); `to` is a `#pagebreak(to:)` section target.
    PageBreak {
        weak: bool,
        to: Option<IrSectionBreakType>,
    },
    /// A pure shape (`#rect`, `#circle`, `#path`, …). The geometry has no
    /// C1 form (dropped with a degradation) and no C2/C3 form yet either;
    /// `body` carries a rect/ellipse's inner content, which always
    /// survives as flow blocks.
    Shape {
        kind: IrShapeKind,
        span: Option<typst_syntax::Span>,
        body: Vec<IrBlock>,
    },
}

/// Section-break target of `#pagebreak(to:)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrSectionBreakType {
    Odd,
    Even,
}

/// A paragraph: inlines plus paragraph-level properties.
///
/// Only properties the OOXML `Paragraph` can carry are represented here.
/// Document-wide facts (body first-line indent, body/heading spacing,
/// line spacing) live on `DocumentStyle`, exactly as the legacy path
/// expresses them — there is no per-paragraph first-line indent or
/// per-paragraph line spacing in the OOXML model this lowering targets.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IrParagraph {
    pub inlines: Vec<IrInline>,
    /// Paragraph style: `None` means normal body text.
    pub style: Option<ParagraphStyle>,
    /// Alignment of this paragraph, when the author set one.
    pub alignment: Option<Alignment>,
    /// Suppress the first-line indent for this paragraph.
    pub suppress_indent: bool,
    /// Hanging indent (bibliography entries, source-authored rules).
    pub hanging_indent: Option<HangingIndent>,
    /// Left indent in twips, when set.
    pub left_indent: Option<u32>,
    /// Apply the `CodeBlock` style (monospace, no indent).
    pub code_block: bool,
    /// This paragraph is a horizontal rule (bottom border).
    pub horizontal_rule: bool,
    /// Tab stop positions in twips.
    pub tab_stops: Vec<u32>,
    /// Override the paragraph's `w:before` spacing (twips).
    pub spacing_before: Option<u32>,
    /// Bookmark name for cross-references, when the paragraph is labelled.
    pub bookmark: Option<String>,
}

/// An inline element.
#[derive(Debug, Clone, PartialEq)]
pub enum IrInline {
    /// A run of text with its full formatting (bold/italic/underline/
    /// strike/smallcaps/super/sub/highlight/shading live on the run
    /// itself — the Paged show rules already merged them there).
    /// Overline rides `Run::overline` to the lowering, which translates
    /// it per compat mode.
    Text(Run),
    /// A hyperlink: external URL, or an internal anchor (bookmark name).
    Link {
        url: Option<String>,
        anchor: Option<String>,
        body: Vec<IrInline>,
    },
    /// A cross-reference (`#ref`): target label + rendered display text.
    /// `page_ref` marks a page-number reference (`counter(page).at`),
    /// which lowers to a PAGEREF field instead of a REF field.
    Ref {
        target_label: String,
        display_text: String,
        page_ref: bool,
    },
    /// A citation (`@key`): keys, rendered marker text, and the target
    /// bibliography label when known.
    Cite {
        keys: Vec<String>,
        rendered: String,
        target: Option<String>,
    },
    /// A footnote marker: ordinal and rendered text.
    FootnoteMarker { ordinal: u32, rendered: String },
    /// A forced line break (`<br>` / `linebreak()`).
    LineBreak,
    /// A space run (kept explicit so the lowering does not re-glue words).
    Space,
    /// An inline image.
    Image(IrImage),
    /// Inline math, with pre-rendered OMML.
    Math { omml: String },
    /// A page break inside a paragraph (from `#pagebreak()` in inline flow).
    PageBreak,
    /// A column break (`#colbreak()`).
    ColumnBreak,
    /// A tab stop.
    Tab,
    /// A Word PAGE field: the current page number. `display_text` is the
    /// number Typst rendered at this spot (the field's cached result).
    PageField { display_text: String },
    /// An inline pure shape (`#rect[...]` inside a paragraph). Like
    /// `IrBlock::Shape`: the geometry is dropped with a C1 degradation;
    /// the body inlines always survive.
    Shape {
        kind: IrShapeKind,
        span: Option<typst_syntax::Span>,
        body: Vec<IrInline>,
    },
}

/// An image, with bytes and display size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IrImage {
    /// Image bytes (PNG/JPEG/SVG source bytes, as decoded by Typst).
    pub bytes: Vec<u8>,
    /// Hash of the image's original bytes, as Typst decoded them — the
    /// key `collect_image_sizes` uses in the companion Paged frames.
    /// `bytes` can differ from the original (SVG/GIF/WebP are re-encoded
    /// to PNG), so the patch must look the size up by this hash, not by
    /// hashing `bytes`.
    pub source_hash: u64,
    /// Display width in EMU.
    pub width_emu: u64,
    /// Display height in EMU.
    pub height_emu: u64,
    /// MIME type when known (`"image/png"`, `"image/jpeg"`, `"image/svg+xml"`).
    pub mime: Option<String>,
    /// Source span of the `ImageElem`, for degradation records.
    pub span: Option<typst_syntax::Span>,
    /// The source was a vector format (SVG): the bytes were rasterized
    /// to PNG, a C1 approximation.
    pub vector: bool,
    /// `fit:` was a non-`cover` value (`contain`, `stretch`): dropped
    /// under C1.
    pub non_cover_fit: bool,
    /// The declared `alt:` text: dropped under C1.
    pub alt: Option<String>,
}

/// A table, with a fully resolved cell grid.
#[derive(Debug, Clone, PartialEq)]
pub struct IrTable {
    /// Rows in visual order. Gutter rows are already expanded into the
    /// grid by Typst's synthesis, so every row here is a real row.
    pub rows: Vec<IrTableRow>,
    /// Column widths in fiftieths of a percent of the table width
    /// (5000 = 100%), when computable (all-`auto` tracks give `None` and
    /// the writer auto-sizes). The values come straight from
    /// `track_widths_pct`, which already normalizes to 5000 — the
    /// lowering sums the spanned tracks, it does not rescale them.
    pub column_widths_pct: Option<Vec<u32>>,
    /// Table borders: `None` means the writer's uniform 0.5pt grid (a
    /// plain table's default stroke); `Some` carries per-side borders
    /// mapped from the resolved grid lines (`Some(TableBorders::default())`
    /// is all-nil — a `stroke: none` table).
    pub borders: Option<TableBorders>,
    /// The table declared `column-gutter`/`row-gutter`: spacing between
    /// tracks, dropped under C1.
    pub gutter: bool,
}

/// One table row.
#[derive(Debug, Clone, PartialEq)]
pub struct IrTableRow {
    pub cells: Vec<IrTableCell>,
}

/// One table cell.
#[derive(Debug, Clone, PartialEq)]
pub struct IrTableCell {
    /// Cell content as blocks (usually a single paragraph).
    pub body: Vec<IrBlock>,
    /// Column span.
    pub colspan: u32,
    /// Row span.
    pub rowspan: u32,
    /// Vertical merge state: first row of a merged run, a continuation,
    /// or not merged.
    pub vmerge: IrVMerge,
    /// Cell background fill, as a hex RGB string (`"DDEEFF"`), when set.
    pub fill_hex: Option<String>,
    /// Vertical alignment inside the cell.
    pub vertical_align: Option<VerticalAlign>,
    /// Cell inset in twips, when set.
    pub inset_twips: Option<u32>,
}

/// Vertical merge state of a table cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IrVMerge {
    #[default]
    None,
    Restart,
    Continue,
}

/// A `#grid`, with a resolved cell grid.
#[derive(Debug, Clone, PartialEq)]
pub struct IrGrid {
    pub rows: Vec<IrTableRow>,
    /// Column widths in fiftieths of a percent (5000 = 100%), as in
    /// `IrTable`.
    pub column_widths_pct: Option<Vec<u32>>,
    /// A grid is borderless by default (`Some(TableBorders::default())`,
    /// all-nil); a grid with strokes or explicit lines maps to per-side
    /// borders (`None` = the writer's uniform grid).
    pub borders: Option<TableBorders>,
}

/// An unordered list.
#[derive(Debug, Clone, PartialEq)]
pub struct IrList {
    pub items: Vec<IrListItem>,
    /// Nesting depth (0 = top level), for the numbering level.
    pub depth: u8,
    /// `marker:` was a non-default value: literalized to the shared
    /// bullet under C1.
    pub custom_marker: bool,
}

/// One list item: marker + body blocks (nested lists are blocks).
#[derive(Debug, Clone, PartialEq)]
pub struct IrListItem {
    /// The rendered marker text (for example `"•"`).
    pub marker: Option<String>,
    pub body: Vec<IrBlock>,
}

/// An ordered (enum) list.
#[derive(Debug, Clone, PartialEq)]
pub struct IrEnum {
    pub items: Vec<IrEnumItem>,
    /// The numbering pattern, resolved from the `EnumElem`.
    pub numbering: IrNumberingPattern,
    /// Starting number (`#enum(start: N)`; 1 when unset).
    pub start: u32,
    /// Nesting depth (0 = top level).
    pub depth: u8,
    /// `reversed: true` — forward numbering is emitted under C1.
    pub reversed: bool,
}

/// One enum item: the rendered number + body blocks.
#[derive(Debug, Clone, PartialEq)]
pub struct IrEnumItem {
    /// The rendered number text (for example `"1."` or `"a)"`).
    pub number_text: Option<String>,
    pub body: Vec<IrBlock>,
}

/// A resolved numbering pattern: the raw pieces of a Typst
/// `Numbering::Pattern`, as the collector read them from the `EnumElem`.
/// The lowering derives one level definition per nesting depth from
/// these pieces, mirroring the legacy walk (deeper levels cycle the
/// last piece; see `numbering_level_def` in `lower`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IrNumberingPattern {
    /// `(prefix, numeral system)` per piece, in order.
    pub pieces: Vec<IrNumberingPiece>,
    /// The pattern suffix (for example `"."` or `")"`).
    pub suffix: String,
    /// Whether nested labels include their parent numbers.
    pub full: bool,
}

/// One piece of a numbering pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IrNumberingPiece {
    /// Literal prefix before this piece's number.
    pub prefix: String,
    /// Numeral system of this piece.
    pub system: IrNumeralSystem,
}

/// One numbering level definition, as derived from a pattern piece.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IrLevelDef {
    /// Word numbering format (`"decimal"`, `"lowerLetter"`, …).
    pub num_fmt: String,
    /// Level text with `%N` placeholders (for example `"%1."`).
    pub lvl_text: String,
}

/// The numeral system of one numbering-pattern piece.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrNumeralSystem {
    Arabic,
    LowerLatin,
    UpperLatin,
    LowerRoman,
    UpperRoman,
}

/// A block equation.
#[derive(Debug, Clone, PartialEq)]
pub struct IrEquation {
    /// Pre-rendered OMML.
    pub omml: String,
    /// The equation number (for example `"(1.1)"`), when numbered.
    pub number: Option<String>,
    /// Bookmark name for cross-references, when the equation is labelled.
    pub bookmark: Option<String>,
}

/// A raw (code) block: one run list per line. Runs carry their syntax
/// colours; the lowering emits them (the legacy path dropped colours).
#[derive(Debug, Clone, PartialEq)]
pub struct IrRaw {
    pub lines: Vec<Vec<Run>>,
    /// The raw block's language, when declared (` ```rust `).
    pub language: Option<String>,
}

/// One outline entry.
#[derive(Debug, Clone, PartialEq)]
pub struct IrOutlineEntry {
    /// Heading level of the entry.
    pub level: u8,
    /// Entry title inlines.
    pub title: Vec<IrInline>,
    /// Target bookmark name.
    pub target: Option<String>,
    /// Rendered page number, as a literal snapshot (C2; C3 turns it into
    /// a PAGEREF field).
    pub page_number: Option<String>,
}

/// One bibliography entry: rendered text plus its bookmark name.
///
/// `bookmark` is the entry's bookmark (`"bib-n"`, n = entry index), not
/// the citation key: keys are not recoverable from the rendered entry
/// order in the general case (author-year styles sort alphabetically,
/// not by citation order), and the legacy path bookmarks entries by
/// their HTML `loc-N` id the same way. Citations link to this bookmark;
/// the citation keys live in `IrInline::Cite.keys` and in the
/// `CitationSource` store.
#[derive(Debug, Clone, PartialEq)]
pub struct IrBibEntry {
    /// Bookmark name for this entry (for example `"bib-0"`).
    pub bookmark: String,
    /// Rendered marker (for example `"[1]"`), when the pairs carry it.
    /// The lowering falls back to a sequential `"[n]"` when absent.
    pub marker: Option<String>,
    /// Rendered entry text, as inlines.
    pub body: Vec<IrInline>,
}

/// What a figure is a figure of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrFigureKind {
    /// A figure with an image body.
    Image,
    /// A figure with a table body.
    Table,
    /// A figure with other content (raw, equation, …).
    Other,
}

/// A rasterized figure: the bytes of a figure body rendered to an image,
/// used when the body has no native OOXML encoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IrRaster {
    pub bytes: Vec<u8>,
    pub width_emu: u64,
    pub height_emu: u64,
}

/// Image sizes by content hash, as measured from the companion Paged
/// document. The collector resolves these into `IrImage` before lowering,
/// so the lowering never consults this map.
pub type ImageSizes = HashMap<u64, (u64, u64)>;
