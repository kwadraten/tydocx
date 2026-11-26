use std::collections::HashMap;

pub use tydocx_ooxml::document::{
    Alignment, AnchoredLine, CitationSource, DocumentStyle, FootnoteFormat, HangingIndent,
    InlineShape, PageNumberFormat, PageSettings, ParagraphStyle, Run, ShapeCap, ShapeInsets,
    ShapeJoin, ShapeOutline, ShapePathSeg, ShapeStroke, SrcRect, TableBorders, TableCellMargins,
    VerticalAlign,
};

#[derive(Debug, Clone, PartialEq)]
pub struct IrDocument {
    pub info: IrDocumentInfo,
    /// Page settings of the first section (and the only section when
    /// `sections` is empty).
    pub page: PageSettings,
    pub style: DocumentStyle,
    pub body: Vec<IrBlock>,
    pub equation_refs: HashMap<String, (String, Option<String>)>,
    /// Sections after the first, in body order. Each starts at
    /// `start_block` in `body`; the section's own settings end the body
    /// (`w:sectPr`), while earlier sections close via a section break on the
    /// last paragraph before `start_block`.
    pub sections: Vec<IrSection>,
    /// `w:pgNumType w:fmt` declared for the first section; only set when
    /// `sections` is non-empty (single-section documents use
    /// `GeometryInput::page_numbering`).
    pub first_numbering: Option<PageNumberFormat>,
    pub footnotes: Vec<IrFootnote>,
    pub citation_sources: Vec<CitationSource>,
    /// Text declared in `#set page(foreground:)` / `background:`. Rendered on
    /// every page by Typst but dropped by Word conversion; C1 literalizes it
    /// into the body with a `page-foreground` degradation.
    pub page_furniture: Vec<String>,
}

/// A Typst section after the first (a mid-document page-settings change).
#[derive(Debug, Clone, PartialEq)]
pub struct IrSection {
    /// Index into `IrDocument::body` where the section's first block sits.
    pub start_block: usize,
    /// The section's resolved page settings (`w:pgSz`/`w:pgMar`/`w:cols`).
    pub settings: PageSettings,
    /// `w:pgNumType w:fmt` for the section.
    pub numbering: Option<PageNumberFormat>,
    /// `w:pgNumType w:start` restart (`None` = continue numbering).
    pub numbering_start: Option<u32>,
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
    /// The `\@ "..."` picture a bare `document.date` read resolves to;
    /// `None` when `document.date` is `none`.
    pub date_picture: Option<&'static str>,
}

/// Which Word field code an [`IrInline::Field`] emits at C3.
#[derive(Debug, Clone, PartialEq)]
pub enum IrField {
    /// ` NUMPAGES ` — Typst `context counter(page).final()`.
    NumPages,
    /// ` NOTEREF <label> \h ` — `counter(footnote).at(<label>)` where the
    /// label marks a footnote; that footnote carries the matching bookmark.
    NoteRef { label: String },
    /// ` DATE ` — `datetime.today()`. `picture` is the `\@ "..."` date/time
    /// picture the Typst display pattern translates to; `None` emits a bare
    /// field (unmappable or computed pattern — reported upstream).
    Date { picture: Option<String> },
    /// ` TIME ` — a `datetime`-shaped time call.
    Time,
    /// ` STYLEREF "Heading <level>" ` — a running-header heading
    /// reference (`context` + a `query(heading)`/`locate` body in a page
    /// header or footer).
    StyleRef { level: u8 },
    /// ` DOCPROPERTY <name> ` — a `context document.<field>` read
    /// (`document.title`, `document.author`, `document.description`,
    /// `document.keywords`, `document.date`). `picture` is the `\@ "..."`
    /// date/time picture for `CreateDate`; `None` emits a bare field.
    DocProperty {
        name: String,
        picture: Option<String>,
    },
    /// ` SEQ <identifier> ` — a single-piece numeral spliced out of literal
    /// numbering text (figure captions, equation numbers).
    Seq {
        identifier: String,
        system: IrNumeralSystem,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct IrFootnote {
    pub ordinal: u32,
    pub rendered: String,
    pub body: Vec<IrParagraph>,
    /// The footnote element's own `<label>`, when one is attached —
    /// `counter(footnote).at(<label>)` references point at it.
    pub label: Option<String>,
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

/// The resolved `DrawingML` payload of a bare shape — the geometry an
/// [`IrBlock::Shape`] or [`IrInline::Shape`] draws as a `wp:inline` +
/// `wps:wsp` at C2/C3. This is [`InlineShape`] minus the text body: the
/// lowerer pairs it with the shape's lowered body (the `wps:txbx` content)
/// and `fill_hex` to build the drawing.
#[derive(Debug, Clone, PartialEq)]
pub struct IrShapeGeometry {
    /// `wp:extent`/`a:ext` — the painted size in EMU.
    pub width_emu: u64,
    pub height_emu: u64,
    /// `a:prstGeom`/`a:custGeom` — the shape's geometry.
    pub outline: ShapeOutline,
    /// `a:ln` — the resolved uniform stroke; `None` = no stroke.
    pub stroke: Option<ShapeStroke>,
    /// `wps:bodyPr` insets in EMU positioning the shape's text body.
    pub inset_emu: ShapeInsets,
    pub cell_rect: Option<IrCellRect>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IrCellRect {
    pub margins: TableCellMargins,
    pub bottom_border: Option<IrBorder>,
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
        labels: Vec<String>,
        /// `par` settings resolved on the heading body's `StyleChain`
        /// (e.g. `#show heading: set par(first-line-indent: 0pt)`); emitted as
        /// `w:ind`/`w:jc` overrides when they differ from Normal.
        par: IrParStyle,
        alignment: Option<Alignment>,
        span: Option<typst_syntax::Span>,
    },
    Table(IrTable),
    Grid(IrGrid),
    Figure {
        body: Vec<IrBlock>,
        caption: Vec<IrInline>,
        caption_alignment: Option<Alignment>,
        /// `par` settings resolved on the caption body's `StyleChain`
        /// (e.g. `#show figure.caption: set par(first-line-indent: 0pt)`);
        /// emitted as `w:ind`/`w:jc` overrides when they differ from Normal.
        caption_par: IrParStyle,
        /// The numeral's position inside `caption`'s flattened text plus its
        /// split — the C3 ` SEQ ` field splices it out at lowering, so the
        /// caption inlines themselves stay byte-identical to C2.
        caption_seq: Option<IrCaptionSeq>,
        kind: IrFigureKind,
        supplement: Option<String>,
        labels: Vec<String>,
        caption_first: bool,
        custom_placement: bool,
        /// `figure(gap:)` override in twips — spacing between body and caption.
        gap_twips: Option<u32>,
        /// Hash of the figure's introspection `Location`; matches the paged
        /// frame's figure tag brackets so drawn canvases reach this figure.
        location_hash: Option<u64>,
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
        title_bookmark: Option<String>,
        entries: Vec<IrOutlineEntry>,
        depth: u8,
        custom_target: bool,
        /// Left indent added per entry level (`level - 1` times this many
        /// twips): Typst's default is `1.2em`; a `Rel` indent resolves to
        /// its declared length per level.
        indent_twips_per_level: u32,
        /// When every entry came from figures of one kind, the Word `SEQ`
        /// identifier the TOC field's `\c` switch regenerates (`"Figure"`,
        /// `"Table"`). `None` for heading TOCs and mixed-kind figure TOCs.
        figure_caption_id: Option<String>,
    },
    Bibliography {
        entries: Vec<IrBibEntry>,
    },
    Place {
        float: bool,
        /// Word wrap mode implied by the float's declared alignment. Typst
        /// floats must anchor `top` or `bottom` (`auto` resolves to one of
        /// those at layout), so `TopAndBottom` is the normal case; `Square`
        /// covers a defensive fallback only.
        wrap: IrPlaceWrap,
        /// `place(clearance:)` in EMU → `wp:anchor` `distT`/`distB`.
        clearance_emu: u64,
        /// Hash of the place's introspection `Location`; matches the paged
        /// float group, which carries the same location as its frame parent.
        location_hash: Option<u64>,
        /// Page-absolute geometry of the rendered float, filled by the
        /// paged-frame patch (`realize::place`). `None` until measured.
        anchor: Option<IrAnchor>,
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
        fill_hex: Option<String>,
        /// The `DrawingML` payload the collector resolved, or `None` when the
        /// size or path could not be resolved — the lowerer then keeps the
        /// C2 shaded-text fallback (and reports `loss`).
        geometry: Option<IrShapeGeometry>,
        /// Paint or geometry detail the resolver could not carry (per-side
        /// strokes, non-solid paint, even-odd fill, dash phase, …); the
        /// lowerer reports it as a `shape` degradation at C2/C3.
        loss: Option<String>,
        /// `align(..)` folded around the shape — the `w:jc` on the carrier
        /// paragraph.
        alignment: Option<Alignment>,
    },
    /// A `#columns(n, gutter:)[…]` block. At the top level the lowerer
    /// wraps it in a pair of `continuous` section breaks — the outgoing
    /// break's `w:sectPr` carries `w:cols` (it ends the block's section),
    /// and the ambient section resumes after it. Inside a nested body
    /// (table cell, figure, …) OOXML offers no column scoping, so it
    /// degrades to a flattened `column-block` instead.
    Columns {
        /// `w:cols w:num` — the column count.
        count: u32,
        /// `w:cols w:space` in twips — the gutter resolved on the element's
        /// `StyleChain` against the ambient text width (Typst's default is
        /// a `4%` ratio, not a length).
        gutter_twips: u32,
        body: Vec<IrBlock>,
        span: Option<typst_syntax::Span>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrSectionBreakType {
    Odd,
    Even,
}

/// Word wrap mode for a floating `wp:anchor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrPlaceWrap {
    /// `wp:wrapTopAndBottom` — the float occupies a band at the top or
    /// bottom of its container and text flows above/below it.
    TopAndBottom,
    /// `wp:wrapSquare` — text wraps on all four sides.
    Square,
}

/// Page-absolute geometry of a rendered float or non-floating placement,
/// measured from the paged frame.
#[derive(Debug, Clone, PartialEq)]
pub struct IrAnchor {
    /// Offset from the page's left edge (`relativeFrom="page"`), EMU.
    pub x_emu: i64,
    /// Offset from the page's top edge (`relativeFrom="page"`), EMU.
    pub y_emu: i64,
    pub width_emu: u64,
    pub height_emu: u64,
    /// `wp:anchor@behindDoc` — the content paints behind the page's text.
    /// For non-floating `#place` content this follows Typst's paint order
    /// relative to the body text layer: it goes behind when text still
    /// paints after it on the page. Floats keep `false` — Word displaces
    /// flow text for them anyway.
    pub behind_doc: bool,
    /// The solid colour painting directly under this unit on the page — the
    /// fill of an earlier placed rect covering the unit's bounds, or the
    /// page's own solid `fill`, or `"FFFFFF"` paper when neither declares
    /// one. Word gives a `wps:txbx` no fill unless asked, so a text box over
    /// a coloured backdrop would show it through; the lowerer paints this on
    /// the shape when the place itself declares no fill.
    pub backdrop_hex: Option<String>,
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
    /// `par` settings resolved on the element's own `StyleChain`; the lowerer
    /// emits them as `w:ind`/`w:jc` overrides wherever they differ from the
    /// document-global Normal style. `IrParStyle::default()` (no resolved
    /// values) emits nothing — the paragraph inherits Normal, as before.
    pub par: IrParStyle,
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
    pub bookmarks: Vec<String>,
}

/// `par` element settings resolved on an element's own `StyleChain` — the
/// values Typst actually laid the paragraph out with, not just source-declared
/// overrides. `None` fields mean "not resolved" (synthetic paragraphs that
/// never had a chain), and emit no override.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IrParStyle {
    /// `par.first-line-indent`, resolved at the chain's font size.
    pub first_line_indent: Option<IrFirstLineIndent>,
    /// `par.justify` — `true` = justified (`w:jc both`), `false` = left.
    pub justify: Option<bool>,
    pub line_spacing: Option<IrLineSpacing>,
    pub spacing_after: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrLineSpacing {
    AtLeast(u32),
    Exact(u32),
}

/// `par.first-line-indent` expressed the way OOXML wants it: a twip amount
/// plus, for pure-em amounts, the char-based value Word prefers for East-Asian
/// indents (`round(em × 100)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IrFirstLineIndent {
    /// `w:firstLine` amount in twips; negative values are an outdent and emit
    /// `w:hanging` instead.
    pub twips: i64,
    /// `w:firstLineChars` for pure-em indents (`em × 100`).
    pub chars: Option<i64>,
}

/// The page-counter read behind a `PAGEREF`-shaped ref — where the counter
/// is read and whether the rendered number is formatted through the page
/// numbering at that spot.
#[derive(Debug, Clone, PartialEq)]
pub struct IrPageTarget {
    /// `Location::hash()` of the element the counter is read at — the
    /// ref's target (`ref`, `counter(page).at(<label>)`).
    pub location: u128,
    /// `ref(form: "page")` formats through the target's page numbering;
    /// `counter(page).at(<label>)` keeps the raw counter-state shape.
    /// Only consulted when `span` is absent — a span lets the patch
    /// re-parse the full display shape.
    pub numbered: bool,
    /// Source span of the `counter(page).at(<label>)…` expression — the
    /// patch re-classifies a trailing `.display(...)` on the chain;
    /// `None` for `ref(form: "page")`.
    pub span: Option<typst_syntax::Span>,
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
        /// Where a `PAGEREF`-shaped ref reads the page counter — the patch
        /// resolves it to the rendered number; `None` on ordinary refs.
        page_target: Option<IrPageTarget>,
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
    /// `#linebreak(justify:)` — Word stretches `w:br`-terminated lines under an
    /// inherited `w:jc="both"`, so the flag must survive to the paragraph
    /// alignment decision (see `lower_paragraph`).
    LineBreak {
        justify: bool,
    },
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
        /// `Location::hash()` of the `context` element — the paged patch
        /// resolves it to the physical page and numbering at this spot.
        location: Option<u128>,
        /// Source span of the counter expression — the patch re-classifies
        /// the display shape (`.display(...)` vs raw `.at`/`.final`).
        span: Option<typst_syntax::Span>,
    },
    /// A Word field (`w:fldChar`/`w:instrText` complex field). `body` holds
    /// the inlines the field replaces: at C1/C2 it lowers verbatim (the
    /// field's cached result equals the literal output — D1); at C3 the
    /// `kind` decides the `w:instrText` field code.
    Field {
        kind: IrField,
        body: Vec<IrInline>,
        /// Source span, so a pre-C3 fallback degradation keeps its position.
        span: Option<typst_syntax::Span>,
        /// `Location::hash()` of the `context` element — the paged patch
        /// resolves counter expressions (`counter(page).final()`) at it.
        location: Option<u128>,
    },
    Shape {
        kind: IrShapeKind,
        span: Option<typst_syntax::Span>,
        body: Vec<IrInline>,
        fill_hex: Option<String>,
        /// Same payload contract as [`IrBlock::Shape`]: `None` falls back to
        /// the C2 shaded-text form (with `loss` reported).
        geometry: Option<IrShapeGeometry>,
        /// Geometry/paint detail dropped on the way; reported at C2/C3.
        loss: Option<String>,
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
    /// Original SVG bytes for vector content; emitted as `asvg:svgBlip`
    /// inside `mc:AlternateContent` with `bytes` (PNG) as the fallback.
    pub svg: Option<Vec<u8>>,
    /// `a:srcRect` crop window derived from the paged frame's clip geometry
    /// (`block/box(clip:)`, `image(fit: "cover")`), filled by the paged patch.
    pub src_rect: Option<SrcRect>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IrTable {
    pub rows: Vec<IrTableRow>,
    pub column_widths_pct: Option<Vec<u32>>,
    pub borders: Option<TableBorders>,
    pub gutter: bool,
    pub cell_margins: Option<TableCellMargins>,
    /// `align(..)` folded around the table — `w:jc` in `w:tblPr`.
    pub alignment: Option<Alignment>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IrTableRow {
    pub cells: Vec<IrTableCell>,
    /// Rendered or declared row height in twips, when known. Emitted as an
    /// exact `w:trHeight` so 5-minute timetable slots keep their pitch.
    pub height_twips: Option<u32>,
    /// Row repeats as a `w:tblHeader` on every page the table spills onto.
    pub is_header: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IrTableCell {
    pub body: Vec<IrBlock>,
    /// Grid column (non-gutter track) the cell starts in.
    pub x: usize,
    pub colspan: u32,
    pub rowspan: u32,
    pub vmerge: IrVMerge,
    pub fill_hex: Option<String>,
    pub vertical_align: Option<VerticalAlign>,
    pub inset_twips: Option<u32>,
    /// Per-side strokes resolved on the cell, used when a grid's stroke
    /// geometry must be reproduced cell-by-cell rather than as table borders.
    pub borders: IrCellBorders,
    /// Which `borders` sides were declared on the cell itself rather than
    /// inherited from the grid's `stroke` function. Boundary resolution
    /// gives a declared edge precedence over an undeclared one.
    pub borders_declared: IrCellBorderFlags,
}

/// Which of a cell's four edge strokes were set on the cell itself rather
/// than inherited from the grid's `stroke` function. Boundary resolution
/// gives a declared edge precedence over an undeclared neighbour.
#[allow(
    clippy::struct_excessive_bools,
    reason = "four per-edge flags mirror the cell's Sides<..> stroke shape"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IrCellBorderFlags {
    pub top: bool,
    pub right: bool,
    pub bottom: bool,
    pub left: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct IrCellBorders {
    pub top: Option<IrBorder>,
    pub right: Option<IrBorder>,
    pub bottom: Option<IrBorder>,
    pub left: Option<IrBorder>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IrBorder {
    /// Stroke thickness in eighths of a point (`w:sz` units).
    pub sz_eighths: u32,
    pub color_hex: Option<String>,
    /// Dashed lines emit `w:val="dashed"` instead of `single`.
    pub dashed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IrTrackSize {
    Auto,
    /// Fixed track length in twips.
    Fixed(u32),
    /// `fr` weight (e.g. `2fr` → 2.0).
    Fr(f64),
    /// `abs + ratio` sizing: absolute part in twips plus a share of the
    /// container in thousandths.
    Rel {
        abs_twips: u32,
        permille: u32,
    },
}

/// A `grid.hline`/`grid.vline` resolved onto the track grid. `index` is the
/// boundary after that many tracks (non-gutter space) — a line placed
/// `after` track `k` on a guttered grid is normalized to boundary `k + 1`,
/// the same DOCX edge; `start..end` is the covered perpendicular range.
#[derive(Debug, Clone, PartialEq)]
pub struct IrGridLine {
    pub index: usize,
    pub start: usize,
    pub end: usize,
    /// `None` is a `stroke: none` line: it removes every stroke on the
    /// covered boundary segments, including cell edges declared there.
    pub border: Option<IrBorder>,
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
    /// Every non-gutter row, in order, including the empty cells Typst
    /// materializes at positions no declared cell covers (the mergeable
    /// whitespace of timetables).
    pub rows: Vec<IrTableRow>,
    pub column_widths_pct: Option<Vec<u32>>,
    pub borders: Option<TableBorders>,
    /// Non-gutter track sizings, same length as `rows` / column count.
    pub row_sizes: Vec<IrTrackSize>,
    pub col_sizes: Vec<IrTrackSize>,
    pub hlines: Vec<IrGridLine>,
    pub vlines: Vec<IrGridLine>,
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
    /// The item's `number:` was set explicitly — a `SEQ` field cannot
    /// express it, so the item stays literal (`enum-seq-override`).
    pub number_override: bool,
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
    /// The `\*` format switch for a `SEQ` field code (`None` for arabic —
    /// the field's own default).
    #[must_use]
    pub fn seq_switch(self) -> Option<&'static str> {
        match self {
            Self::Arabic => None,
            Self::LowerLatin => Some("alphabetic"),
            Self::UpperLatin => Some("ALPHABETIC"),
            Self::LowerRoman => Some("roman"),
            Self::UpperRoman => Some("ROMAN"),
        }
    }

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
    pub number: Option<IrEquationNumber>,
    pub bookmarks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IrEquationNumber {
    pub display: String,
    pub prefix: String,
    pub suffix: String,
    pub seq: Option<IrSeqSplit>,
}

impl IrEquationNumber {
    #[must_use]
    pub fn value(&self) -> &str {
        self.display
            .strip_prefix(&self.prefix)
            .and_then(|value| value.strip_suffix(&self.suffix))
            .unwrap_or(&self.display)
    }
}

/// The caption numeral's location inside a figure caption's flattened text —
/// spliced out into an `IrInline::Field` only when C3 lowers the caption.
#[derive(Debug, Clone, PartialEq)]
pub struct IrCaptionSeq {
    /// Char offset of `seq.numeral` within the caption's visible text.
    pub offset: usize,
    /// The numeral split — identifier and numeral system drive the SEQ code.
    pub seq: IrSeqSplit,
}

/// A numeral split out of rendered numbering text by its pattern's affixes.
#[derive(Debug, Clone, PartialEq)]
pub struct IrSeqSplit {
    /// The pattern's prefix inside `display` ("(").
    pub prefix: String,
    /// The rendered numeral itself ("1").
    pub numeral: String,
    /// The pattern's suffix inside `display` (")").
    pub suffix: String,
    /// The numeral system — decides the SEQ field's `\*` switch.
    pub system: IrNumeralSystem,
    /// The SEQ identifier (the element's counter name, e.g. "equation").
    pub identifier: String,
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
    /// Bookmark name the entry hyperlinks to (the heading's own label, or a
    /// synthesized `outline-target-N` the collector registers on the heading).
    pub target: Option<String>,
    pub page_number: Option<String>,
    /// `Location::hash()` of the target element — the paged patch resolves it
    /// to the rendered page number via `Location::new(hash)` (locations are
    /// stable content hashes across compiles).
    pub page_loc: Option<u128>,
}

/// `bookmark` is the entry's bookmark (`"bib-n"`), not the citation key:
/// keys are not recoverable from rendered entry order in the general case.
#[derive(Debug, Clone, PartialEq)]
pub struct IrBibEntry {
    pub bookmark: String,
    pub marker: Option<Vec<IrInline>>,
    pub body: Vec<IrInline>,
    pub hanging_twips: Option<u32>,
    pub custom_display: bool,
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
