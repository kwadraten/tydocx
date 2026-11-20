use std::borrow::Cow;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParagraphStyle {
    Normal,
    Heading(u8),
}

impl ParagraphStyle {
    /// The `w:pStyle` value string. `Normal` borrows its static name; a
    /// heading level is formatted, so it owns its string — hence `Cow`
    /// instead of `&'static str` like the sibling value methods.
    #[must_use]
    pub fn ooxml_value(&self) -> Cow<'static, str> {
        match self {
            Self::Normal => Cow::Borrowed("Normal"),
            Self::Heading(level) => Cow::Owned(format!("Heading{level}")),
        }
    }
}

/// Paragraph alignment / justification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    Left,
    Center,
    Right,
    Justify,
}

impl Alignment {
    /// Return the OOXML `w:jc` value string for this alignment.
    #[must_use]
    pub fn ooxml_value(self) -> &'static str {
        match self {
            Alignment::Left => "left",
            Alignment::Center => "center",
            Alignment::Right => "right",
            Alignment::Justify => "both",
        }
    }

    #[deprecated(since = "0.2.1", note = "renamed to `ooxml_value`")]
    #[must_use]
    pub fn as_ooxml_str(&self) -> &'static str {
        self.ooxml_value()
    }
}

/// Image format (PNG or JPEG).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Svg,
}

impl ImageFormat {
    #[must_use]
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Svg => "svg",
        }
    }

    #[must_use]
    pub fn content_type(&self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Svg => "image/svg+xml",
        }
    }
}

/// Embedded image data with dimensions in EMU (English Metric Units).
#[derive(Debug, Clone, PartialEq)]
pub struct ImageData {
    pub bytes: Vec<u8>,
    pub format: ImageFormat,
    /// Original vector bytes when the source was vector: emitted as
    /// `asvg:svgBlip` in `mc:AlternateContent`, with `bytes` as the PNG
    /// fallback for consumers without the SVG extension.
    pub svg: Option<Vec<u8>>,
    pub width_emu: u64,
    pub height_emu: u64,
    /// `a:srcRect` — a crop window into the full image, derived from the
    /// paged frame's clip geometry (`block/box(clip:)`, `image(fit: "cover")`).
    /// The original bytes stay embedded; Word displays only the window.
    pub src_rect: Option<SrcRect>,
}

/// `a:srcRect` — the visible window of an embedded image as a fraction of the
/// full image bounds, each side in thousandths of a percent (100000 = 100%).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SrcRect {
    pub l: i32,
    pub t: i32,
    pub r: i32,
    pub b: i32,
}

/// List numbering info for a paragraph that is a list item.
#[derive(Debug, Clone, PartialEq)]
pub struct ListInfo {
    /// Nesting level (0-based).
    pub level: u32,
    /// Numbering definition ID (e.g. 1=ordered, 2=unordered).
    pub id: u32,
}

/// Word bibliography source type (`ST_SourceType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceType {
    JournalArticle,
    Book,
    BookSection,
    ConferenceProceedings,
    Report,
    Thesis,
    InternetSite,
    DocumentFromInternetSite,
    Misc,
}

impl SourceType {
    #[must_use]
    pub fn ooxml_value(self) -> &'static str {
        match self {
            Self::JournalArticle => "JournalArticle",
            Self::Book => "Book",
            Self::BookSection => "BookSection",
            Self::ConferenceProceedings => "ConferenceProceedings",
            Self::Report | Self::Thesis => "Report",
            Self::InternetSite => "InternetSite",
            Self::DocumentFromInternetSite => "DocumentFromInternetSite",
            Self::Misc => "Misc",
        }
    }

    #[deprecated(since = "0.2.1", note = "renamed to `ooxml_value`")]
    #[must_use]
    pub fn as_ooxml_str(&self) -> &'static str {
        self.ooxml_value()
    }
}

/// A person's name for Word bibliography author fields.
#[derive(Debug, Clone, PartialEq)]
pub struct PersonName {
    pub last: String,
    pub first: Option<String>,
    pub middle: Option<String>,
}

/// A citation source entry for Word's bibliography data store (`customXml/item1.xml`).
#[derive(Debug, Clone, PartialEq)]
pub struct CitationSource {
    /// Word keys sources by tag (and the writer derives the GUID from it), so
    /// tags must be unique within a `Document` — the writer does not dedup;
    /// producers (`extract_bibliography_sources`) own that invariant.
    pub tag: String,
    pub source_type: SourceType,
    pub authors: Vec<PersonName>,
    pub title: Option<String>,
    pub year: Option<String>,
    pub journal_name: Option<String>,
    pub volume: Option<String>,
    pub issue: Option<String>,
    pub pages: Option<String>,
    pub doi: Option<String>,
    pub url: Option<String>,
    pub publisher: Option<String>,
    pub city: Option<String>,
    pub edition: Option<String>,
    pub book_title: Option<String>,
}

/// A horizontal line drawn at an absolute position, anchored to a text run
/// (`wp:anchor` + `wps:wsp` with a `line` preset geometry). Used for the
/// overline trick: Typst renders overlines as line shapes, so the C2
/// representation is the same kind of object — a floating line above the
/// anchored line of text.
#[derive(Debug, Clone, PartialEq)]
pub struct AnchoredLine {
    /// Horizontal position relative to the page edge (`relativeFrom="page"`),
    /// in EMU.
    pub x_emu: i64,
    /// Vertical offset downward from the top of the anchor's text line
    /// (`relativeFrom="line"`), in EMU. Typst draws the overline just below
    /// the ascender top, so measured values are small positive numbers.
    pub y_emu: i64,
    /// Line length, in EMU.
    pub length_emu: u64,
    /// Stroke thickness, in EMU.
    pub thickness_emu: u64,
    /// Stroke color as a 6-digit hex string. `None` = automatic (black).
    pub color: Option<String>,
}

/// Floating content anchored at a page-absolute position (`wp:anchor`), used
/// for Typst `#place` — both `float: true` (text wraps around it) and the
/// non-floating overlay form (content paints over/under the page's flow). The
/// paragraph carrying this element keeps the float's logical position in
/// document order (accessibility); the drawing itself holds the rendered
/// position and wrap behavior.
#[derive(Debug, Clone, PartialEq)]
pub struct AnchoredFloat {
    /// `wp:positionH` offset in EMU, measured against `horz_rel`: from the
    /// page edge when `Page`, from the text-area edge when `Margin`
    /// (negative values reach into the page's side margin).
    pub x_emu: i64,
    /// Vertical offset from the page edge (`relativeFrom="page"`), EMU.
    pub y_emu: i64,
    /// `wp:positionH@relativeFrom` — the reference edge `x_emu` is measured
    /// against.
    pub horz_rel: RelFrom,
    /// Drawing extent, EMU.
    pub width_emu: u64,
    pub height_emu: u64,
    /// `place(clearance:)` mapped to `wp:anchor@distT`/`distB`, EMU.
    pub clearance_emu: u64,
    /// How the page's text interacts with the drawing.
    pub wrap: AnchorWrap,
    /// `wp:anchor@behindDoc` — the drawing paints behind the page's text.
    /// Typst paints non-floating placed content over earlier items and under
    /// later ones; `true` is the two-layer approximation for placements that
    /// precede other content on the page.
    pub behind_doc: bool,
    /// `wp:anchor@relativeHeight` — z-order within the drawing layer.
    /// Anchors are numbered in document order so a later `#place` stacks
    /// over an earlier one the way Typst's paint order does; equal values
    /// leave stacking to renderer fallbacks.
    pub z_order: u32,
    pub content: AnchoredContent,
}

/// The reference edge a `wp:positionH` offset is measured against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelFrom {
    /// `relativeFrom="page"` — offset from the paper's left edge.
    Page,
    /// `relativeFrom="margin"` — offset from the text area's left edge, so a
    /// negative `x_emu` lands inside the page's left margin (margin notes).
    Margin,
}

impl RelFrom {
    #[must_use]
    pub fn attr(self) -> &'static str {
        match self {
            Self::Page => "page",
            Self::Margin => "margin",
        }
    }
}

/// The `wp:anchor` wrap element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorWrap {
    /// `wp:wrapTopAndBottom` — a float occupies a band at the top or bottom
    /// of its container; text flows above/below it. Typst floats anchor top
    /// or bottom, so this is the normal float case.
    TopAndBottom,
    /// `wp:wrapSquare` — text wraps on all four sides.
    Square,
    /// `wp:wrapNone` — text does not interact with the drawing at all. This
    /// is the non-floating `#place` form: Typst lays the placed frame over
    /// (or under) the flow without displacing it.
    None,
}

/// What a `wp:anchor` draws: a picture or a text box.
#[derive(Debug, Clone, PartialEq)]
pub enum AnchoredContent {
    /// A `pic:pic` — a floated image.
    Image(ImageData),
    /// A `wps:txbx` text box holding real block content so the float stays
    /// editable text rather than a raster.
    Text {
        blocks: Vec<BlockElement>,
        /// `a:solidFill` on the shape itself — a placed `rect(fill:)` draws
        /// as a filled box, not as text-line shading.
        fill_hex: Option<String>,
    },
}

/// A single inline element within a paragraph.
#[derive(Debug, Clone, PartialEq)]
pub enum InlineElement {
    /// Normal text run.
    Text(Run),
    /// A footnote reference (rendered as superscript number in the document).
    /// The `id` corresponds to the footnote in the `Document::footnotes` list.
    FootnoteRef(u32),
    /// A math equation rendered as OMML XML.
    /// If `equation_number` is set, the equation is a numbered block equation.
    Math {
        omml_xml: String,
        equation_number: Option<EquationNumber>,
    },
    /// An inline image.
    Image(ImageData),
    /// A bookmark start marker (anchor for cross-references).
    Bookmark { id: u32, name: String },
    /// A bookmark end marker.
    BookmarkEnd { id: u32 },
    /// A cross-reference field (REF field code pointing at a bookmark).
    FieldRef {
        bookmark_name: String,
        display_text: String,
    },
    /// A page-number cross-reference field (PAGEREF field code pointing at a
    /// bookmark). The cached `display_text` is the page number Typst rendered
    /// for the target (e.g. from `counter(page).at(<label>)`).
    FieldPageRef {
        bookmark_name: String,
        display_text: String,
    },
    /// A current-page field (PAGE field code). The cached `display_text` is the
    /// page number Typst rendered at this spot (e.g. from a bare
    /// `counter(page).display()` or `counter(page).at(here())`); Word recomputes
    /// the field, the cache is only the pre-update display value. `bold` and
    /// `size_half_pt` style the field's runs (the cached result and the field
    /// chars) so a styled page-number token in a header/footer keeps its look.
    FieldPage {
        display_text: String,
        bold: bool,
        size_half_pt: Option<u32>,
    },
    /// An external hyperlink using `fldSimple` with HYPERLINK field code.
    Hyperlink { url: String, runs: Vec<Run> },
    /// An internal hyperlink (`w:hyperlink w:anchor`) to a bookmark — e.g. a
    /// citation marker linking to its bibliography entry. The runs keep their own
    /// styling (no Hyperlink char style), so the marker just becomes clickable.
    InternalLink { anchor: String, runs: Vec<Run> },
    /// A page break (`w:br type="page"`).
    PageBreak,
    /// A floating line shape anchored to this position in the text
    /// (`wp:anchor`); used for the overline trick.
    AnchoredLine(AnchoredLine),
    /// A floating image or text box anchored to the page at this position
    /// (`wp:anchor`); used for `#place(.., float: true)`. Its carrier
    /// paragraph holds a 1pt mark in the flow.
    AnchoredFloat(AnchoredFloat),
    /// A column break (`w:br type="column"`) — `#colbreak()` in a multi-column page.
    ColumnBreak,
    /// A Table of Contents field code (`TOC \o "1-N" \h \z \u`).
    FieldToc {
        /// Maximum outline depth (e.g. 3 → headings 1–3).
        max_depth: u8,
    },
    /// A tab character (`w:r` containing `w:tab`).
    Tab,
    /// A Word citation wrapped in a Structured Document Tag (SDT).
    Citation {
        /// Citation keys (multiple for merged citations like "[1, 3]").
        keys: Vec<String>,
        /// Rendered display text (e.g., "[1]" or "(Author, 2020)").
        display_text: String,
        /// Bookmark the marker links to (the bibliography entry), when the
        /// citation resolved to a rendered entry — the cached result stays
        /// clickable exactly like a C2 internal link.
        anchor: Option<String>,
    },
    /// A generic complex field (`fldChar`/`instrText`/`separate`/`end`).
    /// `result` is the cached result — the runs the field replaced, so
    /// disconnecting the field reproduces the literal output exactly.
    Field {
        /// Field instruction as written into `w:instrText`
        /// (e.g. `" NUMPAGES "`, `" STYLEREF \"heading 1\" "`).
        instruction: String,
        /// Cached result runs — carry the literal text and its styling.
        result: Vec<Run>,
    },
}

/// How a block equation's number is emitted after the math run.
#[derive(Debug, Clone, PartialEq)]
pub enum EquationNumber {
    /// Literal number text, including any pattern affixes ("(1)").
    Text(String),
    /// A `SEQ` field covering just the numeral: the pattern's literal
    /// affixes stay literal text, so "(1)" writes
    /// `"(" + SEQ field("1") + ")"`. `instruction` is the full field code
    /// (e.g. `" SEQ equation \\* roman "`).
    Seq {
        prefix: String,
        instruction: String,
        display: String,
        suffix: String,
    },
}

impl EquationNumber {
    /// The full displayed number text including affixes ("(1)").
    #[must_use]
    pub fn as_text(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Seq {
                prefix,
                display,
                suffix,
                ..
            } => format!("{prefix}{display}{suffix}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "mirrors the independent w:rPr toggles"
)]
pub struct Run {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub superscript: bool,
    pub subscript: bool,
    pub monospace: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub overline: bool,
    /// Highlight color name (e.g. "yellow", "green", "cyan").
    /// `Some(...)` activates highlighting; `None` means no highlight.
    pub highlight_color: Option<String>,
    /// Character shading fill as a 6-digit hex string (e.g. "FFCCCC").
    /// Used for highlight fills that are not one of the sixteen named
    /// `w:highlight` colors; `None` means no shading.
    pub shading: Option<String>,
    pub smallcaps: bool,
    /// Text color as a 6-digit hex string (e.g. "FF0000" for red).
    /// `None` means inherit the default (black).
    pub color: Option<String>,
    /// Per-run font override for ASCII/Latin text (e.g. from show rules).
    /// `None` means inherit the document default.
    pub font_ascii: Option<String>,
    /// Per-run font override for CJK text.
    pub font_east_asia: Option<String>,
    /// Per-run font size override in half-points (e.g. 24 = 12pt).
    /// `None` means inherit the document default.
    pub size_half_pt: Option<u32>,
    /// Character spacing (tracking) in twentieths of a point. `None` or
    /// `Some(0)` means no expansion; `Some(n)` emits `w:spacing w:val="n"`.
    pub tracking_twips: Option<u32>,
    /// Source span for cross-referencing with `PagedDocument` styling.
    pub span: Option<typst_syntax::Span>,
    /// A forced line break (`\` in Typst): the writer emits `<w:r><w:br/></w:r>`
    /// and ignores `text`. Kept distinct so it never coalesces with text runs.
    pub line_break: bool,
    /// OOXML `w:ligatures` value (e.g. `"none"`, `"contextual"`,
    /// `"historical"`). `None` inherits the document default.
    pub ligatures: Option<&'static str>,
    /// OOXML `w:numForm` value (`"lining"` or `"oldStyle"`).
    pub num_form: Option<&'static str>,
    /// OOXML `w:numberSpacing` value (`"proportional"` or `"tabular"`).
    pub number_spacing: Option<&'static str>,
    /// Baseline shift in half-points (`w:position/@w:val`, signed; positive
    /// raises the text).
    pub position_half_pt: Option<i32>,
}

impl Run {
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            bold: false,
            italic: false,
            superscript: false,
            subscript: false,
            monospace: false,
            underline: false,
            strikethrough: false,
            overline: false,
            highlight_color: None,
            shading: None,
            smallcaps: false,
            color: None,
            font_ascii: None,
            font_east_asia: None,
            size_half_pt: None,
            tracking_twips: None,
            span: None,
            line_break: false,
            ligatures: None,
            num_form: None,
            number_spacing: None,
            position_half_pt: None,
        }
    }

    /// A forced line break run (`<w:br/>`); its text is empty.
    #[must_use]
    pub fn line_break() -> Self {
        Self {
            line_break: true,
            ..Self::new("")
        }
    }

    /// See [`RunFormat`].
    #[must_use]
    pub fn format_key(&self) -> RunFormat<'_> {
        // Exhaustive destructure (no `..`): adding a field to `Run` fails to
        // compile HERE until you decide whether it belongs in the key.
        let Run {
            text: _,
            span: _,
            line_break: _,
            bold,
            italic,
            superscript,
            subscript,
            monospace,
            underline,
            strikethrough,
            overline,
            highlight_color,
            shading,
            smallcaps,
            color,
            font_ascii,
            font_east_asia,
            size_half_pt,
            tracking_twips,
            ligatures,
            num_form,
            number_spacing,
            position_half_pt,
        } = self;
        RunFormat {
            bold: *bold,
            italic: *italic,
            superscript: *superscript,
            subscript: *subscript,
            monospace: *monospace,
            underline: *underline,
            strikethrough: *strikethrough,
            overline: *overline,
            highlight_color: highlight_color.as_deref(),
            shading: shading.as_deref(),
            smallcaps: *smallcaps,
            color: color.as_deref(),
            font_ascii: font_ascii.as_deref(),
            font_east_asia: font_east_asia.as_deref(),
            size_half_pt: *size_half_pt,
            tracking_twips: tracking_twips.filter(|t| *t > 0),
            ligatures: *ligatures,
            num_form: *num_form,
            number_spacing: *number_spacing,
            position_half_pt: *position_half_pt,
        }
    }
}

/// The complete set of `Run` fields distinguishing run formatting.
///
/// Single source of truth shared by `writer::run::write_run` (has-rPr gate via
/// `is_plain`) and run coalescing (merge-eligibility via `PartialEq`).
/// Adding a styled field to `Run`? Add it HERE and to `format_key()`'s
/// exhaustive destructure — the compiler will enforce this at `format_key()`
/// until the new field is classified. `text`, `span`, and `line_break` are
/// deliberately not part of the key (line breaks are handled before either site
/// consults the key).
#[derive(Debug, PartialEq, Eq, Default)]
#[allow(clippy::struct_excessive_bools)] // mirrors Run's independent style toggles
pub struct RunFormat<'a> {
    bold: bool,
    italic: bool,
    superscript: bool,
    subscript: bool,
    monospace: bool,
    underline: bool,
    strikethrough: bool,
    overline: bool,
    highlight_color: Option<&'a str>,
    shading: Option<&'a str>,
    smallcaps: bool,
    color: Option<&'a str>,
    font_ascii: Option<&'a str>,
    font_east_asia: Option<&'a str>,
    size_half_pt: Option<u32>,
    tracking_twips: Option<u32>,
    ligatures: Option<&'a str>,
    num_form: Option<&'a str>,
    number_spacing: Option<&'a str>,
    position_half_pt: Option<i32>,
}

impl RunFormat<'_> {
    /// True iff no `<w:rPr>` element is needed for this run.
    #[must_use]
    pub fn is_plain(&self) -> bool {
        *self == RunFormat::default()
    }
}

/// A footnote with its content paragraphs.
#[derive(Debug, Clone, PartialEq)]
pub struct Footnote {
    /// 1-based footnote ID.
    pub id: u32,
    /// The inline content of the footnote (text runs, math, etc.).
    pub content: Vec<InlineElement>,
    /// A bookmark wrapping the footnote's reference mark — the target a
    /// `NOTEREF` field (Typst `counter(footnote).at(<label>)`) points at.
    pub bookmark: Option<FootnoteBookmark>,
}

/// A bookmark name + ID pair anchoring a footnote's reference mark.
#[derive(Debug, Clone, PartialEq)]
pub struct FootnoteBookmark {
    pub id: u32,
    pub name: String,
}

/// Paragraph hanging-indent behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HangingIndent {
    /// Use the bibliography-compatible default of two body-font ems.
    Default,
    /// Use an exact source-authored width in twips.
    Twips(u32),
}

/// A paragraph-level `w:ind` first-line-indent override — the resolved
/// `par.first-line-indent` when it differs from the Normal style's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FirstLineIndent {
    /// `w:firstLine` amount in twips; negative values emit `w:hanging`
    /// instead (an outdent).
    pub twips: i64,
    /// `w:firstLineChars` (`em × 100`) for pure-em indents — emitted so Word
    /// measures the indent in characters rather than twentieths of a point.
    pub chars: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "mirrors the independent w:pPr toggles"
)]
pub struct Paragraph {
    /// Inline elements including text runs and footnote references.
    pub inlines: Vec<InlineElement>,
    pub style: Option<ParagraphStyle>,
    /// If this paragraph is a list item, its numbering info (level + ID).
    pub list_info: Option<ListInfo>,
    /// Heading-level numbering (`w:numPr` without the list indent): `level`
    /// is the 0-based heading level, `id` is a heading numbering instance in
    /// [`Document::heading_num_instances`].
    pub num_pr: Option<ListInfo>,
    /// Paragraph alignment (left, center, right, justify).
    pub alignment: Option<Alignment>,
    /// The paragraph's resolved `par.first-line-indent`, carried only when it
    /// differs from the Normal style's value — an explicit `w:ind` override.
    /// Negative `twips` is an outdent and emits `w:hanging` instead.
    pub first_line_indent: Option<FirstLineIndent>,
    /// The paragraph's resolved `par.justify`, carried only when it differs
    /// from the Normal style's `body_alignment` — an explicit `w:jc` override
    /// (`both` / `left`). Ignored when `alignment` already emits `w:jc`.
    pub justify: Option<bool>,
    /// Suppress first-line indent (e.g., first paragraph after a heading).
    pub suppress_indent: bool,
    /// Hanging indent for bibliography entries or source-authored paragraph rules.
    pub hanging_indent: Option<HangingIndent>,
    /// Left indent in twips (e.g., 720 for block quotes).
    pub left_indent: Option<u32>,
    /// Right indent in twips (e.g., from `#pad(right:)` or `#block(inset:)`).
    pub right_indent: Option<u32>,
    /// Keep all lines of this paragraph on the same page (`w:keepLines`).
    pub keep_lines: bool,
    /// Keep this paragraph on the same page as the next one (`w:keepNext`).
    pub keep_next: bool,
    /// Apply `CodeBlock` style (monospace, no indent, optional shading).
    pub code_block: bool,
    /// If set, this paragraph ends a section. The `w:sectPr` is emitted
    /// inside this paragraph's `w:pPr`.
    pub section_break: Option<SectionBreak>,
    /// Paragraph-mark font size in half-points, emitted as
    /// `w:pPr/w:rPr/w:sz`. Shrinks an empty paragraph's line height —
    /// used for section-break markers so they do not spill onto a new
    /// page when the section ends near the page bottom.
    pub mark_size_half_pt: Option<u32>,
    /// If true, this paragraph represents a horizontal rule (bottom border).
    pub horizontal_rule: bool,
    /// Tab stop positions in twips (e.g., for grid/multi-column recovery).
    /// Emitted as `<w:tabs><w:tab w:val="right" w:pos="..."/></w:tabs>` in `w:pPr`.
    pub tab_stops: Vec<u32>,
    /// Override the paragraph's `w:before` spacing (twips).
    /// Used to suppress heading above-spacing at the start of a page/document.
    pub spacing_before: Option<u32>,
    /// Override the paragraph's `w:after` spacing (twips).
    pub spacing_after: Option<u32>,
    /// Explicit `<w:ind w:left>` override (twips) for list-item paragraphs;
    /// `None` inherits the numbering level's indentation.
    pub list_left_twips: Option<u32>,
    /// Explicit `<w:ind w:hanging>` override (twips) for list-item paragraphs;
    /// `None` inherits the numbering level's hanging indent.
    pub list_hanging_twips: Option<u32>,
    /// Exact line height in twips: `w:spacing w:line` + `w:lineRule="exact"`.
    /// Timetable whitespace cells pin 1pt paragraphs so Word does not inflate
    /// empty rows to its minimum.
    pub line_twips: Option<u32>,
    /// Paragraph shading fill (`w:pPr/w:shd w:fill`), a six-digit hex like
    /// `Run::shading`.
    pub shading: Option<String>,
}

impl Paragraph {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_run(&mut self, text: &str) {
        let run = Run::new(text);
        self.inlines.push(InlineElement::Text(run));
    }

    /// Add a pre-built run to this paragraph.
    pub fn push_run(&mut self, run: Run) {
        self.inlines.push(InlineElement::Text(run));
    }

    /// Apply `f` to every text run in the paragraph, including hyperlink
    /// display runs and runs inside anchored text boxes.
    pub fn for_each_run_mut(&mut self, f: &mut dyn FnMut(&mut Run)) {
        for inline in &mut self.inlines {
            match inline {
                InlineElement::Text(run) => f(run),
                InlineElement::Hyperlink { runs, .. }
                | InlineElement::Field { result: runs, .. } => {
                    for run in runs {
                        f(run);
                    }
                }
                InlineElement::AnchoredFloat(float) => {
                    if let AnchoredContent::Text { blocks, .. } = &mut float.content {
                        for element in blocks {
                            for_each_paragraph_in_block_mut(element, &mut |p| {
                                p.for_each_run_mut(f);
                            });
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Immutable twin of [`Self::for_each_run_mut`].
    pub fn for_each_run(&self, f: &mut dyn FnMut(&Run)) {
        for inline in &self.inlines {
            match inline {
                InlineElement::Text(run) => f(run),
                InlineElement::Hyperlink { runs, .. }
                | InlineElement::Field { result: runs, .. } => {
                    for run in runs {
                        f(run);
                    }
                }
                InlineElement::AnchoredFloat(float) => {
                    if let AnchoredContent::Text { blocks, .. } = &float.content {
                        for element in blocks {
                            for_each_paragraph_in_block(element, &mut |p| {
                                p.for_each_run(f);
                            });
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Get concatenated text content from all text runs.
    #[must_use]
    pub fn text_content(&self) -> String {
        self.inlines
            .iter()
            .filter_map(|i| {
                if let InlineElement::Text(run) = i {
                    Some(run.text.as_str())
                } else {
                    None
                }
            })
            .collect()
    }

    #[must_use]
    pub fn full_text_content(&self) -> String {
        let mut text = String::new();
        for inline in &self.inlines {
            match inline {
                InlineElement::Text(run) => text.push_str(&run.text),
                InlineElement::Math { omml_xml, .. } => {
                    for part in omml_xml.split("<m:t>") {
                        if let Some(end) = part.find("</m:t>") {
                            text.push_str(&part[..end]);
                        }
                    }
                }
                InlineElement::Hyperlink { runs, .. }
                | InlineElement::InternalLink { runs, .. }
                | InlineElement::Field { result: runs, .. } => {
                    for run in runs {
                        text.push_str(&run.text);
                    }
                }
                InlineElement::FieldRef { display_text, .. }
                | InlineElement::FieldPageRef { display_text, .. }
                | InlineElement::FieldPage { display_text, .. }
                | InlineElement::Citation { display_text, .. } => {
                    text.push_str(display_text);
                }
                _ => {}
            }
        }
        text
    }

    /// Iterate over text runs (immutable).
    pub fn text_runs(&self) -> impl Iterator<Item = &Run> + '_ {
        self.inlines.iter().filter_map(|i| {
            if let InlineElement::Text(run) = i {
                Some(run)
            } else {
                None
            }
        })
    }

    /// Iterate over text runs (mutable).
    pub fn text_runs_mut(&mut self) -> impl Iterator<Item = &mut Run> + '_ {
        self.inlines.iter_mut().filter_map(|i| {
            if let InlineElement::Text(run) = i {
                Some(run)
            } else {
                None
            }
        })
    }

    /// Add a footnote reference to this paragraph.
    pub fn add_footnote_ref(&mut self, id: u32) {
        self.inlines.push(InlineElement::FootnoteRef(id));
    }

    /// Add a math equation (OMML XML) to this paragraph.
    pub fn add_math(&mut self, omml_xml: String) {
        self.inlines.push(InlineElement::Math {
            omml_xml,
            equation_number: None,
        });
    }

    /// Add a numbered math equation (OMML XML) to this paragraph.
    pub fn add_numbered_math(&mut self, omml_xml: String, number: String) {
        self.inlines.push(InlineElement::Math {
            omml_xml,
            equation_number: Some(EquationNumber::Text(number)),
        });
    }

    /// Add an inline image to this paragraph.
    pub fn add_image(&mut self, image: ImageData) {
        self.inlines.push(InlineElement::Image(image));
    }

    /// Add a bookmark start + end pair (anchor for cross-references).
    pub fn add_bookmark(&mut self, id: u32, name: String) {
        self.inlines.push(InlineElement::Bookmark { id, name });
        self.inlines.push(InlineElement::BookmarkEnd { id });
    }

    /// Insert a zero-length bookmark at the very start of the paragraph (a
    /// cross-reference target, e.g. a bibliography entry that citations link to).
    pub fn add_bookmark_at_start(&mut self, id: u32, name: String) {
        self.inlines.insert(0, InlineElement::BookmarkEnd { id });
        self.inlines.insert(0, InlineElement::Bookmark { id, name });
    }

    /// Add a cross-reference field (REF field code).
    pub fn add_field_ref(&mut self, bookmark_name: String, display_text: String) {
        self.inlines.push(InlineElement::FieldRef {
            bookmark_name,
            display_text,
        });
    }

    /// Add a page-number cross-reference field (PAGEREF field code).
    pub fn add_field_pageref(&mut self, bookmark_name: String, display_text: String) {
        self.inlines.push(InlineElement::FieldPageRef {
            bookmark_name,
            display_text,
        });
    }

    /// Add a current-page field (PAGE field code).
    pub fn add_field_page(&mut self, display_text: String) {
        self.inlines.push(InlineElement::FieldPage {
            display_text,
            bold: false,
            size_half_pt: None,
        });
    }

    /// Add an external hyperlink.
    pub fn add_hyperlink(&mut self, url: String, runs: Vec<Run>) {
        self.inlines.push(InlineElement::Hyperlink { url, runs });
    }

    /// Add an internal hyperlink to a bookmark `anchor` (e.g. a citation marker
    /// linking to its bibliography entry).
    pub fn add_internal_link(&mut self, anchor: String, runs: Vec<Run>) {
        self.inlines
            .push(InlineElement::InternalLink { anchor, runs });
    }

    /// Add a Table of Contents field code.
    pub fn add_toc(&mut self, max_depth: u8) {
        self.inlines.push(InlineElement::FieldToc { max_depth });
    }

    /// Add a page break.
    pub fn add_page_break(&mut self) {
        self.inlines.push(InlineElement::PageBreak);
    }

    pub fn add_column_break(&mut self) {
        self.inlines.push(InlineElement::ColumnBreak);
    }

    /// Add a tab character.
    pub fn add_tab(&mut self) {
        self.inlines.push(InlineElement::Tab);
    }

    /// Add a citation field (SDT-wrapped CITATION field code).
    pub fn add_citation(&mut self, keys: Vec<String>, display_text: String) {
        self.inlines.push(InlineElement::Citation {
            keys,
            display_text,
            anchor: None,
        });
    }
}

/// Content of a table cell: a mix of paragraphs and nested tables in order.
#[derive(Debug, Clone, PartialEq)]
pub enum CellContent {
    Paragraph(Paragraph),
    Table(Table),
}

/// A table cell containing paragraphs and optionally nested tables.
#[derive(Debug, Clone, PartialEq)]
pub struct TableCell {
    pub paragraphs: Vec<Paragraph>,
    /// Ordered cell content (paragraphs + nested tables). When non-empty, this
    /// is used instead of `paragraphs` for serialisation, allowing cells to
    /// contain nested `<w:tbl>` elements interleaved with `<w:p>` elements.
    pub content: Vec<CellContent>,
    /// Number of columns this cell spans (1 = no merge). Maps to `w:gridSpan`.
    pub colspan: u32,
    /// Vertical merge state. Maps to `w:vMerge`.
    pub vmerge: VMerge,
    /// Cell width as percentage of table width (in fiftieths of a percent, i.e. 5000 = 100%).
    /// If None, width is auto-distributed.
    pub width_pct: Option<u32>,
    /// Vertical alignment of the cell's content, read from the semantic
    /// `TableElem`. Maps to `w:vAlign`. `None` = unset (don't override Word).
    pub vertical_align: Option<VerticalAlign>,
    /// Cell background as a 6-digit uppercase hex (e.g. `"DDEEFF"`), read from
    /// the semantic `TableElem` fill. Maps to `w:shd` in `w:tcPr`, mirroring
    /// `Run.shading`. `None` = unset.
    pub shading: Option<String>,
    /// Absolute cell width in twips (`w:tcW` `dxa`); used by fixed-layout
    /// tables planned from a resolved Typst grid.
    pub width_twips: Option<u32>,
    /// Per-side cell strokes (`w:tcBorders`). `None` lets the table-level
    /// borders show through; `Some` reproduces the grid's exact stroke
    /// geometry, including `nil` sides.
    pub borders: Option<CellBorders>,
    /// Zero cell margins (`w:tcMar` 0) so empty timetable rows do not inflate.
    pub zero_margins: bool,
}

/// Per-side cell borders (`w:tcBorders`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CellBorders {
    pub top: Option<CellBorder>,
    pub left: Option<CellBorder>,
    pub bottom: Option<CellBorder>,
    pub right: Option<CellBorder>,
}

/// One cell border side: thickness in eighths of a point (`w:sz`), optional
/// hex colour, dashed or single.
#[derive(Debug, Clone, PartialEq)]
pub struct CellBorder {
    pub sz_eighths: u32,
    pub color_hex: Option<String>,
    pub dashed: bool,
}

/// Vertical alignment of a table cell's content. Maps to `w:vAlign`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalAlign {
    Top,
    Center,
    Bottom,
}

impl VerticalAlign {
    /// The `w:vAlign` attribute value.
    #[must_use]
    pub fn ooxml_value(self) -> &'static str {
        match self {
            VerticalAlign::Top => "top",
            VerticalAlign::Center => "center",
            VerticalAlign::Bottom => "bottom",
        }
    }

    #[deprecated(since = "0.2.1", note = "renamed to `ooxml_value`")]
    #[must_use]
    pub fn as_val(self) -> &'static str {
        self.ooxml_value()
    }
}

/// Vertical merge state for a table cell.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum VMerge {
    /// Not part of a vertical merge.
    #[default]
    None,
    /// Start (first cell) of a vertical merge (`w:vMerge val="restart"`).
    Restart,
    /// Continuation cell of a vertical merge (`w:vMerge` with no val).
    Continue,
}

/// A table row containing cells.
#[derive(Debug, Clone, PartialEq)]
pub struct TableRow {
    pub cells: Vec<TableCell>,
    /// Exact row height in twips (`w:trHeight` `hRule="exact"`). Timetable
    /// slot heights pin rendered row pitch instead of inflating to content.
    pub height_twips: Option<u32>,
    /// `w:tblHeader`: repeat this row atop each page the table breaks onto.
    pub is_header: bool,
}

/// Per-side table border thicknesses in eighths of a point (`None` = no border).
///
/// Captures both full grids (every side plus `inside_h`/`inside_v`) and
/// three-line tables (`top`/`bottom` plus `header_sep`, everything else `None`).
/// Derived by reading the rules actually drawn in the `PagedDocument`, so it is
/// not tied to any particular table style.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TableBorders {
    pub top: Option<u32>,
    pub bottom: Option<u32>,
    pub left: Option<u32>,
    pub right: Option<u32>,
    /// Rule between body rows.
    pub inside_h: Option<u32>,
    /// Rule between columns.
    pub inside_v: Option<u32>,
    /// Separator drawn after the header row(s); emitted as the header row's
    /// bottom border so it appears under the header only (three-line style).
    pub header_sep: Option<u32>,
    /// Number of leading header rows the separator follows (usually 1).
    pub header_rows: usize,
}

/// Default cell margins for a table (`w:tblCellMar` in `w:tblPr`), in twips.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableCellMargins {
    pub top: u32,
    pub left: u32,
    pub bottom: u32,
    pub right: u32,
}

/// A table with rows and cells.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub rows: Vec<TableRow>,
    /// Table width as percentage (in fiftieths of a percent, e.g. 5000 = 100%).
    /// `None` defaults to 5000 (100%).
    pub width_pct: Option<u32>,
    /// Border size in eighths of a point (e.g. 4 = 0.5pt).
    /// `None` defaults to 4 (0.5pt solid).
    pub border_size: Option<u32>,
    /// Per-side borders detected from the rendered geometry. When `Some`, the
    /// writer emits exactly these (so a three-line table is not boxed into a
    /// grid); when `None`, it falls back to a uniform `border_size` grid.
    pub borders: Option<TableBorders>,
    /// Cell margins applied to every cell (`w:tblCellMar`). `None` leaves the
    /// Word default (left/right 108 twips).
    pub cell_margins: Option<TableCellMargins>,
    /// Absolute table width in twips (`w:tblW` `dxa`); fixed-layout tables
    /// planned from a resolved Typst grid set this together with
    /// `col_widths_twips`.
    pub width_twips: Option<u32>,
    /// Absolute column widths in twips (`w:tblGrid` + `w:tblLayout
    /// w:type="fixed"`). `None` keeps percentage-based auto layout.
    pub col_widths_twips: Option<Vec<u32>>,
    /// Table-level alignment (`w:jc` in `w:tblPr`) — the horizontal part of
    /// an `align(..)` wrapper around the table. `None` = Word's default left.
    pub alignment: Option<Alignment>,
    /// `w:tblpPr` — the table floats at a page-absolute position instead of
    /// flowing inline (a lone `table` inside a `#place` or a
    /// `figure(placement:)`).
    pub float: Option<TableFloat>,
}

/// `w:tblpPr` — a table floated at a page-absolute position, anchored to its
/// position in the flow instead of displacing it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TableFloat {
    /// `w:tblpX` — left edge, twips from the page's left (`w:horzAnchor="page"`).
    pub x_twips: i32,
    /// `w:tblpY` — top edge, twips from the page's top (`w:vertAnchor="page"`).
    pub y_twips: i32,
    /// `place(clearance:)` mapped to `w:*FromText` distances, twips.
    pub distance_twips: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BlockElement {
    Paragraph(Paragraph),
    Table(Table),
    /// Bibliography section wrapped in SDT with BIBLIOGRAPHY field code.
    BibliographyBlock {
        paragraphs: Vec<Paragraph>,
    },
}

/// Visit every paragraph the writer serializes under a block element: plain
/// paragraphs, bibliography blocks, and table cells — both a cell's legacy
/// `paragraphs` vector and its `content` (paragraphs and nested tables,
/// recursively). This is the single point of truth for that traversal:
/// hand-rolled copies have already desynced once (walkers that skipped
/// `cell.content` silently dropped style patches on nested-table cells).
pub fn for_each_paragraph_in_block_mut(
    element: &mut BlockElement,
    f: &mut dyn FnMut(&mut Paragraph),
) {
    match element {
        BlockElement::Paragraph(p) => f(p),
        BlockElement::Table(t) => for_each_paragraph_in_table_mut(t, f),
        BlockElement::BibliographyBlock { paragraphs } => {
            for p in paragraphs {
                f(p);
            }
        }
    }
}

fn for_each_paragraph_in_table_mut(table: &mut Table, f: &mut dyn FnMut(&mut Paragraph)) {
    for row in &mut table.rows {
        for cell in &mut row.cells {
            for para in &mut cell.paragraphs {
                f(para);
            }
            for content in &mut cell.content {
                match content {
                    CellContent::Paragraph(p) => f(p),
                    CellContent::Table(nested) => for_each_paragraph_in_table_mut(nested, f),
                }
            }
        }
    }
}

/// Immutable twin of [`for_each_paragraph_in_block_mut`].
pub fn for_each_paragraph_in_block(element: &BlockElement, f: &mut dyn FnMut(&Paragraph)) {
    match element {
        BlockElement::Paragraph(p) => f(p),
        BlockElement::Table(t) => for_each_paragraph_in_table(t, f),
        BlockElement::BibliographyBlock { paragraphs } => {
            for p in paragraphs {
                f(p);
            }
        }
    }
}

fn for_each_paragraph_in_table(table: &Table, f: &mut dyn FnMut(&Paragraph)) {
    for row in &table.rows {
        for cell in &row.cells {
            for para in &cell.paragraphs {
                f(para);
            }
            for content in &cell.content {
                match content {
                    CellContent::Paragraph(p) => f(p),
                    CellContent::Table(nested) => for_each_paragraph_in_table(nested, f),
                }
            }
        }
    }
}

/// OOXML bookmark names cap at 40 chars — `w:bookmarkStart@w:name` and the
/// field instruction pointing at it must truncate identically.
#[must_use]
pub fn truncate_bookmark_name(name: &str) -> &str {
    if name.len() <= 40 {
        return name;
    }
    let mut end = 40;
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    &name[..end]
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Body {
    pub elements: Vec<BlockElement>,
}

/// Section break type for multi-section documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionBreakType {
    NextPage,
    Continuous,
    EvenPage,
    OddPage,
}

impl SectionBreakType {
    #[must_use]
    pub fn ooxml_value(self) -> &'static str {
        match self {
            Self::NextPage => "nextPage",
            Self::Continuous => "continuous",
            Self::EvenPage => "evenPage",
            Self::OddPage => "oddPage",
        }
    }
}

/// A section break that ends a section, optionally overriding page settings.
#[derive(Debug, Clone, PartialEq)]
pub struct SectionBreak {
    pub break_type: SectionBreakType,
    /// Page settings (`w:pgSz`/`w:pgMar`/`w:cols`) of the section that ends here.
    pub page_settings: Option<PageSettings>,
    /// `w:titlePg` — the section that ends here has a distinct first page.
    pub title_page: bool,
    /// `w:pgNumType w:fmt` of the section that ends here.
    pub page_numbering: Option<PageNumberFormat>,
    /// `w:pgNumType w:start` restart of the section that ends here.
    pub page_number_start: Option<u32>,
}

/// Header or footer content (a sequence of paragraphs).
#[derive(Debug, Clone, PartialEq)]
pub struct HeaderFooter {
    pub paragraphs: Vec<Paragraph>,
}

/// Which footer slots carry a generated PAGE-field footer (a bare page
/// number in the margin zone) instead of authored `footer*` content.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageNumberFooters {
    /// Default (`w:type="default"`) slot.
    pub default: bool,
    /// Even-page (`w:type="even"`) slot.
    pub even: bool,
    /// First-page (`w:type="first"`) slot.
    pub first: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PageSettings {
    pub width_twips: u32,
    pub height_twips: u32,
    pub margin_top: u32,
    pub margin_bottom: u32,
    pub margin_left: u32,
    pub margin_right: u32,
    /// Number of columns in this section (None or 1 = single column).
    pub columns: Option<u32>,
    /// Spacing between columns in twips (default 720 = 0.5 inch).
    pub column_spacing: Option<u32>,
    /// `<w:pgMar w:header>` distance (twips) — Typst `page(header-ascent:)`.
    pub header_twips: Option<u32>,
    /// `<w:pgMar w:footer>` distance (twips) — Typst `page(footer-descent:)`.
    pub footer_twips: Option<u32>,
    /// When set, emit `<w:lnNumType w:distance="…">` — Typst
    /// `par.line(numbering:)` enabled, value = `number-clearance`.
    pub line_number_distance_twips: Option<u32>,
    /// `<w:vAlign>` for the section — Typst `set align(horizon|bottom|top)`.
    pub vertical_align: Option<VerticalAlign>,
}

impl Default for PageSettings {
    fn default() -> Self {
        Self {
            width_twips: 11906,  // A4 width
            height_twips: 16838, // A4 height
            margin_top: 1440,    // 2.54cm
            margin_bottom: 1440,
            margin_left: 1800, // 3.17cm
            margin_right: 1800,
            columns: None,
            column_spacing: None,
            header_twips: None,
            footer_twips: None,
            line_number_distance_twips: None,
            vertical_align: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DocumentMetadata {
    pub title: Option<String>,
    pub author: Option<String>,
    /// ISO 8601 timestamp for creation time. If not set, a default is used.
    pub created: Option<String>,
    /// Typst `document(description:)` → `dc:description`.
    pub description: Option<String>,
    /// Typst `document(keywords:)` → `cp:keywords` (comma-joined).
    pub keywords: Option<String>,
}

impl DocumentMetadata {
    /// Return the creation timestamp string (ISO 8601 / W3CDTF format).
    /// Falls back to a fixed default if not explicitly set.
    ///
    /// NOTE: We use a fixed fallback rather than a compile-time timestamp
    /// (`env!("BUILD_TIMESTAMP")`) to avoid adding a build-script dependency
    /// and to keep builds reproducible.
    #[must_use]
    pub fn created_time(&self) -> String {
        self.created
            .clone()
            .unwrap_or_else(|| "2026-01-01T00:00:00Z".to_string())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum FootnoteFormat {
    #[default]
    Decimal,
    CircledNumber,
}

/// Footnote area layout overrides from Typst `set footnote.entry(...)`.
/// Each `None` inherits the Word default.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FootnoteEntryStyle {
    /// First-line indent (twips) applied to each footnote's first paragraph.
    /// Typst `footnote.entry(indent:)`.
    pub indent_twips: Option<u32>,
    /// Spacing above the separator line (twips). Typst `footnote.entry(clearance:)`.
    pub clearance_twips: Option<u32>,
    /// Gap between consecutive footnotes (twips) — `w:spacing w:after` on
    /// footnote paragraphs. Typst `footnote.entry(gap:)`.
    pub gap_twips: Option<u32>,
    /// The entry's declared `par.justify`, pinned as `w:jc` so a footnote
    /// does not inherit the Normal style's justification — `FootnoteText`
    /// is `w:basedOn` Normal and `#show footnote.entry: set par(justify:)`
    /// reaches no chain the entry paragraphs see.
    pub justify: Option<bool>,
    /// Separator rule thickness in eighths of a point (`w:sz`), declared via a
    /// `line(...)` separator's `stroke` thickness.
    pub separator_stroke_eighths: Option<u32>,
}

/// Page number format for footer page numbering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageNumberFormat {
    Decimal,
    LowerRoman,
    UpperRoman,
    LowerLetter,
    UpperLetter,
}

impl PageNumberFormat {
    #[must_use]
    pub fn ooxml_value(self) -> &'static str {
        match self {
            Self::Decimal => "decimal",
            Self::LowerRoman => "lowerRoman",
            Self::UpperRoman => "upperRoman",
            Self::LowerLetter => "lowerLetter",
            Self::UpperLetter => "upperLetter",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "flat bag of orthogonal document-level flags; a state machine would not model them"
)]
pub struct DocumentStyle {
    pub body_font_ascii: String,
    pub body_font_east_asia: String,
    pub body_size_half_pt: u32,
    pub line_spacing: u32,
    pub first_line_indent_twips: u32,
    /// East-Asian character-based first-line indent = `round(em × 100)`. `Some`
    /// only for em-based source indents (e.g. `2em` → `Some(200)`); `None` for
    /// absolute (pt/cm) indents, which keep emitting only `w:firstLine` twips.
    pub first_line_indent_chars: Option<u32>,
    /// Whether `#set par(first-line-indent: (amount: …, all: true))` was declared,
    /// i.e. indent EVERY paragraph including the first one after a heading (the
    /// Chinese-typography convention). `false` (the Typst default) suppresses the
    /// first-line indent on the paragraph that follows a heading.
    pub first_line_indent_all: bool,
    pub footnote_format: FootnoteFormat,
    /// Whether the footnote counter resets on every page. Typst's default is
    /// continuous numbering across the whole document; per-page restart only
    /// happens when the source resets the footnote counter in a page header
    /// (`counter(footnote).update(0)` there).
    pub footnote_restart_each_page: bool,
    /// Font used for code/raw blocks (detected from Typst rendering).
    pub code_font: String,
    /// Font size for code blocks in half-points.
    pub code_size_half_pt: u32,
    /// Font size for footnote text in half-points.
    pub footnote_size_half_pt: u32,
    /// Heading sizes in half-points, indexed by level (0=h1, 1=h2, ..., 4=h5).
    pub heading_sizes: [u32; 5],
    /// Paragraph justification (e.g., "both" for justify, "left" for left-align).
    pub body_alignment: String,
    /// Body paragraph spacing before in twips.
    pub body_spacing_before: u32,
    /// Body paragraph spacing after in twips.
    pub body_spacing_after: u32,
    /// Heading spacing before in twips, per level (0=h1 .. 4=h5).
    pub heading_spacing_before: [u32; 5],
    /// Heading spacing after in twips, per level (0=h1 .. 4=h5).
    pub heading_spacing_after: [u32; 5],
    /// BCP 47 language tag for Latin text (e.g. "en-US").
    pub lang_latin: String,
    /// BCP 47 language tag for East Asian text (e.g. "zh-CN", "ja-JP", "ko-KR").
    pub lang_east_asia: String,
    /// Whether the document contains CJK content. Controls `w:hint="eastAsia"`.
    pub has_cjk_content: bool,
    /// Hyperlink color as a 6-digit hex string (e.g. "0563C1").
    pub hyperlink_color: String,
    /// Body font's cap-height ratio (em units). Used to compute line pitch:
    /// `line_pitch = cap_height_ratio × body_size + leading`.
    pub body_cap_height_ratio: f64,
    /// `#set text(hyphenate: false)` → `w:suppressAutoHyphens` in settings.
    pub suppress_auto_hyphens: bool,
    /// `set footnote.entry(...)` overrides (indent, clearance, gap, separator).
    pub footnote_entry: FootnoteEntryStyle,
}

impl Default for DocumentStyle {
    fn default() -> Self {
        // Typst defaults: 11pt body, 0.65em leading → 18.15pt line height → 363 twips,
        // no first-line indent, h1=1.4em=31hp, h2=1.2em=26hp, h3-5=body size,
        // paragraph spacing = 1.2 * 11pt = 13.2pt → 264 twips.
        Self {
            body_font_ascii: "Times New Roman".to_string(),
            body_font_east_asia: "\u{5b8b}\u{4f53}".to_string(),
            body_size_half_pt: 22,      // Typst default: 11pt = 22 half-points
            line_spacing: 276,          // ~13.8pt: typical rendered line pitch for 11pt body
            first_line_indent_twips: 0, // Typst default: no indent
            first_line_indent_chars: None,
            first_line_indent_all: false,
            footnote_format: FootnoteFormat::default(),
            footnote_restart_each_page: false,
            code_font: "Courier New".to_string(),
            code_size_half_pt: 22, // Typst raw text uses body size by default
            footnote_size_half_pt: 18,
            // Typst defaults: h1=1.4*22=31, h2=1.2*22=26, h3-h5=body size
            heading_sizes: [31, 26, 22, 22, 22],
            body_alignment: "left".to_string(),
            // Typst default par.spacing = 1.2em; at 11pt → 13.2pt → 264 twips.
            // before=0 because Word sums before+after (no collapsing).
            body_spacing_before: 0,
            body_spacing_after: 264,
            heading_spacing_before: [0; 5],
            heading_spacing_after: [264; 5],
            lang_latin: "en-US".to_string(),
            lang_east_asia: "zh-CN".to_string(),
            has_cjk_content: true,
            hyperlink_color: "0563C1".to_string(),
            body_cap_height_ratio: 0.66,
            suppress_auto_hyphens: false,
            footnote_entry: FootnoteEntryStyle::default(),
        }
    }
}

/// One numbering level's override for a list instance: an OOXML
/// `w:lvlOverride` carrying a full `w:lvl` (numFmt + lvlText), e.g. a
/// lower-letter enum level. Levels without an override keep the shared
/// abstract definition (decimal `"%{n}."` / bullet).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelOverride {
    /// Zero-based level (`w:ilvl`).
    pub ilvl: u32,
    /// OOXML `w:numFmt` value (`"decimal"`, `"lowerLetter"`, ...).
    pub num_fmt: &'static str,
    /// OOXML `w:lvlText` (e.g. `"%1)"` for an `"a)"` enum).
    pub lvl_text: String,
}

/// A heading numbering instance: one `w:num`/`w:abstractNum` pair per
/// distinct mappable heading numbering pattern. `levels` holds the full
/// `w:lvl` definitions for ilvl 0..=8 (deeper heading levels reuse the
/// pattern's last piece, matching Typst's pattern cycling).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadingNumInstance {
    /// Unique `w:numId` for this heading numbering instance.
    pub num_id: u32,
    /// `w:abstractNumId` this instance points at (heading abstracts start at 3).
    pub abstract_num_id: u32,
    /// Per-level definitions, in ascending `ilvl` order.
    pub levels: Vec<LevelOverride>,
}

/// A dynamically allocated list numbering instance: one top-level list's
/// `w:num` entry, with its abstract numbering definition (1=ordered,
/// 2=unordered), level-0 start value, and per-level overrides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListNumInstance {
    /// Unique `w:numId` for this list.
    pub num_id: u32,
    /// `w:abstractNumId` this instance points at.
    pub abstract_num_id: u32,
    /// Level-0 start value (1 unless the author wrote `#enum(start: N)`).
    pub start: u32,
    /// Per-level overrides, in ascending `ilvl` order.
    pub level_overrides: Vec<LevelOverride>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    pub body: Body,
    pub page_settings: PageSettings,
    /// Footnotes referenced by `FootnoteRef` inline elements.
    pub footnotes: Vec<Footnote>,
    /// Document metadata (title, author) written to docProps/core.xml.
    pub metadata: DocumentMetadata,
    /// Style information extracted from the Typst document rendering.
    pub style: DocumentStyle,
    /// Counter for generating unique bookmark IDs.
    pub bookmark_counter: u32,
    /// Default (`w:type="default"`, odd/first) header (written to `word/header1.xml`).
    pub header: Option<HeaderFooter>,
    /// Even-page header (`w:type="even"`, `word/header2.xml`). Only referenced
    /// when `even_and_odd_headers` is set.
    pub header_even: Option<HeaderFooter>,
    /// First-page header (`w:type="first"`, `word/header3.xml`). Only
    /// referenced when `title_page` is set.
    pub header_first: Option<HeaderFooter>,
    /// Default footer for the document (written to `word/footer1.xml`).
    pub footer: Option<HeaderFooter>,
    /// Even-page footer (`w:type="even"`, `word/footer2.xml`).
    pub footer_even: Option<HeaderFooter>,
    /// First-page footer (`w:type="first"`, `word/footer3.xml`).
    pub footer_first: Option<HeaderFooter>,
    /// If set, footer contains a PAGE field code with this number format.
    /// When this is `Some`, a footer is always generated (even if `footer` is `None`).
    pub page_numbering: Option<PageNumberFormat>,
    /// `w:start` on the final section's `w:pgNumType` — a page-number restart
    /// declared for the last section (Typst `counter(page).update(n)`).
    pub page_number_start: Option<u32>,
    /// Whether each footer slot carries a generated PAGE field (a bare page
    /// number in the footer margin zone). Decoupled from `page_numbering`: the
    /// number format may be declared (`#set page(numbering:)`) without any
    /// footer existing.
    pub page_number_footers: PageNumberFooters,
    /// `w:evenAndOddHeaders` in settings.xml — Typst's header/footer rendered
    /// different content on odd vs even pages.
    pub even_and_odd_headers: bool,
    /// `w:titlePg` on the section that contains page 1 — Typst's header/footer
    /// differed on the first page (e.g. a `page(header:)` suppressed there).
    pub title_page: bool,
    /// Counter for generating unique list numbering IDs (starts at 4;
    /// 1=ordered, 2=unordered, 3=Chinese headings are reserved).
    pub next_list_id: u32,
    /// Dynamically allocated list numbering instances. Used to generate
    /// `w:num` entries with a `startOverride` and per-level overrides.
    pub list_num_instances: Vec<ListNumInstance>,
    /// Heading numbering instances, one per distinct numbering pattern.
    pub heading_num_instances: Vec<HeadingNumInstance>,
    /// Bibliography sources for Word's citation data store (customXml/item1.xml).
    pub citation_sources: Vec<CitationSource>,
    /// Emit `<w:updateFields w:val="true"/>` in settings.xml (C3: Word
    /// refreshes every field on open).
    pub update_fields: bool,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            body: Body::default(),
            page_settings: PageSettings::default(),
            footnotes: Vec::new(),
            metadata: DocumentMetadata::default(),
            style: DocumentStyle::default(),
            bookmark_counter: 0,
            header: None,
            header_even: None,
            header_first: None,
            footer: None,
            footer_even: None,
            footer_first: None,
            page_numbering: None,
            page_number_start: None,
            page_number_footers: PageNumberFooters::default(),
            even_and_odd_headers: false,
            title_page: false,
            next_list_id: 4,
            list_num_instances: Vec::new(),
            heading_num_instances: Vec::new(),
            citation_sources: Vec::new(),
            update_fields: false,
        }
    }
}

impl Document {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_paragraph(&mut self, para: Paragraph) {
        self.body.elements.push(BlockElement::Paragraph(para));
    }

    pub fn add_table(&mut self, table: Table) {
        self.body.elements.push(BlockElement::Table(table));
    }

    /// Allocate and return the next unique bookmark ID.
    pub fn next_bookmark_id(&mut self) -> u32 {
        let id = self.bookmark_counter;
        self.bookmark_counter += 1;
        id
    }

    /// Apply `f` to every text run the writer serializes: body paragraphs
    /// (including all table-cell content and nested tables), bibliography
    /// blocks, footnote bodies, and header/footer paragraphs. Hyperlink
    /// display runs are included.
    pub fn for_each_run_mut(&mut self, f: &mut dyn FnMut(&mut Run)) {
        for element in &mut self.body.elements {
            for_each_paragraph_in_block_mut(element, &mut |p| p.for_each_run_mut(f));
        }
        for footnote in &mut self.footnotes {
            for inline in &mut footnote.content {
                match inline {
                    InlineElement::Text(run) => f(run),
                    InlineElement::Hyperlink { runs, .. } => {
                        for run in runs {
                            f(run);
                        }
                    }
                    _ => {}
                }
            }
        }
        for hf in self.header_footers_mut() {
            for para in &mut hf.paragraphs {
                para.for_each_run_mut(f);
            }
        }
    }

    /// Mutable iterator over every header/footer part that will be serialized.
    fn header_footers_mut(&mut self) -> impl Iterator<Item = &mut HeaderFooter> {
        [
            &mut self.header,
            &mut self.header_even,
            &mut self.header_first,
            &mut self.footer,
            &mut self.footer_even,
            &mut self.footer_first,
        ]
        .into_iter()
        .flatten()
    }

    /// Immutable iterator over every header/footer part that will be serialized.
    pub fn header_footers(&self) -> impl Iterator<Item = &HeaderFooter> {
        [
            &self.header,
            &self.header_even,
            &self.header_first,
            &self.footer,
            &self.footer_even,
            &self.footer_first,
        ]
        .into_iter()
        .flatten()
    }

    /// Immutable twin of [`Self::for_each_run_mut`].
    pub fn for_each_run(&self, f: &mut dyn FnMut(&Run)) {
        for element in &self.body.elements {
            for_each_paragraph_in_block(element, &mut |p| p.for_each_run(f));
        }
        for footnote in &self.footnotes {
            for inline in &footnote.content {
                match inline {
                    InlineElement::Text(run) => f(run),
                    InlineElement::Hyperlink { runs, .. } => {
                        for run in runs {
                            f(run);
                        }
                    }
                    _ => {}
                }
            }
        }
        for hf in self.header_footers() {
            for para in &hf.paragraphs {
                para.for_each_run(f);
            }
        }
    }

    /// Allocate a unique list numbering ID for a new top-level list.
    /// `ordered` selects abstractNum 1 (decimal) or 2 (bullet); `start` is the
    /// list's first number (1 unless the author wrote `#enum(start: N)`).
    pub fn allocate_list_id(&mut self, ordered: bool, start: u32) -> u32 {
        self.allocate_list_id_with_overrides(ordered, start, Vec::new())
    }

    /// Allocate a unique list numbering ID, with per-level overrides (e.g. an
    /// enum's non-decimal numbering pattern). Overrides are in ascending
    /// `ilvl` order; levels without one keep the shared abstract definition.
    pub fn allocate_list_id_with_overrides(
        &mut self,
        ordered: bool,
        start: u32,
        level_overrides: Vec<LevelOverride>,
    ) -> u32 {
        let id = self.next_list_id;
        self.next_list_id += 1;
        let abstract_num = if ordered { 1 } else { 2 };
        self.list_num_instances.push(ListNumInstance {
            num_id: id,
            abstract_num_id: abstract_num,
            start,
            level_overrides,
        });
        id
    }

    /// Find the heading numbering instance for these level definitions, or
    /// allocate a new `w:num`/`w:abstractNum` pair for them. Returns the
    /// `w:numId` and whether a new instance was created.
    pub fn find_or_allocate_heading_numbering(&mut self, levels: &[LevelOverride]) -> (u32, bool) {
        if let Some(instance) = self
            .heading_num_instances
            .iter()
            .find(|instance| instance.levels == levels)
        {
            return (instance.num_id, false);
        }
        let num_id = self.next_list_id;
        self.next_list_id += 1;
        let abstract_num_id =
            u32::try_from(self.heading_num_instances.len() + 3).unwrap_or(u32::MAX);
        self.heading_num_instances.push(HeadingNumInstance {
            num_id,
            abstract_num_id,
            levels: levels.to_vec(),
        });
        (num_id, true)
    }

    /// Replace the per-level overrides of an already allocated list instance
    /// (the walk discovers nested-level patterns after allocation).
    pub fn set_list_overrides(&mut self, num_id: u32, level_overrides: Vec<LevelOverride>) {
        if let Some(instance) = self
            .list_num_instances
            .iter_mut()
            .find(|instance| instance.num_id == num_id)
        {
            instance.level_overrides = level_overrides;
        }
    }

    /// Add a footnote and return its ID (starting from 2, since 0 and 1 are reserved by OOXML).
    pub fn add_footnote(&mut self, content: Vec<InlineElement>) -> u32 {
        let id = u32::try_from(self.footnotes.len()).unwrap_or(u32::MAX - 3) + 2;
        self.footnotes.push(Footnote {
            id,
            content,
            bookmark: None,
        });
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_key_tracks_every_styled_field() {
        let plain = Run::new("x");
        assert!(plain.format_key().is_plain());
        assert_eq!(plain.format_key(), Run::new("y").format_key()); // text ignored

        // Every styled field must flip both equality and is_plain.
        let styled: Vec<Run> = vec![
            {
                let mut r = Run::new("x");
                r.bold = true;
                r
            },
            {
                let mut r = Run::new("x");
                r.italic = true;
                r
            },
            {
                let mut r = Run::new("x");
                r.superscript = true;
                r
            },
            {
                let mut r = Run::new("x");
                r.subscript = true;
                r
            },
            {
                let mut r = Run::new("x");
                r.monospace = true;
                r
            },
            {
                let mut r = Run::new("x");
                r.underline = true;
                r
            },
            {
                let mut r = Run::new("x");
                r.strikethrough = true;
                r
            },
            {
                let mut r = Run::new("x");
                r.overline = true;
                r
            },
            {
                let mut r = Run::new("x");
                r.highlight_color = Some("yellow".into());
                r
            },
            {
                let mut r = Run::new("x");
                r.smallcaps = true;
                r
            },
            {
                let mut r = Run::new("x");
                r.color = Some("FF0000".into());
                r
            },
            {
                let mut r = Run::new("x");
                r.font_ascii = Some("Courier New".into());
                r
            },
            {
                let mut r = Run::new("x");
                r.font_east_asia = Some("SimSun".into());
                r
            },
            {
                let mut r = Run::new("x");
                r.size_half_pt = Some(24);
                r
            },
        ];
        for r in &styled {
            assert!(!r.format_key().is_plain());
            assert_ne!(r.format_key(), plain.format_key());
        }
    }
}
