# Fallback registry (step 2)

The realize path (`convert` → `convert_ir`) still leans on the legacy
tag-walker path and on Paged geometry in the places listed here. Each
item states what it is, when it can be deleted, and who owns the decision.
Nothing in this file is deleted now — this is the "when can it go" record,
not a deletion list.

## Legacy path as a whole

### `export()` + `convert_legacy()` + `compile_both_targets()`
- **What:** the HtmlDocument tag-walker pipeline, kept as the
  explicitly-injected fallback (`convert_legacy`) and the equivalence
  baseline.
- **Delete when:** the IR path reaches parity on the equivalence corpus
  (`tests/integration/ir_path.rs`: every fixture either byte-identical
  or carrying a documented, user-accepted diff), **and** the C1 gates are
  ported to the IR path (see below). Deleting `export()` also deletes
  `compile_both_targets` and the `HtmlDocument` dependency of
  `typort-core`.
- **Owner:** user decision (merge gate), after the independent test agent
  accepts the equivalence corpus.

### C1-gate port
- **What:** the C1 compat gates are ported to the IR path. The
  collector records gate metadata on the IR (`Run.overline`,
  `IrList.custom_marker`, `IrEnum.reversed`, `IrTable.gutter`,
  `IrBlock::Figure::custom_placement`, `IrBlock::Outline::custom_target`/
  `custom_indent`, `IrImage::vector`/`non_cover_fit`/`alt`/`span`,
  `IrBlock`/`IrInline::Shape`); the lowering emits the C1 degradations
  (`overline`, `list-marker`, `enum-numbering`, `enum-reversed`,
  `table-cell-fill`, `table-gutter`, `grid-layout`, `figure-placement`,
  `outline-target`, `outline-indent`, `vector-image`, `image-fit`,
  `image-alt`, `shape`, `footnote-numbering`) and the corresponding
  skip/fallback behavior. `section-break` is emitted in `export_ir` from
  the collected `ir.sections` (StyleChain `PageSnapshot`s), no longer from
  `detect_page_setting_change`. The gate tests run against
  `convert()` — the IR path — unchanged.
- **Owner:** C1 Phase 1 agent, then user review.

## Post-passes the IR path deliberately skips

### Recovery passes (`recover_missing_content`) — DELETED (step 4)
- **What it was:** geometry-driven recovery of content the HTML walk missed
  (`#align(center)` blocks and similar): `lines.rs` extracted rendered
  lines, `deduplication.rs` text-diffed them against the model,
  `insertion.rs` re-inserted the missing ones.
- **Deleted when (met):** the IR path emits the same content natively from
  the pairs — `collect_standalone`/`collect_layouter_block` (`#align`,
  `#place`, `#block`), `shape_parts` (`#rect` text), `transform_parts`
  (`#rotate`/`#move`/`#scale` bodies), grids → `IrTable`, context/layout
  output — so recovery only ever produced *duplicates* of what the IR path
  already had. The `recovery_*`/`edge_recovery_*` fixtures' assertions all
  hold on `convert()`; `convert()` output is byte-identical before/after
  the deletion across the fixture corpus. What the legacy baseline lost is
  enumerated per fixture in `~/workspace/devin-step4-summary.md`; rulings
  are IR-correct or old-path bug throughout. The one case where the scrape
  held text the IR path genuinely lacks today is
  `issue_nested_table_alignment` (an `align(center)`-wrapped table nested
  in a table cell is dropped by collection — pre-existing IR gap,
  unchanged by the deletion).
- **Went with it:** `page/sections.rs` (section detection — see below),
  `stats::proportional_index{,_rounded}`, `units::pt_to_eighth_pt`,
  `Paragraph::page_from_paged`, and the recovery half of `text_norm.rs`
  (the shared `strip_cjk_spaces` stays; `frames.rs` stays — footnote
  groups and run-style extraction still consume it).

### Three-line-table borders (`detect_three_line_tables`) — DELETED (step 4)
- **What it was:** table borders inferred from the rules actually drawn in
  the Paged frames (three-line vs grid tables).
- **Deleted when (met):** the IR path derives the same borders from the
  realized `CellGrid` strokes (`table_borders_from_grid` in
  `realize/collect.rs`: explicit `hline`/`vline` map per side with a header
  separator, else cell strokes decide, else nil). Verified on
  `edge_three_line_table`, `mixed_table_borders`,
  `issue_table_after_set_in_block`, `issue_table_header_border_override`,
  `table_cell_math`: the IR path emits `w:val="nil"` for borderless tables
  and thick top/bottom rules + `w:tblHeader` for three-line tables; the
  post-deletion legacy path emits the writer's uniform grid for all of
  them (expected baseline loss). The `mixed_table_borders` recovered
  duplicate paragraph the old ruling pinned is gone with recovery.

### Section breaks from Paged (`detect_section_breaks`,
`apply_section_breaks`, `build_element_page_map`) — DELETED (step 4)
- **What it was:** section breaks inferred from page-setting changes across
  Paged pages, placed via an element→page map that recovery built.
- **Deleted when (met):** the IR path emits section breaks from its own
  collected sections — `ir.sections` (the `PageSnapshot` chains) lowered by
  `lower_body`/`attach_section_break` in `typort-ir/lower.rs` into
  mid-body `w:sectPr`s, plus `PageBreak { to }` for odd/even breaks.
  Verified: `section_break_test` (2 `w:sectPr`), `edge_multi_section` (3),
  `issue_final_section_landscape` (2), `issue_margin_change_section` (2),
  `issue_page_numbering_change` (2, with `w:pgNumType w:start`),
  `issue_table_first_section`/`issue_table_cascade_sections` (table-ending
  sections keep their boundary via the marker paragraph), and the C1
  `section-break` degradation on `issue_c1_section_columns`. The
  legacy-targeted tests were retargeted to `convert()` in
  `tests/integration/structure.rs`.
- **Known IR gap (pre-existing):** `#set page(flipped: …)` changes are not
  in the `PageSnapshot` chain, so `edge_landscape_pages` gets no
  `w:orient="landscape"` sections on either path now. The fixture's test
  keeps the heading assertions and drops the orientation assertions until
  the gap is closed.

### `apply_styles_from_paged`
- **What:** per-run styles and heading alignment patched from Paged
  frames, matched by span.
- **Delete when:** the IR path's runs (which already carry weight, style,
  size, color, and font from the realize `StyleChain`) cover the same
  fixtures — `issue_heading_size_equals_body`,
  `issue_underline_font_size_change`, `variable_font_weight`,
  `variable_font_style` — with no diff. IR runs now carry their source
  span (threaded from the pair content since the equivalence round), so
  the span-matching port is possible again — but it is still skipped by
  design, and the heading sizes the legacy measured from Paged runs are
  now measured from the collected headings instead
  (`measure_heading_sizes_from_ir`).
- **Owner:** step-2 follow-up.

### `apply_breaks_from_source` (pagebreak/colbreak recovery)
- **What:** explicit `#pagebreak()`/`#colbreak()` recovered from the
  source AST when the compiled output does not surface them.
- **Status (step 3):** the IR path never called this — all `pagebreak_*`
  fixtures (weak/strong breaks, inactive and bound-false branches,
  called and unused functions, `#include` chains, consecutive and
  odd/even breaks) pass through the `PagebreakElem`/`ColbreakElem` pairs
  and the `FreshPage` state machine in the lowering. The function now
  serves the legacy path only; it is deleted when `export()` goes.
- **Owner:** step-2 follow-up.

### Same-line paragraph merging (`merge_same_line_paragraphs`)
- **What:** consecutive paragraphs on the same visual line merged, using
  Paged geometry and the HTML body children.
- **Delete when:** a pair-based equivalent exists, or the fixtures that
  exercise it (`recovery_short_*`, `issue_split_paragraph`) match without
  it. There is no HTML body in the IR path, so the port needs a new
  geometry source or a proof that the pairs never split such lines.
- **Owner:** step-2 follow-up.

## Page/style extraction still shared with the legacy path

### `apply_page_and_style` / `apply_headers_and_footers` (temp `Document`)
- **Status (step 3):** `export_ir` no longer runs the legacy three-source
  extraction. A scratch `Document` still carries `page`/`style`/header/footer
  into the IR, but it is filled by `PagedDocument` heuristics plus
  `StyleChain`-resolved overrides (`convert/realize/chain.rs`), with the
  source AST only as a `merge_from` fill-in for fields no chain can carry
  (see "Remaining source-AST reads" below).
- **Remaining:** headers/footers still come from the companion
  `PagedDocument` (`apply_headers_and_footers`) — page-margin content is
  not represented in the pairs.
- **Owner:** step-2 follow-up.

## Remaining source-AST reads on the IR path (step-3 exceptions)

The source AST is no longer a third authority — these are the narrow reads
that remain, each because no `StyleChain` or pair carries the value:

- **Imported-module `set` rules** — module styles do not escape the
  import, so a `set par(first-line-indent:)` inside an imported file that
  owns no content is invisible to every chain. `merge_from` in
  `apply_page_and_style_ir` keeps the reachable-file AST scan as a
  fill-in for unset fields (`issue_import_nested` /
  `issue_import_parent_dir` fixtures).
- **Declared font spelling** — `FontFamily::new` lowercases the name
  before it reaches the chain; OOXML and CJK `eastAsia` selection need
  the declared case (`issue_cjk_*` fixtures, golden font names).
  `text_font` is left unset by `chain_overrides` and filled by the merge.
- **`doc.page_numbering`** — the AST `page_numbering` field is the single
  source for the document-level `w:pgNumType` (last-wins, as before). A
  deliberate single-source choice, not a blind spot: the chain sees the
  same declarations but supplies only the `numbering_changed` flag (more
  than one distinct ambient-chain value), which drives the C1
  `page-numbering-change` degradation.
- **`show heading:` / `show outline.entry:` flags** — whether the rule
  re-emitted `it` is unobservable in the pairs; `c1_chain_degradations`
  keeps the narrow `extract_source_style_overrides` call for those two
  flags.
- **`counter(footnote).update(0)`** — `CounterUpdateElem::key` is private
  in typst 0.15.1, so the chain cannot identify the counter;
  `detect_footnote_restart_each_page` stays.
- **`counter(page).at(<label>)` targets** — the label is recovered from
  the source span (`page_ref.rs::counter_page_label`), used only to
  classify an already-realized `RefElem`.
- **Overline span matching** — `overline.rs` uses source spans to attach
  Paged geometry to IR runs; it reads positions, not style values.

Everything else that used to be source-AST on this path — document-global
`set` values, smallcaps, hanging indent, `text(baseline:)`, transform
bodies — now comes from `StyleChain`s or element identity.

## Known gaps in the IR path itself

### Bibliography entries
- **What:** the IR path renders a bibliography as an empty
  `<w:bibliography/>` field: the fragment realization of a
  `BibliographyElem` yields only its heading text — the entry grid is
  built by the show rule into the Paged/Html realization, never into
  fragment pairs. Citations already render as literal markers
  (`IrInline::Cite`, target `None`), and the lowering is correct for a
  populated entry list; only the entry *extraction* is missing.
- **Delete when:** entries are extracted from the companion Paged frames
  (the same source family as footnote bodies: recurse into frame groups
  for the bibliography entry blocks) or from the HtmlDocument, and
  `bibliography_basic` / `bibliography_multiple` carry entries. Note the
  anchors will still differ by design (IR bookmarks `bib-n`; legacy
  `loc_N` from Html `li` ids), so these fixtures stay in
  `DOCUMENTED_DIFFS` even then.
- **Owner:** step-2 follow-up.

### Drawn / vector figure bodies — resolved (batch 9, L4 Phase 2d)
- A figure whose realized body is a drawing canvas is matched to its
  Paged frame by element `Location` hash (`attach_figure_canvases` in
  `realize/patch.rs`) and emitted as an `IrImage` carrying both an SVG
  part (`typst_svg` serialization of the laid-out frame) and a PNG
  fallback (`typst_render` at 150 DPI). The writer embeds it through
  `mc:AlternateContent` (`asvg:svgBlip` Choice + raster `a:blip`
  Fallback); SVG `#image()` bodies keep their source bytes the same
  way. Canvases attach only when the collected figure body holds no
  image already, so a text/table figure can never steal one.
  `edge_figure_rasterized` and `figure_kinds_no_steal` run on the IR
  path again. C1 keeps the raster-only form plus the `vector-image`
  degradation; a figure whose canvas cannot be matched still reports
  `figure-body-empty`.

### Body-font misdetection on code-dominated documents
- **What:** the body-font detector votes by glyph count, so a document
  whose code glyphs dominate (e.g. `style_code_blocks`: 1381 code
  glyphs vs 27 body glyphs) misdetects Normal as the monospace face.
  The legacy path compensates per-run with explicit body-font overrides,
  so its body text renders correctly; the IR path has no such overrides
  and its body text inherits the mono face.
- **Delete when:** monospace-flagged faces are excluded from the body-font
  vote (with a mono fallback for genuinely mono documents), and
  `style_code_blocks` matches. `tech_doc` shares the misdetection but
  both paths agree there byte-for-byte, so it is not a regression.
- **Owner:** step-2 follow-up.

### Footnote SVG/PDF images
- **What:** the frame-based patch keeps raster footnote images
  (PNG/JPEG, with frame display size) but skips SVG/PDF images in
  footnote entries — there is no raster form in the patch, and the main
  image path's SVG rasterization is not wired into it. Since the
  footnote-body collection (see "Resolved" below), SVG footnote images
  in collected bodies ARE rasterized (`image_inline` → `rasterize_svg`);
  only the frame fallback still skips them.
- **Delete when:** the frame fallback rasterizes footnote SVGs through
  the same path as body images, and `issue_footnote_two_images` /
  `issue_footnote_hyperlink_image` match.
- **Owner:** step-2 follow-up.

### Image sizes when Paged fails to compile
- **What:** the collector leaves image sizes at 0 and the patch fills
  them (Paged size map, then intrinsic dimensions at 96 DPI). The patch
  only runs when the Paged compile succeeded, so if Paged fails, images
  reach the writer at 0×0 EMU and render invisibly.
- **Delete when:** the intrinsic-size fallback runs unconditionally in
  `export_ir` (it needs no Paged data), or the writer falls back to
  intrinsic dimensions for a 0-size image.
- **Owner:** step-2 follow-up.

### Fragment counter re-evaluation
- **What:** a `#context counter(...).display()` expression re-evaluated
  inside a fragment reads the counter's FINAL value, not the value at
  its position — fragment locations are synthetic and never match the
  main realization's introspector entries. Both theorems in
  `edge_theorem_proof` render as "Theorem 2" (the first should be
  "Theorem 1"). Rooting the fragment at a real span (the first child's)
  was tried and did not change the result, and was reverted.
- **Delete when:** fragment realization evaluates context expressions
  against the main document's locations (a typst-realize-level fix), or
  the collector reads counter displays from the main pairs instead of
  re-realizing them. Until then `golden_edge_theorem_proof` is retargeted
  to the legacy path — regenerating it would bake the wrong numbers into
  the snapshot — and `misc::edge_theorem_proof_content` (which only
  asserts the display equation's OMML) passes on the IR path.
- **Owner:** step-2 follow-up (typst-realize semantics).

### Footnote-body references
- **What:** a footnote whose body is `FootnoteBody::Reference(label)`
  (a second marker sharing another footnote's entry) has no content of
  its own, so the collector leaves it to the Paged-frame body. The frame
  body for such a marker is the shared entry's text.
- **Delete when:** the collector resolves the reference to the original
  footnote's collected body (by label, via the label map).
- **Owner:** step-2 follow-up.

### Fragment-per-block cost
- **What:** the collector re-realizes a fragment for every block element
  whose body needs layout-synthesized data (tables, grids, figures).
  There is no performance data on this yet; correctness was done first.
- **Delete when:** measured. If a document with many tables/grids shows
  a material slowdown versus the legacy path, the fragment calls need
  batching or caching (keyed by content span within one `create`).
  Measure on the manuscript-scale fixture before deciding.
- **Owner:** step-2 follow-up (measurement), then user decision.

### U26 `(n)`-style numbering
- **What:** the frame shape of `(n)`-style numbering (equation numbers
  in parentheses, and similar parenthesized counters) was never probed.
- **Delete when:** probed and handled — either the pairs already carry
  the number in the expected form, or the collector/lowering gains a
  case for it. See the pagination audit (C1 downgrade: no OOXML
  equivalent for some forms).
- **Owner:** C1 follow-up.

## Manuscript-case C2 targets (Phase 2)

These are the C2 forms the Phase 2 work should implement; each is currently
covered by the C1 degradation named below (integration tests live in
`sections.rs` / `structure.rs` / `tables.rs` / `footnotes.rs`).

### Odd/even running headers (M7, T-01)
- **Done (batch 8):** margin-zone extraction classifies each zone into
  default/even/first slots (`ZoneParts`); the writer emits
  `w:headerReference`/`w:footerReference` for `default`, `even`, and `first`
  types, `w:evenAndOddHeaders` in `settings.xml`, and `w:titlePg` for
  first-page suppression (the first-page part is only emitted when it has
  content of its own — a suppressed slot stays empty).
- **Remaining degradation:** a zone that changes mid-document in a way the
  three OOXML slots cannot express (e.g. a new header starting on page 3)
  still reports `header-footer-variant`; C1 keeps page 1's zone and reports
  any variant.

### Page `foreground:`/`background:` (U13, T-02)
- **Current:** `page_furniture` is detected from the source AST; margin-zone
  content reaches the header part but is not anchored, and positioned
  foreground outside the margin zone is dropped. C1 reports
  `page-foreground`.
- **C2 target:** `wp:anchor` (not `wp:inline`) inside the header part,
  `positionH`/`positionV` relative to page, `txbxContent` carrying the text.

### Absolute `#place` (U1, T-03)
- **Current:** `IrBlock::Place` lowers to normal paragraphs; C1 reports
  `placed-content` (approximated, text kept).
- **C2 target:** `wp:anchor` with page-relative `positionH`/`positionV`
  offsets, text in `txbxContent`.

### Floating `#place` (M3)
- **Done (batch 10):** `IrBlock::Place` carries the float's wrap mode,
  `clearance` in EMU, and the `Location` hash; the companion-Paged patch
  `realize/place.rs::patch_floats` measures each rendered float group
  (its `FrameParent` carries the place's `Location`) into a page-absolute
  `IrAnchor`. The lowering emits an `InlineElement::AnchoredFloat` on a
  1pt-mark carrier paragraph, so logical/document order is unchanged
  while the drawing renders where Typst placed it. The writer emits
  `wp:anchor` with `relativeFrom="page"` on both axes, `wrapTopAndBottom`,
  and `distT`/`distB` from `place(clearance:)`; a lone image body anchors
  as `pic:pic` (raster + the existing SVG `mc:AlternateContent`), any
  other body as a `wps:txbx` text box holding real lowered blocks.
  `scope: "parent"` floats are anchored wherever they rendered — column
  semantics are not modeled.
- **Remaining degradation:** a float the patch cannot measure (no
  matching paged group) keeps the flow fallback and reports
  `placed-content`; C1 takes the same path. Non-float `#place` is
  unchanged (U1/T-03 above).

### Monospace detection (upstream #26)
- **Done (batch 10):** `run_from_styles` no longer matches `"mono"`/
  `"courier"` substrings in the declared family name. The collector
  builds `StyleDefaults::monospace_families` from the font book's
  `FontFlags::MONOSPACE` (excluding the sampled body font, so a monospace
  body face is not rewritten to the code font), matching the legacy
  path's flag-based `detect_code_font`. Runs in a flag-verified mono
  family lower with `run.monospace` → the `code_font` `w:rFonts`.

### `#rect` / drawn color bands (M5, T-04)
- **Current:** `IrBlock::Shape`/`IrInline::Shape` drops the drawing; its
  text body is kept as paragraphs; C1 reports `shape`.
- **C2 target:** a full-width paragraph shading (`w:shd`/`w:pBdr`) or a
  VML/DrawingML rectangle; body text in the shaded paragraph.

### Mid-document margin/column changes (M6, T-06)
- **Done (batch 8):** the collector resolves a `PageSnapshot` (page size,
  all four margins, columns, header/footer distances, numbering)
  from each top-level pair's `StyleChain` and pushes an `IrSection` per
  transition; synthesized `PagebreakElem` pairs are skipped since their
  chains may not carry top-level `set page` rules. The lowering attaches the
  ending `w:sectPr` to the previous section's last paragraph — or, when the
  section ends on a non-paragraph block (table, image, equation), to an
  empty marker paragraph appended after it (1pt paragraph mark, zeroed
  spacing and an exact-zero line box so it almost never spills to a blank
  page at the page-bottom edge) — and the writer emits each
  section's real `w:pgSz`/`w:pgMar`/`w:cols`. `margin_amount` maps `0pt` to
  `Some(0)` — a declared zero distinct from unset.
- **Remaining degradation:** C1 still collapses to a single section and
  reports `section-break` from `detect_page_setting_change`; the legacy
  path likewise keeps one section (the divergence is pinned in
  `ir_path.rs`). The section-break marker paragraph cannot go below the
  renderer's minimum line box for a paragraph mark: a table ending within
  ~0.3mm of the page bottom before a section break (e.g. the 21mm-bottom
  stress shape) still yields a blank page in LibreOffice — a format floor,
  not a fixable gap; Word's exact floor for this encoding is unverified.
  In LibreOffice the bare marker's height is ~0.4mm and bare and
  collapsed markers flip at the same bottom-margin boundary, so the
  collapse is unobservable there: the ignored
  `visual::visual_table_section_marker_no_blank_page` passes pre-fix in
  that renderer and is a weak guard only — it would catch a regression
  that makes the marker grossly taller (e.g. a run with text) but cannot
  catch the loss of the 1pt mark itself. The mark's outcome pin is
  Word-only (a 1pt paragraph mark is standard Word practice — Word
  renders empty paragraphs at the mark's font size — but unverified
  here); the XML-level test is the pin in this environment.
  A `#counter(page).update(n)` restart with no format change
  is not yet detected in either path and gets no `w:start` — there is no
  page-settings transition to split a section on; realizing it needs a
  synthesized split keyed on the counter update, deferred as pre-existing
  in both paths.

### `inside:`/`outside:` margins (U31, T-07)
- **Current:** keys parse to `margins_mirrored`; C1 reports
  `mirrored-margins`, sides keep the Typst defaults.
- **C2 target:** `w:mirrorMargins` in `settings.xml`; `sectPr` `pgMar`
  left=inside, right=outside (Word swaps on even pages).

### Page-numbering format change + restart (U30, T-08)
- **Done (batch 8):** `numbering_page_format` maps the resolved
  `Numbering::Pattern` (arabic/lower-roman/upper-roman/lower-latin/
  upper-latin) to `w:pgNumType w:fmt` per section;
  `assign_numbering_starts` compares the rendered page numbers across the
  format change and emits `w:start` only when the counter actually
  restarted. `detect_multi_format_page_numbering` keeps the footer `PAGE`
  field across a format change.
- **Remaining degradation:** decorated/multi-piece numbering patterns and
  function numberings still return `None` (no `w:fmt`); C1 reports
  `page-numbering-change` and emits no `pgNumType`.

### Header/footer run weight (L3, T-22)
- **Done (batch 8):** margin-zone extraction keeps per-run weight
  (`Run.bold` from the glyph's effective weight) alongside size; header,
  footer, and styled `PAGE`-field runs emit `<w:b/>`. The
  `header-footer-style` degradation is removed — size and weight both
  round-trip now.

### Function footnote numbering `(n)` (U26, T-09)
- **Current:** `footnote-numbering` degradation (literalized); markers fall
  back to decimal `w:footnoteReference`.
- **C2 target:** pending design decision — literal `(n)` marker text (the
  L2-style literal fallback) vs `w:numFmt`; no Word numFmt renders
  parentheses.

## Universe-case C2 targets (Phase 2)

Universe template-survey cases whose C1 form is a degradation plus literal
text. Integration tests live in `crates/typort-cli/tests/integration/universe.rs`
(`universe_*` fixtures).

### `show heading:` rewrite rules (V-01)
- **Current:** `show-heading-rule` (approximated); a rule that does not return
  `it` loses the Heading style and outline entry, text is kept.
- **C2 target:** realize the show-rule output and detect a re-emitted
  `HeadingElem` in it (Typst's own `#show heading: it => it` keeps identity);
  a full rewrite stays degraded.

### `show outline.entry:` rewrite rules (V-04)
- **Current:** `show-outline-entry-rule` (approximated); Word's TOC field
  uses default entry formatting.
- **C2 target:** per-entry `w:hyperlink`/styled TOC entries reproducing the
  rewritten entry formatting.

### `move`/`rotate`/`scale`/`skew` bodies (V-10)
- **Current:** on the IR path the realized layouter resolves back to the
  original `MoveElem`/`RotateElem`/`ScaleElem`/`SkewElem` through the span
  index (`collect_layouter_block`), the body is collected in place, and
  `moved-content`/`rotated-content`/`scaled-content`/`skewed-content`
  report the dropped transform — no source-AST read. (The legacy path
  still uses `transform_bodies_text`.)
- **C2 target:** `w:position` run offsets for `move(dy:)` and — for
  `rotate` — a `wp:anchor` text box with `rot` transform.

### `box`/`block` sizing and decorations (V-21, V-26)
- **Current:** `block-width`/`box-width`/`block-decoration`/
  `box-decoration` (skipped); `box(baseline:)` reports `box-baseline`.
- **C2 target:** `w:ind`/fixed-width tables for block widths; paragraph
  shading `w:shd` and `w:pBdr` for fills/strokes; `w:position` for
  `box(baseline:)` (the run property exists, no IR source sets it yet).

### Flexible and relative spacing (V-22, V-23)
- **Current:** `h-fr-space`/`h-ratio-space`/`h-negative-space` and the
  `v-*` equivalents (skipped); fixed `h`/`v` keep their twips.
- **C2 target:** `h(1fr)` → `w:jc="distribute"` or a right-aligned tab;
  negative `v` → negative `w:spacing w:after`; ratio spaces → resolved
  values once the containing width is known.

### Text metrics and decorations (V-27, V-30, V-31)
- **Current:** `highlight-geometry`, `underline-offset`,
  `text-edge-metrics`, `box-baseline` (skipped). `text(baseline:)` is the
  one fully implemented path here: it is a ghost property, but `text(...)`
  calls wrap their body in `styled_with_map`, so the declared value reaches
  the fragment `StyleChain` — `run_from_styles` resolves
  `TextElem::baseline` to `position_half_pt`, emitted as `w:position` by the
  run writer (the AST post-pass `apply_baseline_shifts_from_source` remains
  on the legacy path only). `box(baseline:)` is degrade-only.
- **C2 target:** `w:u w:val="single"` already carries the underline — the
  offset needs `w:position`-style run baseline or a custom underline;
  highlight extents map to `w:shd` padding (limited OOXML support);
  text edges need OpenType metric overrides Word does not expose —
  likely stays C1.

### `footnote.entry` spacing and separator (V-19)
- **Current:** fully mapped, no degradation — `footnote.entry` clearance
  → `w:spacing w:before` on the separator footnote, separator
  `line(stroke:)` → `w:pBdr` bottom border, gap → `w:spacing w:after` on
  footnote bodies, indent → `w:ind w:firstLine`. This direct mapping is
  the intended C1 form; the plan's fallback branch is not needed.
- **C2 target:** none outstanding.

### Image `fit:` and `alt:` (V-28)
- **Current:** `image-fit`, `image-alt` (skipped); the image is embedded
  at its display size.
- **C2 target:** `alt` → `wp:docPr descr`; non-cover fit → `srcRect`
  cropping or `a:stretch` adjustments.

## Retargeted tests (step 2)

These tests pin legacy-only behavior and run against `convert_legacy`
via the `*_legacy` helpers in `crates/typort-cli/tests/common/mod.rs`
(`fixture_document_legacy`, `fixture_doc_xml_legacy`,
`check_golden_legacy`). Each carries a comment citing its gap. Un-retarget
one when its gap is fixed and the test passes on the IR path.

- Bibliography entries (gap above): `bibliography_entries_are_not_a_bulleted_list`,
  `citations_link_to_their_bibliography_entry`,
  `multiple_bibliographies_render_two_blocks`,
  `superscript_citation_style_raises_the_marker`,
  `golden_bibliography_basic` (regenerating would bake the empty
  bibliography field into the snapshot).
- Fragment counter re-evaluation (gap above):
  `golden_edge_theorem_proof`.

Step 4 un-retargeted the recovery/section tests back to `convert()`:
`edge_bordered_blocks_text_preserved` and `issue_rotate_content_recovered`
(the `#rect`/`#rotate` gaps are closed — `shape_parts`/`transform_parts`
collect them), and the five section-break tests
(`section_break_produces_multiple_sect_pr`,
`section_break_document_model_has_section_break`,
`edge_multi_section_different_page_sizes`,
`issue_final_section_landscape_sections`,
`edge_landscape_pages_orientation` — the last keeps only its heading
assertions pending the `flipped` gap above). Deleted outright:
`grid_multi_column_has_tab_stops` and `grid_document_model_has_tab_inlines`
— their assertions pinned recovery-scraped M4 tab-stop paragraphs with no
IR equivalent (grids are tables on the IR path; coverage lives in
`grid_content_recovered_in_output` and the `grid_test` block of
`ir_path_documented_diffs`).

Regenerated on the IR path (the IR output is correct per the rulings in
`ir_path.rs`, so the snapshots were rewritten with `UPDATE_SNAPSHOTS=1`
and the diffs reviewed): `golden_aligned_equations` (display-equation
centering), `golden_edge_term_list` (tight term lists are one paragraph
per item), `golden_formatted_footnote` (stale snapshot expected a
pre-cleanup redundant size override).

## Resolved — no longer a fallback

### M9 overline (batch 7)
- Typst renders `#overline` as real `Geometry::Line` shapes inside an
  `OverlineElem` tag bracket in the paged frame. `realize/overline.rs`
  measures each bracket (segment extents, stroke, and the offset from the
  text line's top) and injects `IrInline::Overline(AnchoredLine)` before
  the first covered run, matched by span containment with an order+text
  fallback; the covered runs' `overline` flag is cleared. C2/C3 lower it
  to a `wp:anchor` + `wps` `line` shape positioned `relativeFrom="line"`.
  The writer declares `xmlns:wp`/`xmlns:a`/`xmlns:pic` on `document.xml`
  and `footnotes.xml` whenever the part holds any drawing (image or
  anchored line) — not only images — and strokes a colorless line with an
  explicit black `a:solidFill` so the shape renders without a theme
  default. C1 is unchanged (degradation + text kept); an overline the
  paged match cannot cover keeps the flag and degrades the same way. The
  underline stopgap is gone.

### Heading-size measurement
- The IR path measures per-level heading sizes from the collected
  headings' own runs (glyph-weighted dominant size per level), mirroring
  the legacy `measure_heading_sizes`; the rank-based size histogram is
  only the fallback for levels with no sized run
  (`measure_heading_sizes_from_ir` in `realize/mod.rs`).

### Footnote markers in fragments
- Fragment realizations are single-pass, so their footnote counter never
  resolves and the marker text is "0". The collector treats "0" as
  unresolved and substitutes the ordinal, and the patch falls back to
  ordinal-order matching when the fragment Location misses. Footnote
  entries in `footnote_in_table` and `formatted_footnote` are
  byte-identical.

### Footnote bodies from content
- Footnote entry bodies are collected from the `FootnoteElem.body`
  content at the marker (not from Paged frames): the tag group carries
  only the marker, but the element itself carries the full body. This
  gives real OMML for footnote math (inline and display — display
  equations are kept as inline `IrInline::Math` whose OMML already
  carries the `<m:oMathPara>` wrapper, the legacy shape), real
  hyperlinks (display text included), and real images (with Paged
  display sizes patched in, including footnote bodies). The frame body
  remains the fallback when collection fails or is empty. A
  `FootnoteBody::Reference` keeps the frame body (see the gap above).
  Circled footnote numbering (①②③) is read from the footnote
  elements' numbering patterns (`pattern.apply(None, &[1])` tested with
  the legacy `parse_circled_number`) and written to
  `DocumentStyle.footnote_format` in `export_ir`.

### Page references (M8)
- `counter(page).at(<label>)` is detected in fragments via a
  `ContextElem` branch in `handle_tag_start`: the tag carries only the
  rendered page number, so the label is recovered from the source AST
  (`counter_page_label`, the same resolver the legacy path uses) via the
  main source threaded into the Collector, and the collector emits
  `IrInline::Ref { page_ref: true }` (lowered to a PAGEREF field).
  Anything the resolver cannot identify falls through to plain-text
  collection.

### Body-size baseline sampling
- The body-default vote (`defaults_from_pairs`) samples only the leading
  portion of the document (9,000 glyphs — the pairs-level analogue of
  the legacy "first 3 pages" rule), so back matter cannot win the vote
  and misdetect the body size. All three votes (size, first font, last
  font) share the budget.

### Inline images in fragments
- `handle_tag_start` has an `ImageElem` branch: an image arriving as a
  tag in a fragment (mid-paragraph images, images inside footnote
  bodies, figure-body SVGs) is collected via `image_inline` instead of
  being dropped. This also fixed the figure-SVG case
  (`image_inside_rounded_container_embedded`).

### Display equations in nested sequences
- `collect_body_blocks` recursively flattens `SequenceElem` children,
  so a block element nested in a sequence child (e.g. a display equation
  inside a `#body` substitution) is dispatched as a block instead of
  being swallowed into the pending inline run and dropped. This fixed
  the definition's display equation in `edge_theorem_proof`
  (`misc::edge_theorem_proof_content`).

### Body-size detection vs footnote text
- Footnote entry text (0.85em) is excluded from the body-size/font
  histograms and from run-style detection (shared predicate
  `is_footnote_entry_group` in `convert/frames.rs`), so a
  footnote-dominated document no longer misdetects Normal as the
  footnote size.

### SVG image sizes
- Re-encoded images (SVG/GIF/WebP) are matched to the Paged size map by
  `IrImage.source_hash` (the hash of the original bytes, taken before
  re-encoding), not by hashing the stored bytes.

### Term lists
- `TermsElem` is collected (bold term runs, tight → one paragraph per
  item, wide → term paragraph + description blocks, including nested
  bare `TermItem` children).

### Run spans
- IR runs carry their source span (threaded from the pair content), so
  span-keyed lookups (the `counter(page).at` exception, overline geometry
  matching) locate IR content.

### Pure shapes via the layouter span index
- The built-in Paged show rules rewrite `#rect`/`#square`/`#ellipse`/
  `#circle`/`#polygon`/`#curve`/`#line` into a `BlockElem` whose body is
  a private layouter capture — element identity is erased. But the
  rewrite copies the element's span onto the block, so the collector
  indexes the pre-realization content tree by span
  (`index_content_tree`, generic `Content::fields()` walk) and recovers
  the original element (`collect_layouter_block`, plus the `BlockElem`
  branch in `collect_fragment_inlines` for mid-paragraph shapes).
  Shapes become `IrBlock`/`IrInline::Shape` — C1 emits a `shape`
  degradation, all modes keep the body text (`#rect[Text]` survives).
  `#line` resolves the same way — its built-in show rule is
  context-wrapped, so evaluating the `ContextElem` exposes the
  `LineElem`, and `line_blocks` emits a horizontal-rule paragraph (the
  former M5 `#line` gap against the legacy path is closed).
- Content produced inside `context { … }` keeps the `ContextElem`'s
  span, so the index returns the closure wrapper rather than the
  displayed element. `collect_context_group` applies the public
  `typst_library::foundations::CONTEXT_RULE` to the packed
  `ContextElem` — the same rule realization runs — and collects the
  displayed content recursively (timble's `#context rect(grid(…))`
  resolves to a real grid this way). A context expression with no
  location reports `context-content`; a layouter block whose span
  cannot be traced back at all reports `layouter-body`.

### Citation sources
- The IR path uses `extract_bibliography_sources` through its own
  introspector (`DocxDocument` implements `Introspector` via the
  pair-tag `ElementIntrospector`). The function was refactored to take
  `&dyn Introspector` so both paths share it; there is nothing left to
  delete here.

### Dual `CompatMode` / `Degradation` definitions
- `typort-core` (`compat.rs`) and `typort-ir` (`mode.rs`,
  `degradation.rs`) define identical `CompatMode` /
  `DegradationAction` / `Degradation` types, converted at the boundary
  in `realize/mod.rs` (`ir_compat`, `core_degradation`).
- **Unify when:** a follow-up moves the canonical definitions into
  `typort-ir` and has core re-export them (or vice versa). Not a
  correctness issue — the conversion is exhaustive and tested by the
  equivalence corpus — but it is duplication worth one cleanup change.
- **Owner:** step-2 follow-up.

### Grid planning (batch 9, M4 Phase 2a)
- `GridElem` collection keeps every source row (empty time-slot rows
  included) plus per-cell `x`/`colspan`/`rowspan`/fill/stroke, the
  resolved `CellGrid` track sizing and h/v lines, header flags, and
  row heights measured by laying the grid fragment out with
  `typst_layout::layout_frame` at the page content width.
  `typort-ir::grid_plan::plan_grid` is a pure planner: it derives a
  lane/track occupancy grid from cell coordinates and spans — the
  empty cells Typst materializes at positions no declared cell covers
  (empty body, no fill) count as mergeable whitespace, while a stroke
  on such a cell still pins the boundary it sits on — compresses runs
  of uncut rows into one `w:tr` with `w:vMerge`-free exact
  `w:trHeight`, merges adjacent lanes whose occupancy is identical
  into `w:gridSpan` columns, and maps cell spans to
  `gridSpan`/`vMerge`. Lanes covered by a span are planning
  markers only (`PlannedCellKind::Filler`) — they never become extra
  `w:tc` elements. Planned tables get fixed layout, per-column twip
  widths resolved from `fr`/fixed/relative/auto tracks, zero cell
  margins, and 1pt paragraph marks in empty cells so Word does not
  inflate empty slots; grid h/v lines become `w:tcBorders` and cell
  fills `w:shd`, propagated across span continuations.
- **Remaining degradations:** a grid wider than `MAX_COLUMNS` (63)
  lanes is split at lane boundaries into consecutive tables and
  reports `grid-column-split`; `fr` row heights are measured from one
  laid-out frame, so they are the renderer's heights, not Word's own line
  layout, while `auto` rows are measured per row from their own cell
  bodies; C1 still flattens grids to paragraphs with the
  `grid-layout` degradation.
