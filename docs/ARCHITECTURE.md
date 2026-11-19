# typort Architecture

This document explains how typort turns a Typst document into an editable Word
file. It is written for newcomers to the codebase. For build and contribution
rules, see [`AGENTS.md`](../AGENTS.md) in the repository root.

## The problem

A `.typ` file is a *program*. Running it produces a laid-out document. Converting
that to a `.docx` is hard for one reason: **the two formats disagree about what a
document *is***.

- Word is **semantic and reflowable**: a heading is a heading (style `Heading 1`),
  a footnote is a native footnote, a table is a grid of cells, an equation is an
  editable OMML object. There are no "pages" in the content model — Word paginates
  on its own.
- A rendered Typst document is **geometric**: glyphs placed at coordinates on
  fixed-size pages. By the time it is laid out, "this is a heading" has decayed
  into "these glyphs are 22pt and bold."

A naive converter renders Typst to pages (or PDF) and reconstructs Word from the
geometry. That path is lossy — headings become bold text, footnotes become
page-bottom paragraphs, cross-references break. typort exists to avoid exactly
that. (See the competitor analysis in the project notes: PDF-intermediate tools
are fundamentally lossy.)

## The core idea: read the document from three sources at once

typort compiles the **same source** more than once, because no single
representation carries everything Word needs. There are **three** information
sources, in order of authority:

| Source | Produced by | Carries | Used for |
|--------|-------------|---------|----------|
| **Realize pairs** | `typst::compile::<DocxDocument>` (`Target::Paged`, via `typst-realize`) | element identity + document order + the `StyleChain` that applies to each element + an *introspector* | the skeleton *and* the paint: what each thing *is*, what order it comes in, and how it is formatted |
| **Paged geometry** | `typst::compile::<PagedDocument>` (companion compile) | footnote entry bodies, on-page image display sizes, page dimensions, headers/footers, page numbering | what the pairs cannot carry: footnote bodies, image sizes, page-level facts |
| **Source AST** | re-parsing the `.typ` text (and imports) for `set` rules | authoritative declared values (`set text(font: ...)`, margins, columns) | overriding heuristic guesses with what the author literally wrote |

The mental model:

> **The realize pairs are the skeleton and the paint** (semantics + order +
> styles, straight from the evaluator). **Paged supplies only what the pairs
> cannot** (footnote entry bodies, image display sizes, page-level facts).
> **The source AST corrects both when the author stated a value
> explicitly.**

The legacy `HtmlDocument` tag-walker path is kept as `convert_legacy` /
`export` — the explicitly-injected fallback and the equivalence baseline
for the realize path (see `docs/fallback-registry.md`). It is not the
conversion path anymore.

### Why realization replaced HTML

The legacy path walked `HtmlDocument`'s tag sequence. Typst's HTML export
deliberately omits visual information and drops content that has no
semantic HTML representation:

- **No styling values.** `<h1>` tells you "heading"; it does *not* tell you the
  font, the exact point size, the RGB color, or the alignment. Those exist only in
  the rendered frames. The legacy path reverse-engineered them from Paged geometry
  (`convert/page/style.rs` and `convert/page/run_style.rs`). The realize pairs
  carry the `StyleChain` with each element, so the styles are read directly —
  never inferred back from rendered glyphs.
- **Layout-only constructs vanish.** `#align(center)[...]`, `#place(...)`, some
  `#grid(...)` layouts, and `#line()` rules have no HTML element, so they are
  *absent from the DOM*. The legacy path recovered them by diffing rendered text
  lines against the model (`convert/recovery/`). In the realize path, elements
  that only exist after layout (a table's synthesized `grid`, a figure body's
  caption) are resolved by **fragment re-realization**: the enclosing body is
  realized a second time as a `Fragment` with the same engine and a locator
  derived from the main one, and the fragment's tags carry the synthesized
  element (`docs/spike-realize.md`, verification loop route (a)).
- **Page-level facts don't exist before pagination.** Page size, margins, columns,
  headers/footers (which live in the page-margin zones), page breaks, and page
  numbering only come into being once the document is laid out — i.e. only from
  Paged. The realize path still takes these from the companion `PagedDocument`
  (route (b)), as an explicit patch step (`convert/realize/patch.rs`).
- **Math is MathML, not OMML.** The HTML target (typst 0.15) renders equations to
  **MathML** `<math>` elements — semantic, but not what Word edits. Both paths
  pull the original `EquationElem` Content tree through the **introspector** and
  convert it to OMML (`typort-math`); the HTML walk skips the `<math>` node so
  the equation's glyphs aren't re-emitted as duplicate literal text
  (`convert/inline_walk.rs`, `"math"` arm), and the realize collector
  likewise emits only the OMML, not the equation's rendered text pairs.
  MathML→OMML transliteration is a possible
  future alternative, but the Content tree is the richer source today.

### Why Paged alone is not enough

The rendered document has lost the semantics. A heading is just large bold
glyphs; a footnote is just text near the bottom of a page; a table is just lines
and text boxes. Building Word from geometry alone is the lossy PDF→Word path we
are explicitly avoiding. The realize pairs are what let typort emit **native** Word headings,
footnotes, and cross-references.

### Why the source AST is a third source

Heuristics from geometry are a *fallback*. When the author wrote
`#set text(font: ("Times New Roman", "SimSun"))`, that declaration is more
trustworthy than counting glyphs. `convert/page/source_ast.rs::extract_source_style_overrides`
re-parses the main source **and its imports** (template `lib.typ` files often hide
`set` rules inside functions) and these values override the Paged-derived guesses.

## The pipeline

`typort_core::convert::convert(world, &options) -> Result<ConvertOutcome, Vec<SourceDiagnostic>>`
is the heart of the system. It is a thin wrapper: it compiles the realize
document and a companion `PagedDocument` (`convert_ir`) and hands them to
`export_ir`, which lowers the collected IR. `export_ir(docx_doc, paged_doc, world, &options) -> ConvertOutcome`
is the backend-shaped entry point — callers that already hold compiled
documents (a host compiler, a test) use it instead of `convert`. The outcome
pairs the document with its degradation report (`ConvertOutcome { document, degradations }`);
the compat mode rides in `ConvertOptions` (see `docs/compat-modes.md`). Compilation failures keep
their spans: they are returned as `Vec<SourceDiagnostic>`, never as plain
strings. The steps are grouped here by purpose; see the numbered comments in
`crates/typort-core/src/convert/realize/mod.rs` for the exact order.

```mermaid
flowchart TD
    INPUT["input.typ"] --> WORLD["TyportWorld (World trait: source, fonts, packages, Feature::Html)"]
    WORLD -->|compile::&lt;DocxDocument&gt; — realize pairs + collect IR| COMPILE["convert_ir(): thin wrapper<br/>1 compile DocxDocument + companion PagedDocument"]
    WORLD -->|compile::&lt;PagedDocument&gt; — geometry + frames| COMPILE
    COMPILE --> EXPORT["export_ir(docx_doc, paged_doc, world, &options)<br/>2 page + style from Paged + AST overrides (scratch Document)<br/>3 patch: footnote bodies + image sizes from Paged frames<br/>4 pure lowering: IrDocument → typort_ooxml::Document<br/>5 post-passes (paragraph formatting, source hanging indent, CJK fonts, smallcaps, run coalescing)"]
    EXPORT --> IR["typort_ooxml::Document"]
    IR --> WRITER["writer → XML parts → ZIP"]
    WRITER --> OUT["output.docx"]
```

Two design properties worth knowing:

- **Graceful degradation.** Paged compilation is treated as optional
  (`paged_result.output.ok()`). Every Paged-dependent step is guarded by
  `if let Some(paged)`, so if only the realize compile succeeds,
  conversion still produces a document — footnotes keep their markers
  literally, but images keep their 0 size (the intrinsic-size fallback
  lives in the patch, which needs Paged; see
  `docs/fallback-registry.md`).
- **The pairs drive order.** The realize pair walk (`collect_document`)
  defines document order. Paged data is matched back onto it — by
  `Location` within the one Paged compile (footnote entry groups, keyed
  by their parent marker location), or by content hash (image display
  sizes). `Location` is process-local and is never compared across
  compiles or persisted. Image bytes need no matching: they travel in
  the pairs' `ImageElem` content.

The legacy path (`convert_legacy` → `export`) is the same shape with a
different skeleton: it compiles `HtmlDocument` + `PagedDocument` and walks
the HTML tags (`walk_tags`) through pipeline phases 2–5 (page + style,
HTML body walk, headers/footers, recovery and post-processing). It stays
as the fallback and the equivalence baseline; its recovery passes are
deliberately **not** run by the realize path (see
`docs/fallback-registry.md` for what that costs and when each piece can
be ported or deleted).

## Crate layout

```mermaid
flowchart TD
    CRATES["crates/"] --> CLI["typort-cli/ — Thin clap binary: parse args → convert → write .docx"]
    CRATES --> CORE["typort-core/ — Typst compilation + the conversion engine (the brain)"]
    CRATES --> IR["typort-ir/ — The owned realize IR (IrDocument) + its pure lowering to typort_ooxml::Document"]
    CRATES --> OOXML["typort-ooxml/ — The OOXML document model (IR) + XML writer + ZIP packaging"]
    CRATES --> MATH["typort-math/ — Typst math Content tree → OMML"]
```

`typort-ooxml/src/lib.rs` is the crate's public API index: it contains only module
declarations and re-exports. Its writer tests live in `typort-ooxml/tests/`, where
they exercise that public API as integration tests.

### typort-ooxml writer internals

| File | Responsibility |
|------|----------------|
| `writer/mod.rs` | Package entry point (`write_docx`), write context, and inventory of optional parts/relationships. |
| `writer/package.rs` | ZIP/package plumbing, content types, relationships, settings, core properties, and shared XML helpers. |
| `writer/document.rs` | Main document body and section properties. |
| `writer/paragraph.rs`, `writer/run.rs`, `writer/table.rs` | WordprocessingML paragraphs, inline runs, and tables. |
| `writer/numbering.rs`, `writer/footnotes.rs`, `writer/fields.rs` | Numbering definitions, footnote parts/references, and Word fields/bookmarks. |
| `writer/math.rs`, `writer/image.rs`, `writer/citation.rs` | OMML passthrough, DrawingML images/media inventory, and citation/bibliography XML. |
| `writer/header_footer.rs` | Header and footer parts, including automatic page numbering. |

### typort-core internals

| File | Responsibility |
|------|----------------|
| `world.rs` | `TyportWorld`: implements Typst's `World` trait (source, system fonts, `@preview` package download, `Feature::Html`). |
| `convert/mod.rs` | Entry orchestration: `convert` (the realize path, thin wrapper over `convert_ir`) and `convert_legacy`/`export` (the legacy HtmlDocument pipeline, kept as fallback and equivalence baseline), module declarations/re-exports, `WalkCtx`, equation state, and the legacy pipeline phases. |
| `convert/realize/mod.rs` | The realize path: `DocxDocument: Output` (`Target::Paged`, collects the IR during `create`), `with_fragment` (fragment re-realization), `convert_ir`/`export_ir` (page/style via a scratch `Document`, patch, pure lowering, path-independent post-passes), and the core↔ir `CompatMode`/`Degradation` boundary conversions. |
| `convert/realize/collect.rs` | Pair → `IrDocument` collection: the tag-group walk, label/bibliography pass, footnote markers, and fragment-resolved tables/grids/figures. |
| `convert/realize/inline.rs` | Pair → `IrInline` collection: runs from `StyleChain`, links, refs, cites, images, math, breaks. |
| `convert/realize/style.rs` | `StyleChain` → `Run` conversion (weight/style/size/color/font, smart quotes, run-merge key). |
| `convert/realize/patch.rs` | Companion-Paged patch: footnote entry bodies from frame groups and image display sizes by content hash (with the intrinsic fallback). |
| `convert/realize/place.rs` | Companion-Paged patch: `#place(.., float: true)` geometry — matches each float's frame group (keyed by the place's `Location` in `FrameParent`) to a page-absolute `IrAnchor` for `wp:anchor` emission. |
| `convert/block.rs` | Block-level HTML tag walk and dispatch, paragraphs, equations, figures, code blocks, blockquotes, and term lists. |
| `convert/inline_walk.rs` | HTML inline traversal: formatted spans, links, citations, footnotes, images, and inline equations. |
| `convert/headings.rs` | Semantic heading conversion, numbering, smart quotes, and inline heading content. |
| `convert/tables.rs` | HTML table conversion, cells and nested tables, plus rowspan post-processing. |
| `convert/lists.rs` | Ordered and unordered HTML list conversion, including nested levels. |
| `convert/source.rs` | Gathers and applies authoritative source-AST style and paragraph overrides. |
| `convert/smallcaps.rs` | Recovers consumed `smallcaps` calls and aliases from the source AST. |
| `convert/postprocess.rs` | Paragraph-indent cleanup and document metadata extraction. |
| `convert/dom.rs` | Shared HTML DOM, tag, attribute, text, location, and alignment helpers. |
| `convert/page/mod.rs` | Paged-style facade: declares responsibility modules and re-exports their caller-facing API. |
| `convert/page/units.rs`, `convert/page/style.rs` | Word-unit conversion and document-style detection from rendered frames. |
| `convert/page/source_ast.rs`, `convert/page/hanging_indent.rs`, `convert/page/reachable.rs` | Authoritative source-AST `set`-rule parsing (including scoped hanging indents) and reachable local-source collection. |
| `convert/page/sections.rs`, `convert/page/margin.rs` | Section changes, page settings, body/margin zones, headers, footers, and page numbering from Paged geometry. |
| `convert/page/run_style.rs`, `convert/page/language.rs` | Per-run rendered-style overrides, measured per-level heading sizes, redundant heading-property suppression, BCP-47 helpers, and localized CJK font names. |
| `convert/recovery/mod.rs` | Orchestrates recovery of layout-only content HTML dropped and re-exports the recovery passes. |
| `convert/recovery/lines.rs` | Extracts positioned rendered lines and run/x-cluster data from paged frames. |
| `convert/recovery/deduplication.rs` | Builds the emitted-text corpus and decides which rendered lines are already represented. |
| `convert/recovery/insertion.rs` | Places genuinely missing rendered lines into the document model. |
| `convert/recovery/horizontal_rules.rs` | Recovers source-declared horizontal rules and maps emitted elements to paged locations. |
| `convert/recovery/table_rules.rs` | Derives each table's border style from its own paged tag bracket. |
| `convert/text_norm.rs` | Shared walk/recovery normalisation for visual markers, CJK spacing/fragments, math italics, whitespace, and citation markers. |
| `convert/coalesce.rs` | Final pass: merges same-line paragraphs, coalesces adjacent runs with identical effective `rPr`, and folds whitespace-only runs. |
| `convert/table_width.rs` | Turns a `TableElem`'s declared column `TrackSizings` (fr/rel/auto) into per-cell `w:tcW` percentages. |
| `convert/table_align.rs` | Faithful table-cell alignment from the semantic `TableElem`: horizontal → cell-paragraph `w:jc`, vertical → `w:vAlign`. |
| `convert/breaks.rs` | Explicit `#pagebreak()`/`#colbreak()` recovery from the source AST (both are consumed at compile time), positioned by run spans, expanding local function calls, and following `#include` chains. |
| `convert/bibliography.rs` | Citation data via the semantic `BibliographyElem` (+ re-parsing `.bib`/`.yml` with hayagriva). Takes `&dyn Introspector`, so both conversion paths share it (legacy: HtmlDocument's introspector; realize: DocxDocument's). |
| `convert/footnote.rs` | Footnote bodies from the HTML `doc-endnotes` section. |
| `convert/image.rs` | Image content decoded from each `<img>`'s base64 src data-URL (typst-html 0.15 embeds the bytes in the DOM); Paged frames contribute display sizes (by content hash) and drawing-canvas SVG+raster pairs keyed by their figure's `Location`. |
| `convert/inline.rs` | Typst Content-AST inline formatting (bold/italic/…) → styled runs. |

## The IR: `typort_ooxml::Document`

This is the typed contract between the two halves of the system. `typort-core`
**writes** it; `typort-ooxml::writer` **reads** it; it is the only thing they
share.

- `Document` — root: body elements, footnotes, page settings, document style,
  header/footer, page numbering, citation sources, metadata.
- `BlockElement` — `Paragraph | Table | BibliographyBlock` (the bibliography
  section, wrapped in an SDT carrying a `BIBLIOGRAPHY` field code).
- `Paragraph` — inline runs + optional `ParagraphStyle` + alignment/indent/spacing.
- `InlineElement` — `Text(Run)`, images, footnote refs, breaks, fields/bookmarks.
- `Run` — a styled text span (bold/italic/underline/color/font/size/script).
- `Table` / `TableRow` / `TableCell` — colspan/rowspan, shading, borders,
  multi-paragraph cells.
- `ParagraphStyle` — a **typed enum** (`Heading(n)`, `Normal`, `Quote`, `Code`, …)
  rather than stringly-typed style IDs.

Units are stored in **Word-native form** (half-points for size, twips for
spacing/indent) so the writer does minimal conversion.

## The realize IR: `typort_ir::IrDocument`

Between the realize pairs and `typort_ooxml::Document` sits a second,
owned IR — the **Styled Semantic IR** (`crates/typort-ir`). The pairs
borrow the realize arenas and cannot outlive `create()`, so the collector
copies them into owned data at once: `IrDocument { info, page, style,
body, footnotes, citation_sources }`, with `IrBlock` (14 variants:
Paragraph, Heading, Table, Grid, Figure, List, Enum, Equation, Quote,
Raw, Outline, Bibliography, Place, PageBreak) and `IrInline` (12
variants: Text, Link, Ref, Cite, FootnoteMarker, LineBreak, Space, Image,
Math, PageBreak, ColumnBreak, Tab). Strong/Emph/Super/Sub are **run
attributes**, not inline variants — the Paged show rules already merged
them into `TextElem` by the time the pairs exist, so the IR records them
where the writer reads them (bold/italic/superscript/subscript on `Run`).

Rules the equivalence round established (see
`crates/typort-cli/tests/integration/ir_path.rs` for the rulings):

- **Alignment** comes from the pair's `StyleChain` (`AlignElem` in
  scope); justification is document-level (the Normal style's
  `body_alignment`), never per-paragraph.
- **Table borders** are mapped from the fragment grid
  (`table_borders_from_grid`): explicit lines map per side with a header
  separator; with no explicit lines, any cell stroke gives the writer's
  uniform grid and no strokes give nil borders. **Column widths** are
  fiftieths of a percent (5000 = 100%), summed over spanned tracks
  without rescaling.
- **Grids** lower to fixed-layout tables through a pure planner
  (`typort-ir::grid_plan`): occupancy is derived from cell `x`/spans,
  empty source rows run-length-merge into single rows with summed
  `w:trHeight`, identical adjacent columns merge into `w:gridSpan`
  lanes, and `rowspan`s become `w:vMerge` (a continuation keeps the
  restart's `w:gridSpan`, so a spanning merge never splits into
  per-lane cells). Strokes resolve per boundary segment the way Typst
  folds them — an explicit line wins, a declared cell edge beats an
  undeclared neighbour, ties go to the inner side — and the winning
  side alone emits the `w:tcBorder`, so whitespace and continuation
  cells never paint borders they do not own. `auto` row heights are
  measured per row (`typst_layout::layout_frame` on the row's cell
  bodies at their resolved widths) instead of equal-sharing the
  fragment total; empty slots get zero cell margins and a 1pt
  paragraph mark.
- **Drawn figure bodies** attach their Paged canvas by `Location` hash
  and embed as SVG (`typst_svg`) plus a PNG fallback (`typst_render`)
  inside `mc:AlternateContent`; source SVG images keep their bytes the
  same way. C1 keeps the raster only.
- **Heading sizes** are measured from the collected headings' own runs
  (glyph-weighted dominant size per level); the rank-based histogram is
  only the fallback for levels with no sized run.
- **Citations** arrive in fragments as `RefElem` tags, never `CiteGroup`;
  a ref target that is not a document label is a bibliography key, and
  the collector emits `IrInline::Cite` with the full literal marker text
  (`"[1]"`, `"[1], [2]"`) and `target: None` until bibliography entries
  exist. Citations are literal markers plus internal links, never
  `CITATION` fields.
- **Footnote markers** in a fragment are single-pass, so an unresolved
  marker reads `"0"` and is replaced by the ordinal; entries are matched
  by `Location` first, then by ordinal order. Footnote entry text is
  excluded from the body-size/font histograms, so it cannot misdetect
  the Normal style.
- **Runs carry their source span** (from the pair content), so the
  source-AST post-passes (hanging indent, small caps) locate IR
  paragraphs. **Term lists** are collected as paragraphs: tight items
  are one paragraph (bold term + separator + description), wide items a
  term paragraph plus description blocks.
- **Footnote bodies** are collected from the `FootnoteElem.body` content
  at the marker (the tag group carries only the marker), giving real
  OMML, hyperlinks, and images; the Paged-frame body is the fallback.
  Display equations in a footnote body stay inline `Math` (their OMML
  already carries the `<m:oMathPara>` wrapper). Circled footnote
  numbering comes from the elements' numbering patterns.
- **Page references** (`counter(page).at(<label>)`) are detected in
  fragments from a `ContextElem` tag, the label recovered from the
  source AST via the main source threaded into the Collector, and
  emitted as `IrInline::Ref { page_ref: true }` (PAGEREF field).
- **Body defaults** are voted from the leading portion of the document
  only (9,000 glyphs, the pairs-level analogue of the legacy first-3-
  pages rule), so back matter cannot misdetect the body size or font.
- **Inline images** arriving as tags in fragments are collected
  (`ImageElem` branch in `handle_tag_start`); **block elements nested
  in sequence children** are flattened out of the pending inline run
  (`flatten_sequences`), so a display equation inside a `#body`
  substitution is dispatched as a block.

The lowering (`typort_ir::lower(ir, geometry, compat) -> (Document,
Vec<Degradation>)`) is a **pure function**: no `World`, no `Engine`, no
`PagedDocument` — geometry arrives as the explicit `GeometryInput {
page_size, page_numbering, supplement, header (_even/_first), footer
(_even/_first), even_and_odd_headers, title_page }` struct. That
purity is what makes the lowering unit-testable (`crates/typort-ir` has
its own lib tests) and what lets the two conversion paths share the
writer without sharing a walk.

## The writer: direct OOXML, no library

`typort-ooxml::writer` emits the `.docx` XML parts directly via `quick-xml` — **no
`docx-rs`, no intermediate format**. This is a deliberate choice: Word is strict
about WML child-element ordering (`rPr`/`pPr` children must appear in schema
order), and direct emission gives full control. Its root owns package orchestration
and shared write state; part-specific modules emit `document.xml`, `styles.xml`,
`numbering.xml`, `settings.xml`, relationships, content types, headers/footers,
footnotes, citations, math, and media before the entry point zips them.

## Math → OMML

`typort-math` converts the Typst math Content tree (obtained *semantically* via
the introspector, not from rendered glyphs) into OMML, which keeps equations
**editable** in Word. It implements a broad set of OMML constructs: fractions
(`m:f`), scripts (`m:sSub`/`m:sSup`/`m:sSubSup`), pre-scripts (`m:sPre`), radicals
(`m:rad`), n-ary operators (`m:nary`), delimiters (`m:d`), matrices (`m:m`),
aligned equations (`m:eqArr`), accents (`m:acc`), bars (`m:bar`), group characters
/ over-under braces (`m:groupChr`), named functions (`m:func`), limits
(`m:limLow`/`m:limUpp`), phantoms (`m:phant`), and boxes (`m:box`). Math **style
wrappers** (e.g. `bold`, `bb`, `cal`, `upright`, `dif`) are resolved to their
styled Unicode glyphs via the `codex` crate Typst itself uses (`bb(R)` → ℝ,
`bold(e)` → 𝒆), with an explicit `m:sty="p"` forcing upright for `upright`/`dif`.

An n-ary operator's operand (the integrand/summand) is bound into its `<m:e>`
body rather than left as detached siblings: Typst stores the operand as flat
content following the operator, so the sequence walker consumes following items
into the operand until a **Relation-class** symbol (`=`, `<`, `→`, …) — using the
same `unicode-math-class` table Typst uses, so the boundary matches Typst's own
classification (e.g. in `sum_i a_i = S`, the `= S` stays outside the n-ary).

**Hard limits** (OMML / Word constraints, not bugs):

- No color inside math zones.
- No extensible arrows.
- Word forces the Cambria Math font in math zones.

## The fragile seam: `convert/recovery/`

The honest part of this document. The recovery layer is where typort does, in
miniature, the very PDF→Word inference it set out to avoid — because some content
(`#align(center)`, `#place`, grids) reaches the model *only* as geometry.

`recovery/mod.rs::recover_missing_content` orchestrates this seam:
`recovery/lines.rs` extracts every rendered text line with its position,
`recovery/deduplication.rs` **text-diffs** it against what already made it into
the model, and `recovery/insertion.rs` inserts genuinely-missing lines at the
geometrically-correct slot. `recovery/horizontal_rules.rs` and
`recovery/table_rules.rs` contain the two rule-shape paths. Correctness depends on:

- **Text normalization matching** across two very different pipelines. The shared
  normalisers in `convert/text_norm.rs` strip CJK spaces, math italics, visual
  markers, whitespace, citation markers, and identify CJK fragments.
- **Geometry and text thresholds** collected as documented constants with their
  owning responsibility under `convert/page/` and at the top of the responsible
  `convert/recovery/*.rs` module. Those constants are the authoritative tuning
  references for the paged-style and recovery heuristics.

These heuristics are individually justified but collectively brittle: both false
negatives (real content skipped) and false positives (content duplicated) are
possible. Most of the project's known edge-case bugs live here. **Anyone touching
this directory should add a fixture-based regression test for the specific case.**

The realize path does not run recovery at all — it reads semantics from the
pairs instead of reconstructing them from geometry. What that costs (and
when each recovery piece can be ported or deleted) is recorded in
`docs/fallback-registry.md`.

## Language neutrality (how it stays universal)

typort's stated goal is a **universal** Typst→Word converter — it must not bake
in assumptions about a document's language or genre. Earlier versions did, in
three places; all now derive from semantics instead, and they stand as the
pattern to follow:

1. **Bibliographies** are detected via the semantic `doc-bibliography` role from
   `#bibliography(...)`, not by matching a `参考文献`/`References` heading. A
   hand-written reference list is, to Typst, ordinary text — and is converted as
   such.
2. **Figure/table captions** are deduplicated during recovery against the text
   the semantic figure path already emitted, not by matching `表 `/`图 `/`Table `/
   `Figure ` prefixes.
3. **Document language** (`w:lang`) is derived from `#set text(lang:, region:)`,
   not guessed from the presence of CJK glyphs.

If a future change reintroduces `if text.contains("<word>")` to drive layout,
it regresses this principle. See `AGENTS.md`.
