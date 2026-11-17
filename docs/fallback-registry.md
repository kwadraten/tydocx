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
  `detect_page_setting_change` (source AST). The gate tests run against
  `convert()` — the IR path — unchanged.
- **Owner:** C1 Phase 1 agent, then user review.

## Post-passes the IR path deliberately skips

### Recovery passes (`recover_missing_content`)
- **What:** geometry-driven recovery of content the HTML walk missed
  (`#align(center)` blocks and similar).
- **Delete when:** the IR path proves no fixture needs it — i.e. the
  `recovery_*` and `edge_recovery_*` fixtures are byte-identical between
  the two paths, or their diffs are ruled IR-correct. Until then the
  legacy path keeps it and the IR path simply does not recover.
- **Owner:** step-2 follow-up, after the equivalence corpus is green.

### Three-line-table borders (`detect_three_line_tables`)
- **What:** table borders inferred from the rules actually drawn in the
  Paged frames (three-line vs grid tables).
- **Delete when:** the IR path derives the same borders from the pairs
  (table stroke data is in the fragment grid; the lowering already maps
  it) and the `edge_three_line_table` / `mixed_table_borders` fixtures
  match. Status: the border mapping is implemented
  (`table_borders_from_grid` in `realize/collect.rs`: explicit lines map
  per side with a header separator, else cell strokes decide, else nil)
  and no border diff remains on any fixture; the `mixed_table_borders`
  fixture's remaining diff is the legacy Paged-recovery duplicate
  paragraph (ruled old-path bug, see `ir_path.rs`), not borders.
- **Owner:** step-2 follow-up.

### Section breaks from Paged (`detect_section_breaks`,
`apply_section_breaks`, `build_element_page_map`)
- **What:** section breaks inferred from page-setting changes across
  Paged pages, placed via an element→page map.
- **Delete when:** the IR path emits section breaks from its own
  `PageBreak { to }` / page-setting pairs, and the
  `section_break_test` / `edge_multi_section` /
  `issue_final_section_landscape` fixtures match. The C1 half is done:
  `export_ir` emits the `section-break` degradation from the source-AST
  `detect_page_setting_change` when a mid-document columns/margins
  change is collapsed to a single section.
- **Owner:** step-2 follow-up.

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
- **Delete when:** the `pagebreak_*` and `issue_column_break` fixtures
  are byte-identical between the two paths. The IR path already emits
  `PageBreak`/`ColumnBreak` from the pairs; this item is only the
  source-AST backstop for breaks the pairs never see (inactive branches,
  show-rule-generated breaks).
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
- **What:** `export_ir` still runs the legacy three-source extraction
  (Paged + source-AST overrides) on a scratch `Document` and copies
  `page`/`style`/header/footer into the IR. The IR path does not yet
  derive page settings or running headers from the pairs.
- **Delete when:** page settings and headers/footers are derived from the
  realize pairs (page settings live on the root `StyleChain`) plus the
  source-AST overrides, with the `style_*` / `header_footer_test` /
  `edge_multi_section` fixtures matching.
- **Owner:** step-2 follow-up.

## Known gaps in the IR path itself

### M9 overline
- **What:** overline is a line shape above the text — `Run` carries it
  as `Run::overline` (never serialized directly) and the lowering
  translates it per compat mode: C1 drops it with an `overline`
  degradation (text kept); C2/C3 rewrite it to underline, matching the
  legacy walk.
- **Delete when:** the lowering gains a real overline shape (a paragraph
  border or a drawing above the run), per the M9 design decision
  (overline = line shape above text). Until then the degradation is
  recorded on the construct, not silently dropped.
- **Owner:** batch 7 (M9 + L2).

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

### Drawn / vector figure bodies (rasterization)
- **What:** a figure whose body is drawn content (shapes, CeTZ, lines)
  is emitted as an empty paragraph plus a `figure-body-empty`
  degradation. The legacy path rasterizes the frame to PNG
  (`rasterize_html_frame` / `rasterize_frame`).
- **Delete when:** the collector finds the figure's Paged frame by
  location, rasterizes it to PNG (typst-render), and plumbs it through
  `IrImage`. Affects `edge_figure_rasterized`.
- **Owner:** step-2 follow-up (after the M4 grid work; vector figures are
  the C2 SVG-embed ladder, Phase 2d).

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
- **Current:** the first page's margin-zone header is emitted; a differing
  later header reports `header-footer-variant` (all modes).
- **C2 target:** `w:headerReference w:type="even"`/`"default"` parts plus
  `w:evenAndOddHeaders` in settings; title-page suppression maps to
  `w:titlePg`.

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

### `#rect` / drawn color bands (M5, T-04)
- **Current:** `IrBlock::Shape`/`IrInline::Shape` drops the drawing; its
  text body is kept as paragraphs; C1 reports `shape`.
- **C2 target:** a full-width paragraph shading (`w:shd`/`w:pBdr`) or a
  VML/DrawingML rectangle; body text in the shaded paragraph.

### Mid-document margin/column changes (M6, T-06)
- **Current:** single section; C1 reports `section-break` from
  `detect_page_setting_change`. `margin_amount` still reads a `0pt` margin
  as unset (explicit zero margins are undetectable).
- **C2 target:** one `w:sectPr` per Typst section carrying its real
  `w:pgMar`; `margin_amount` must treat `0pt` as a declared zero.

### `inside:`/`outside:` margins (U31, T-07)
- **Current:** keys parse to `margins_mirrored`; C1 reports
  `mirrored-margins`, sides keep the Typst defaults.
- **C2 target:** `w:mirrorMargins` in `settings.xml`; `sectPr` `pgMar`
  left=inside, right=outside (Word swaps on even pages).

### Page-numbering format change + restart (U30, T-08)
- **Current:** a second distinct `numbering:` declaration sets
  `numbering_changed`; C1 reports `page-numbering-change`, no `pgNumType`.
- **C2 target:** per-section `w:pgNumType` (`fmt="lowerRoman"`/`"decimal"`,
  `w:start` on restart). Depends on M6 multi-section output.

### Header/footer run weight (L3, T-22)
- **Current:** margin-zone extraction keeps per-run size (`w:sz`) but
  flattens weight; `header-footer-style` reports it when a bold weight
  (>=600) is flattened.
- **C2 target:** per-run `<w:b/>` in header/footer parts.

### Function footnote numbering `(n)` (U26, T-09)
- **Current:** `footnote-numbering` degradation (literalized); markers fall
  back to decimal `w:footnoteReference`.
- **C2 target:** pending design decision — literal `(n)` marker text (the
  L2-style literal fallback) vs `w:numFmt`; no Word numFmt renders
  parentheses.

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
- Drawn/vector figure rasterization (gap above):
  `drawing_figure_is_rasterized_not_leaked`,
  `text_figure_does_not_steal_drawing_raster`.
- Recovery passes the IR path skips: `edge_bordered_blocks_text_preserved`
  (`#rect()` text only; the `#block` assertions pass on the IR path),
  `grid_document_model_has_tab_inlines` and `grid_multi_column_has_tab_stops`
  (tab assertions pin the superseded M4 representation; grids are tables
  on the IR path), `issue_rotate_content_recovered` (`#rotate()` only
  recoverable from Paged).
- Section breaks from Paged: `edge_landscape_pages_orientation`,
  `edge_multi_section_different_page_sizes`,
  `issue_final_section_landscape_sections`,
  `section_break_document_model_has_section_break`,
  `section_break_produces_multiple_sect_pr`.
- `#line` horizontal rules (M5 gap): `hrule_produces_paragraph_with_bottom_border`,
  `imported_template_line_rule_recovered`,
  `hrule_document_model_has_horizontal_rule_flag`,
  `source_line_call_still_produces_a_rule`.
- Fragment counter re-evaluation (gap above):
  `golden_edge_theorem_proof`.

Regenerated on the IR path (the IR output is correct per the rulings in
`ir_path.rs`, so the snapshots were rewritten with `UPDATE_SNAPSHOTS=1`
and the diffs reviewed): `golden_aligned_equations` (display-equation
centering), `golden_edge_term_list` (tight term lists are one paragraph
per item), `golden_formatted_footnote` (stale snapshot expected a
pre-cleanup redundant size override).

## Resolved — no longer a fallback

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
  the source-AST post-passes (hanging indent, small caps) locate IR
  paragraphs.

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
  `#line` is recovered the same way and deliberately still dropped —
  the horizontal-rule recovery stays a legacy-only pass.

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
