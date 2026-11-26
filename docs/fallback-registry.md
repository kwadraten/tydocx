# Fallback registry

The realize path (`convert` → `export_ir`) is the conversion path. It still
leans on the legacy tag-walker path (`convert_legacy`/`export`), on Paged
geometry, and on a small set of source-AST reads in the places listed here.
Each item states what it is and when it can be deleted. Only *live* items are
kept — resolved history lives in git.

## Legacy path as a whole

### `export()` + `convert_legacy()` + `compile_both_targets()`
- **What:** the HtmlDocument tag-walker pipeline, kept as the
  explicitly-injected fallback (`convert_legacy`) and the equivalence
  baseline (`tests/integration/ir_path.rs`).
- **Delete when:** the IR path reaches parity on the equivalence corpus
  (every fixture either byte-identical or carrying a documented,
  user-accepted diff), **and** the C1 gates are ported to the IR path —
  done. Deleting `export()` also deletes `compile_both_targets` and the
  `HtmlDocument` dependency of `tydocx-core`.
- **Owner:** user decision (merge gate), after the equivalence corpus is
  accepted.

## Post-passes the IR path does not run

These live inside `run_post_passes`, which only `export()` (the legacy path)
calls. They die with it, or are ported earlier if a fixture needs them.

### `apply_styles_from_paged`
- **What:** per-run styles and heading alignment patched from Paged frames,
  matched by span.
- **Delete when:** the IR path's runs (which already carry weight, style,
  size, color, and font from the realize `StyleChain`) cover the same
  fixtures — `issue_heading_size_equals_body`,
  `issue_underline_font_size_change`, `variable_font_weight`,
  `variable_font_style` — with no diff. IR runs carry their source span, so
  the span-matching port is possible; it is still skipped by design, and
  heading sizes are measured from the collected headings
  (`measure_heading_sizes_from_ir`).

### `apply_breaks_from_source`
- **What:** explicit `#pagebreak()`/`#colbreak()` recovered from the source
  AST for the legacy path. The IR path uses the
  `PagebreakElem`/`ColbreakElem` pairs. In single-column page flow a
  `ColbreakElem` shares the lowering's `PageFlow` state machine with page
  breaks; inside columns it remains a column break. This function serves
  `export()` only and is deleted with it.

### `merge_same_line_paragraphs`
- **What:** consecutive paragraphs on the same visual line merged, using
  Paged geometry and the HTML body children.
- **Delete when:** a pair-based equivalent exists, or the fixtures that
  exercise it (`recovery_short_*`, `issue_split_paragraph`) match without
  it. There is no HTML body in the IR path, so the port needs a new
  geometry source or a proof that the pairs never split such lines.

## Still shared with the legacy path

- **Headers/footers** come from the companion `PagedDocument`
  (`apply_headers_and_footers`) — page-margin content is not represented in
  the pairs. Extraction classifies each zone into default/even/first slots
  (`ZoneParts`); the writer emits `w:headerReference`/`w:footerReference`
  per type, `w:evenAndOddHeaders`, and `w:titlePg`.
- **Page/style extraction** fills a scratch `Document` from `PagedDocument`
  heuristics plus `StyleChain`-resolved overrides
  (`convert/realize/chain.rs`), with the source AST as a `merge_from`
  fill-in for fields no chain can carry (below).

## Remaining source-AST reads

The source AST is no longer a third authority — these are the narrow reads
that remain, each because no `StyleChain` or pair carries the value:

- **Imported-module `set` rules** — module styles do not escape the import,
  so a `set par(first-line-indent:)` inside an imported file that owns no
  content is invisible to every chain. `merge_from` in
  `apply_page_and_style_ir` keeps the reachable-file AST scan as a fill-in
  for unset fields (`issue_import_nested` / `issue_import_parent_dir`).
- **Declared font spelling** — `FontFamily::new` lowercases the name before
  it reaches the chain; OOXML and CJK `eastAsia` selection need the declared
  case (`issue_cjk_*` fixtures, golden font names). An explicit root `text_font`
  is recovered into `chain_overrides`. After lowering, `canonicalize_font_names`
  prefers the installed font book's canonical family and falls back to literal
  font declarations in the main file, run-span sources, and reachable local
  modules when the requested family is unavailable.
- **`doc.page_numbering`** — the AST `page_numbering` field is the single
  source for the document-level `w:pgNumType` (last-wins, as before). A
  deliberate single-source choice, not a blind spot: the chain sees the
  same declarations but supplies only the `numbering_changed` flag, which
  drives the C1 `page-numbering-change` degradation.
- **`show heading:` / `show outline.entry:` flags** — `c1_chain_degradations`
  keeps the narrow `extract_source_style_overrides` call to report these
  source declarations. Heading output is collected from pairs instead.
- **`counter(footnote).update(0)`** — `CounterUpdateElem::key` is private,
  so the chain cannot identify the counter;
  `detect_footnote_restart_each_page` stays.
- **`#show footnote.entry: set par(...)`** — the entry element is realized
  in the page's footnote area, so no pair's `StyleChain` carries its
  `first-line-indent`/`justify` back to the emitted footnote paragraphs.
  The show-rule's `set par` args are read from source and pinned per
  footnote paragraph (the entry would otherwise inherit Normal through
  `FootnoteText`).
- **`counter(page).at(<label>)` targets** — the label is recovered from the
  source span (`counter_page_label`), used only to classify an
  already-realized `RefElem`.
- **`context` expression classification** — a `ContextElem` pair carries the
  rendered text but not the expression that produced it, so the C3 fields
  classify the span's source (`classify_counter_use` adds
  `counter(page).final()` → ` NUMPAGES `; `counter_footnote_label`,
  `datetime_use`, `docproperty_use` → ` NOTEREF `/` DATE `/` DOCPROPERTY `).
  The cached result always comes from the pair's own realization; source
  only picks the field code.
- **Heading-query page zones** — a `page(header:/footer:)` query evaluates
  per page, so nothing in the IR keeps whether a zone's content is purely a
  heading reference; `page_zone_heading_levels` reads it from source for
  the C3 ` STYLEREF "Heading N" ` wrap in `apply_headers_and_footers`.
- **Overline span matching** — `overline.rs` uses source spans to attach
  Paged geometry to IR runs; it reads positions, not style values.

Everything else that used to be source-AST on this path — document-global
`set` values, smallcaps, hanging indent, `text(baseline:)`, transform
bodies — now comes from `StyleChain`s or element identity.

## Open gaps in the IR path

### Footnote SVG/PDF images
- **What:** the frame-based patch keeps raster footnote images but skips
  SVG/PDF images in footnote entries — there is no raster form in the
  patch. SVG footnote images inside *collected* bodies are rasterized;
  only the frame fallback still skips them.
- **Delete when:** the frame fallback rasterizes footnote SVGs through the
  same path as body images, and `issue_footnote_two_images` /
  `issue_footnote_hyperlink_image` match.

### Image sizes when Paged fails to compile
- **What:** the collector leaves image sizes at 0 and the patch fills them
  (Paged size map, then intrinsic dimensions at 96 DPI). The patch only
  runs when the Paged compile succeeded, so if Paged fails, images reach
  the writer at 0×0 EMU and render invisibly.
- **Delete when:** the intrinsic-size fallback runs unconditionally in
  `export_ir` (it needs no Paged data), or the writer falls back to
  intrinsic dimensions for a 0-size image.

### Fragment counter re-evaluation
- **What:** a `#context counter(...).display()` expression re-evaluated
  inside a fragment reads the counter's FINAL value, not the value at its
  position — fragment locations are synthetic and never match the main
  realization's introspector entries. Both theorems in
  `edge_theorem_proof` render as "Theorem 2" (the first should be
  "Theorem 1").
- **Delete when:** fragment realization evaluates context expressions
  against the main document's locations (a typst-realize-level fix), or
  the collector reads counter displays from the main pairs instead of
  re-realizing them. Until then `golden_edge_theorem_proof` is retargeted
  to the legacy path and `misc::edge_theorem_proof_content` (which only
  asserts the display equation's OMML) passes on the IR path.

### Footnote-body references
- **What:** a footnote whose body is `FootnoteBody::Reference(label)` (a
  second marker sharing another footnote's entry) has no content of its
  own, so it keeps the Paged-frame body — the shared entry's text.
- **Delete when:** the collector resolves the reference to the original
  footnote's collected body (by label, via the label map).

### Fragment-per-block cost
- **What:** the collector re-realizes a fragment for every block element
  whose body needs layout-synthesized data (tables, grids, figures,
  bibliography). No performance data exists yet.
- **Delete when:** measured. If a document with many tables/grids shows a
  material slowdown versus the legacy path, batch or cache the fragment
  calls (keyed by content span within one `create`).

### `(n)`-style numbering
- **What:** parenthesized counter numbering — `#enum(numbering: "(n)")`,
  `footnote(numbering:)` functions and similar — has no `w:numFmt` form.
  Function/parenthesized patterns fall back to literal `number_text` runs
  (enum) or decimal `w:footnoteReference` (footnote); C1 reports
  `footnote-numbering`, C2/C3 silently use the active `numFmt`.
- **C2 decision pending:** literal `(n)` marker text vs `w:numFmt` — no
  Word numFmt renders parentheses.

### Header/footer mid-document variants
- **What:** a margin zone that changes mid-document in a way the three
  OOXML slots (default/even/first) cannot express — e.g. a new header
  starting on page 3 — reports `header-footer-variant`. C1 keeps page 1's
  zone and reports any variant.
- **Delete when:** per-section `w:headerReference`/`w:footerReference` is
  emitted (each `IrSection` would carry its zone parts).

### Page `foreground:`/`background:` and `page(fill:)`
- **Current:** at C2/C3 the page-frame furniture zones (background before
  the header bracket, header/footer `ArtifactElem` brackets, foreground at
  the page origin after the body frame) are measured per page into
  `wp:anchor` drawings inside the header part — `positionH`/`positionV`
  `relativeFrom="page"`, `wps:txbx`/`txbxContent` keeping the text
  editable, `pic:pic` for images, `a:solidFill` shapes for solid rects and
  a solid `page(fill:)` (full-page extent, `behindDoc="1"`). A `#place`
  painted inside the header/footer zones anchors into that zone's own
  part instead of the body's overlay pass. The layer's page-variation
  shape picks the OOXML slot: uniform → default, first-vs-rest → `first` +
  `w:titlePg`, odd/even → `even` + `w:evenAndOddHeaders`; per-physical-page
  variation Word's three slots cannot express keeps page 1's layer on
  `first` and reports `page-foreground` (approximated). Non-solid fills,
  transformed/clipped groups (the content survives, the transform does
  not), and item kinds with no anchored representation report
  `page-foreground` remainders. C1 literalizes the furniture text as
  before.
- **Remaining gap:** mid-document per-physical-page furniture variation
  has no OOXML slot; a true fix would need per-section furniture parts
  plus `w:headerReference` sets per section.

### Absolute (non-float) `#place`
- **Current:** a measured non-float place lowers to a `wp:anchor` with
  `wp:wrapNone` (it displaces nothing) and `behindDoc` derived from paint
  order — behind when the page still has content after it. Text bodies
  become editable `wps:txbx` content with a pinned `w:jc`; images become
  `pic:pic`; a filled `rect` becomes `a:solidFill` on the shape, and an
  unfilled box takes the fill of what paints under it — the latest
  covering placed `rect`, else a declared solid `page(fill:)`, else paper
  white — so it covers its underlay as Typst's overlay implies (an image
  underlay keeps the box unfilled; so does earlier non-place content
  overlapping the box, which Word could not show through a fill). A body
  that is a lone table lowers to `w:tblpPr` at the measured position
  instead (a table cannot live inside `txbxContent`). A placed box that
  sits entirely inside a side margin anchors `relativeFrom="margin"` so a
  margin note tracks the text-area edge. An unmeasured place keeps the
  flow fallback and reports `placed-content`; C1 reports at every level.
- **Measurement:** `realize/place.rs` reads the place's
  `Tag::Start`/`Tag::End` bracket in the page frame — the items between
  the tags are laid out at their absolute positions, so their union
  bounds are the anchor's page-absolute extent, and the anchor keeps that
  true measured extent (`bodyPr wrap="none"` + `a:noAutofit` keep Word
  from re-wrapping inside it). An earlier round expanded text boxes to
  the page edge because LibreOffice ignores `wrap="none"`; in Word the
  expansion produces a page-sized invisible box, so the true extent is
  emitted and LibreOffice may show the box's text overflowing it.

### Float `scope: "parent"` and unmeasured floats
- **What:** floating `#place` anchors wherever it rendered — column
  semantics are not modeled. A `figure(scope: "parent", placement:)`
  float renders page-wide, so the measured anchor/`w:tblpPr` lands at
  that page position; the `figure_scope_parent` fixture pins this. A
  float the Paged patch cannot measure keeps the flow fallback and
  reports `placed-content` (same path at C1).

### `#rect` and other shapes
- **Current:** `realize/shape.rs` re-runs Typst's own layout math
  (`layout_shape`/`layout_line`/`layout_polygon`/`layout_curve` mirrored
  against a region of page text-area size) to resolve every bare shape to
  DrawingML geometry: `wp:inline` + `wps:wsp` with `prstGeom`
  (`rect`/`roundRect`/`round1Rect`/`round2SameRect`/`round2DiagRect`/
  `ellipse`/`line` with `a:xfrm` flips) or `a:custGeom` for
  polygons/curves/non-corner lines, `a:solidFill`/`a:noFill`, `a:ln`
  (thickness/cap/join/miter/solid colour/`custDash`), and the body in an
  editable `wps:txbx` (`bodyPr` carries the resolved inset). At C1 shapes
  still report `shape` Skipped and the body flattens into the flow.
  A `#line` with only `length` (no `end`, zero `angle`) stays a paragraph
  `w:pBdr` rule at every level.
- **Losses reported as `shape` Approximated at C2/C3** (the shape still
  draws): a non-solid fill or stroke paint (gradient/tiling/pattern) —
  the shape draws unfilled / the stroke draws black — a per-side stroke
  (the shape takes the top side's stroke), a non-zero dash `phase`, an
  `evenodd` fill rule (drawn non-zero), and a non-zero `outset` (Word's
  inline extent cannot paint outside itself). A shape whose size or path
  cannot be resolved at all keeps the old shaded-body flatten with the
  same `shape` Approximated report.
- **Deviation:** the legacy `HtmlDocument` path never sees shapes —
  typst-html registers no show rules for shape elements, so they never
  appear in the DOM — and shares no collector with the realize path.

### `inside:`/`outside:` margins
- **Current:** `margin.two_sided` is accumulated over every top-level page
  state the way `one_sided_asymmetric_margins` is — the flag is
  document-global, so any mirrored section sets it even when a later
  section re-declares margins one-sided (the chain's `margins_mirrored`
  read is last-wins and would mask it). C2/C3 emit `w:mirrorMargins` in
  `settings.xml` (the `sectPr` `pgMar` already carries left=inside,
  right=outside — the form Word reads under the flag). C1 keeps
  report-and-no-flag: `mirrored-margins` Skipped, no setting emitted — and
  it keeps the chain's last-wins read, so a document *ending* on a
  one-sided margin under-reports there (pinned quirk). A document that
  mixes inside/outside with one-sided asymmetric margins cannot be
  faithful on both halves (the flag is document-global): the flag still
  goes out and `mirrored-margins` reports the one-sided section's
  even-page swap as approximated.

### Decorated / multi-piece page-numbering patterns
- **Current:** `numbering_page_format` maps arabic/lower-roman/upper-roman/
  lower-latin/upper-latin patterns to `w:pgNumType w:fmt` per section;
  `assign_numbering_starts` emits `w:start` only when the rendered counter
  actually restarted. The restart stamps once — on the first `w:sectPr`
  the restarted section ends with; the `continuous` breaks a `#columns`
  block inserts inside that section keep `w:fmt` but do not re-initiate
  `w:start`. Decorated patterns and function numberings still
  return `None` (no `w:fmt`); C1 reports `page-numbering-change`, and
  `page-numbering-pattern` reports the unmappable declared form at every
  level.
- **`#counter(page).update(n)`** restarts with no format change are not
  detected on either path — there is no page-settings transition to split
  a section on. Realizing it needs a synthesized split keyed on the
  counter update.

### Section-break marker paragraph floor
- **What:** the `w:sectPr` that ends a section attaches to the last
  paragraph, or — when the section ends on a table/image/equation — to an
  appended marker paragraph (1pt mark, zeroed spacing). The marker cannot
  go below the renderer's minimum line box: a table ending within ~0.3mm
  of the page bottom still yields a blank page in LibreOffice — a format
  floor, not a fixable gap. Word's exact floor is unverified; the
  XML-level test is the pin in this environment.

### `show heading:` / `show outline.entry:` rules
- **Current:** reported as `show-heading-rule` /
  `show-outline-entry-rule` (C1 only). The heading tag retains its outline
  identity. All realized replacement blocks are emitted in order; the
  block produced by a re-emitted heading keeps its Heading style,
  numbering, and bookmark. Otherwise the first displayed text block can
  carry the target without imposing native heading numbering.
- **C2 target:** per-entry styled TOC entries for outline rewrites.

### `move`/`rotate`/`scale`/`skew` bodies
- **Current:** the layouter's rewritten block resolves back to the
  original element through the span index; the body is collected in place
  and `moved-content`/`rotated-content`/`scaled-content`/`skewed-content`
  report the dropped transform at every level.
- **C2 target:** `w:position` run offsets for `move(dy:)`; `wp:anchor`
  text box with `rot` for `rotate`.

### `box`/`block` sizing and decorations
- **Current:** `block-width`/`box-width`/`block-decoration`/
  `box-decoration`/`box-baseline` (all levels); body kept.
- **C2 target:** `w:ind`/fixed-width tables for widths; `w:shd`/`w:pBdr`
  for fills/strokes; `w:position` for `box(baseline:)`.

### Flexible and relative spacing
- **Current:** `h-fr-space`/`h-ratio-space`/`h-negative-space` and the
  `v-*` equivalents (all levels); fixed `h`/`v` keep their twips.
- **C2 target:** `h(1fr)` → `w:jc="distribute"` or a right-aligned tab;
  negative `v` → negative `w:spacing w:after`; ratio spaces → resolved
  values once the containing width is known.

### Text metrics and decorations
- **Current:** `text-edge-metrics`, `underline-offset`,
  `highlight-geometry` (all levels). `text(baseline:)` is fully mapped to
  `w:position`.
- **C2 target:** underline offset via run baseline or custom underline;
  highlight extents via `w:shd` padding (limited OOXML support); text
  edges need OpenType metric overrides Word does not expose — likely stays
  reported.

### Clip-derived crops (`block`/`box(clip:)`, `image(fit:)`)
- **Current:** the Paged frame walk (`collect_image_sizes`) accumulates the
  `GroupItem::clip` window enclosing each `FrameItem::Image` — the frame
  mechanism both `clip:` and `fit:` route through — and compares the visible
  intersection to the image's own bounds. The difference becomes `a:srcRect`
  l/t/r/b (thousandths of a percent) on `ImageData.src_rect`; the full bytes
  stay embedded and the clip window is the `wp:extent`. Nested clips
  intersect, so `fit: "cover"` inside a clipped block composes. A clip
  cannot propagate through a group with a non-identity transform — those
  images keep their full bounds.
- **Limitation:** placements are keyed by content hash and first occurrence
  wins — two copies of the same bytes (e.g. an image used cropped and
  uncropped) share the first placement found on the page.
- **Remaining:** `fit: "contain"` letterboxes — it needs extent math, not
  `srcRect`, and is still dropped silently at C2/C3 (`+ "image-fit"` at C1);
  `alt:` → `wp:docPr descr` is written at C2/C3, still dropped at C1
  (`+ "image-alt"`).

### Grid lowering limits
- **What:** a grid wider than 63 lanes is split at lane boundaries into
  consecutive tables + `grid-column-split`; `fr`/`auto` row heights are
  measured from one laid-out frame — the renderer's heights, not Word's
  line layout — and only rows the planner rewrites carry `trHeight` (a
  grid it leaves unchanged reflows in Word like an ordinary table). C1
  flattens grids to paragraphs + `grid-layout`.

### Silent-at-C2 degradations
- **What:** several aspects report only at C1 even though C2/C3 also drop
  them: `outline(target:)` / `outline(indent:)` (title-only / default
  indent), custom `footnote(numbering:)` (falls back to the active
  `numFmt`), two-sided margins, bare shapes, `figure` explicit placement,
  `image(fit:/alt:)`.
  See `docs/spec-ecma376.md` for the per-feature rows.

### Reversed enums still on `w:numPr`
- **What:** a reversed enum whose numerals the collector rendered (every
  item carries `number_text`) is lowered as literal text at every compat
  level — the faithful `N … 1` order `w:numPr` cannot express — and
  reports `enum-reversed` (literalized). Two cases still take the
  `w:numPr` path and number forward: a reversed enum nested under a
  numbering parent (`inherited` allocation) and one whose items lack
  `number_text` (an unmappable pattern like `numbering: "①"`, or a
  numbering function that failed to apply). Both still report
  `enum-reversed`.

### Dual `CompatMode` / `Degradation` definitions
- **What:** `tydocx-core` (`compat.rs`) and `tydocx-ir` (`mode.rs`,
  `degradation.rs`) define identical `CompatMode`/`DegradationAction`/
  `Degradation` types, converted at the boundary in `realize/mod.rs`
  (`ir_compat`, `core_degradation`). Not a correctness issue — the
  conversion is exhaustive and pinned — but duplication worth one cleanup
  change.
- **Unify when:** a follow-up moves the canonical definitions into
  `tydocx-ir` and has core re-export them (or vice versa).

### C3 field layer
- **What landed:** `--compat c3` emits `w:updateFields`, ` NUMPAGES `
  (`counter(page).final()`), ` NOTEREF <label> \h ` (`counter(footnote)`
  references to labelled footnotes — the footnote carries a matching
  `w:bookmarkStart`), ` DATE ` (`datetime.today()`), ` DOCPROPERTY `
  (`document.title`/`author`/`keywords`/`description`/`date` →
  Title/Author/Keywords/Comments/CreateDate, `\@` picture switches
  translated from the source display pattern), ` SEQ ` for single-piece
  figure/table caption numbers and equation numbering (the guard is the
  rendered numeral's arity) and for flat literal enums (` SEQ enum-N `),
  ` STYLEREF "Heading N" ` for page zones containing a heading query —
  the query-result span is spliced out when literal markup surrounds it,
  in headers and footers —
  and ` CITATION <key> \l 1033 ` for `@key`/`#cite` (D2 decided — the
  marker stays literal at C1/C2). `customXml/item1.xml` carries the
  hayagriva sources at every level. `#link(url)` was already a
  `w:fldSimple` ` HYPERLINK ` field at all levels, so the field form was
  already in place. Every field's cached result is the C2 literal,
  pinned corpus-wide by `fields::c3_cached_results_reproduce_c2_text`
  (D1); a field body the cache cannot carry losslessly falls back to the
  literal runs.
- **Registered remainder:**
  - ` NUMPAGES ` after a page-number restart — the field refreshes to the
    *physical* page count while `counter(page).final()` is the last page's
    *logical* number. When they differ the cached result stays the Typst
    value and `page-counter-final` reports the mismatch (C3 only — C1/C2
    emit no field and stay literal under `page-counter-value`).
  - `SECTIONPAGES` — Typst has no per-section total-page construct
    (`counter(page).final()` is always whole-document), so there is no
    source expression to feed it.
  - `TIME` — `datetime.now()` does not exist in Typst 0.15.
  - `SEQ` for nested or reversed literal enums, per-item `number:`
    overrides, and rendered numerals with genuinely multiple pieces (no
    probed path produces one) — stays literal +
    `enum-seq-nested`/`enum-seq-override`/`enum-seq-numeral`/
    `enum-reversed`/`seq-multi-piece`; native `w:numPr`
    numbering must not regress.
  - ` NOTEREF ` for references whose label
    never lands on a `FootnoteElem` (stays literal +
    `footnote-counter-reference`, same as C2).

## Numbering invariants

Constraints the `numbering.xml` writer already satisfies — keep them true:

- **abstractId-scoped counters.** OOXML counters are scoped to the
  `w:abstractNumId`; every top-level list gets a distinct `w:num` with a
  level-0 `w:startOverride` so lists never continue each other.
- **`startOverride` scoping.** Overrides live inside
  `w:num → w:lvlOverride(w:ilvl=0)`, never on the shared `w:abstractNum`.
- **`lvlRestart` absent on purpose.** OOXML's default restarts a level
  whenever ANY higher level increments — exactly Typst's enum semantics.
- **Every `w:lvl` declares `w:start`**, so an item first appearing at a
  deeper `w:ilvl` has a defined ancestor start.
- **Markers hang, not firstLine.** Numbered paragraphs use
  `w:ind w:left + w:hanging`.

## Oracle limitations

Named limitations the conformance harness records (not defects):

- **`libreoffice-math` absent.** LibreOffice without the Math component
  imports OMML as nothing — the `m:*` markup is well-formed yet renders
  no formula even in PDF export. Round-trip tests exempt `m:*` classes and
  `m:t` text; the emitted OMML is pinned by non-oracle tests.
- **LibreOffice rewrites, not drops.** On docx→docx conversion it rewrites
  `w:fldSimple` hyperlinks into `w:hyperlink` + external relationship,
  normalizes `w:tblBorders` to per-cell `w:tcBorders` (`w:left`/`w:right`
  to `w:start`/`w:end`), and elides empty `w:footnotePr`. The round-trip
  test accepts each rewrite only when the equivalent form is present.
