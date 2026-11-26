# OOXML output contract and feature map

Use the **emission contract** to decide whether the writer may generate a
tree, and the **feature map** below to decide when the converter selects it.
The map alone does not specify valid XML, part ownership, or application
support. The writer targets **WML Transitional with selected Microsoft
extensions**, not Strict. C1/C2/C3 are tydocx policies, not ECMA conformance
classes or guarantees that a given reader renders the document.

| Writer question | Contract |
|---|---|
| Which editions, namespaces, ZIP parts, media types, relationships and compatibility rules apply? | [Profile, packaging and namespaces](spec-ecma376/package.md) |
| What are the exact drawing containers, geometry, line sequence, picture/SVG and text-box payloads? | [DrawingML, pictures and Word shapes](spec-ecma376/drawing.md) |
| Where does OMML go, and how do tabs, fields, bookmarks, hyperlinks and story parts interact? | [Math and cross-references](spec-ecma376/math-fields.md) |

## Compatibility levels

The CLI flag `--compat` accepts `c1`, `c2` and `c3`; the default is `c2`.

| Level | Name | Contract |
|-------|------|----------|
| C1 | minimal | Only constructs inside the Typst/OOXML intersection are emitted. Anything outside it is skipped or literalized and reported as a named degradation. |
| C2 | graphical | Default. Everything C1 emits, plus Word's graphical vocabulary (anchored floats, numbered-list patterns, cell shading, section breaks, headers/footers) to preserve appearance where the semantic mapping does not reach. |
| C3 | dynamic | Everything C2 emits, plus Word fields around representable values C2 rendered literally. The C2 literal becomes the cached result; `w:updateFields` enables refresh. |

In the tables below, the "C2/C3" column describes what both levels emit; a
row calls out the C3 form explicitly only where C3 differs from C2.

## How to read this document

- Element names use their OOXML prefixes (`w:`, `m:`, `wp:`, `a:`, `mc:`).
- Quoted names like `"vector-image"` are degradation constructs — the
  `construct` string in the `Degradation` report.
- Degradations raised while *collecting* the IR fire at every level (they
  describe aspects no level can express); degradations raised while *lowering*
  are mostly C1-only, because C1 is where the intersection is enforced.
- "Literalized" means the rendered text is kept but the dynamic/semantic
  aspect is dropped; "approximated" means a nearby OOXML construct stands in;
  "skipped" means nothing is emitted.

## Runs and text

| Typst | OOXML | C1 | C2/C3 |
|-------|-------|----|-------|
| `#text(font:)` | For declared fonts, select by grapheme cluster: try the resolved list in declaration order, respecting `covers` and the selected `FontBook` face's glyph coverage. Eligible uninstalled declarations can retain their names; otherwise use available fallback when enabled. Split runs when their `w:rFonts` `w:ascii`/`w:eastAsia` selections conflict. A slot is not assigned by taking the first or last family. | same | same |
| `#raw` / monospace fonts | Raw resolves its own text rules for font, fallback and size; block lines use `CodeBlock` and retain resolved run overrides. Runs marked monospace use the document `code_font`; explicitly selected font slots can instead emit their own `w:rFonts`. Monospace family detection uses font metadata, not name matching. | same | same |
| `#text(size:)` | Resolve the run size from the style chain and round to half-points; emit `w:sz`/`w:szCs` for overrides. Reuse matching body, heading, code or footnote styles rather than emitting redundant run sizes. Sub/superscripts follow their `w:vertAlign` path without explicit reduced size. | same | same |
| weight / `#strong` | `w:b` | same | same |
| italic / `#emph` | `w:i` | same | same |
| `#underline` | `w:u w:val="single"`; unsupported decoration options → `"underline-style"`, custom offset → `"underline-offset"` | same | same |
| `#strike` | `w:strike`; unsupported decoration options → `"strikethrough-style"` | same | same |
| `#overline` | C1: dropped + `"overline"`. C2/C3: measured `wp:anchor` line shape (`AnchoredLine`); unmatched runs degrade | dropped + report | `wp:anchor` line; unmatched → report |
| `#highlight` | `w:highlight` (palette-mapped); non-default extent/edges/radius/stroke → `"highlight-geometry"` (all levels) | same | same |
| `#smallcaps`, `smcp`/`c2sc` features | `w:smallCaps` | same | same |
| `#sub` / `#super` | `w:vertAlign` subscript/superscript | same | same |
| `#text(baseline:)` | `w:position` half-points | same | same |
| `#text(fill:)` (non-black) | `w:color` | same | same |
| `#text(tracking:)` | `w:spacing` | same | same |
| ligatures / numeral form / numeral width | Microsoft `w14:ligatures`, `w14:numForm`, `w14:numSpacing` with `w14:val` inside `w:rPr` declaring `mc:Ignorable="w14"` | same | same |
| `#text(lang:, region:)` | `w:lang` (document-level via `Normal` style) | same | same |
| `top-edge` / `bottom-edge` metrics | dropped + `"text-edge-metrics"` (all levels) | same | same |
| `#text(hyphenate: false)` | `w:autoHyphenation w:val="0"` in `settings.xml` | same | same |
| `#text(case:)`, `#lower`/`#upper` | literal text transform | same | same |
| smart quotes | literal typographic characters | same | same |
| U+200B zero-width space / U+2060 word joiner (Typst's URL break hints in realized text) | dropped from `w:t` text; Word finds its own break opportunities | same | same |
| `stroke`, `kerning`, `spacing`, `overhang`, `costs`, `cjk-latin-spacing`, fractions, slashed-zero, stylistic sets, font variations | not read — dropped silently | same | same |

## Paragraphs

| Typst | OOXML | C1 | C2/C3 |
|-------|-------|----|-------|
| `#par(justify:)` | `w:jc` on `Normal` style; a paragraph whose own chain resolves a different `justify` carries the `w:jc` override (`both`/`left`) | same | same |
| `#align` / folded alignment | `w:jc` per paragraph | same | same |
| `#par(first-line-indent:)` | `w:ind w:firstLine` (+ `w:firstLineChars` for em amounts) on `Normal`; a paragraph whose chain resolves a different amount carries the `w:ind` override (`0` clears a char-based Normal indent; negative is `w:hanging`) | same | same |
| `#par(hanging-indent:)` | `w:ind w:hanging` | same | same |
| `#par(leading:)` | `w:spacing w:line w:lineRule="atLeast"` (cap_height + leading) | same | same |
| `#par(spacing:)` | `w:spacing w:after` (spacing minus leading compensation) | same | same |
| `#par.line(numbering:)` | `w:lnNumType w:distance` in `w:sectPr` | same | same |
| `#block(breakable: false)` / sticky | `w:keepLines` / `w:keepNext` | same | same |
| `#v(amount)` | `w:spacing w:before` on the next paragraph | same | same |
| `#h(amount)` | `w:spacing` (tracking) on a space run | same | same |
| `fr` / ratio / negative `#h`/`#v` | dropped + `"h-fr-space"` / `"v-fr-space"` / `"-ratio-space"` / `"-negative-space"` (all levels) | same | same |
| linebreaks `simplex`/`optimized`, `justification-limits`, widow/orphan costs | not read — dropped silently | same | same |

## Headings

| Typst | OOXML | C1 | C2/C3 |
|-------|-------|----|-------|
| `= Heading` | `w:pStyle Heading1`–`Heading9` (level >5 uses the level-5 size) + `w:bookmarkStart/End` for labels; the heading body's resolved `par` (`first-line-indent`, `justify` — including set rules scoped inside `show heading` bodies) emits `w:ind`/`w:jc` overrides where it differs from `Normal` | same | same |
| heading level >9 | `Heading9` + literal rendered numbering instead of an out-of-range `w:numPr`, with `"heading-level"` approximated | same | same |
| `#set heading(numbering: "1.1")` | `w:numPr` against a heading `abstractNum` (per-level `lvlText`/`numFmt`); counter divergence vs the rendered number → `"heading-numbering"` approximated | literal `numbering_text` run in the heading paragraph | `w:numPr` mapping |
| numbering via a function or unmappable numeral system | literal text + `"heading-numbering"` literalized (all levels) | same | same |
| `#heading(outlined: false)` | plain paragraph (keeps the number text, keeps the bookmark) so it stays out of the TOC + `"unoutlined-heading"` (all levels) | same | same |

## Lists, enumerations, terms

| Typst | OOXML | C1 | C2/C3 |
|-------|-------|----|-------|
| `- item` | `w:numPr` (`w:numId`/`w:ilvl`) against a bullet `abstractNum`; `indent`/`body-indent` → `w:ind` left+hanging; `spacing` → `w:after` on the item's last paragraph | default bullet; custom markers → `"list-marker"` literalized | custom text markers kept as `bullet` `lvlText` (e.g. `—`, `▸`); function markers → `"list-marker"` |
| `+ item` | `w:numPr` against an enum `abstractNum` with per-level `numFmt`/`lvlText` and `startOverride`; `full:` numbering → multi-level `lvlText` (`%1.%2`) | shared default decimal + `"enum-numbering"` when a non-default pattern would be needed | pattern mapped |
| numeral systems | `numFmt`: arabic→`decimal`, `a`/`A`→`lowerLetter`/`upperLetter`, `i`/`I`→`lowerRoman`/`upperRoman`. Other systems (e.g. CJK) and function numbering → the item's literal `number_text` run instead of `w:numPr` | literal | literal |
| `#enum(reversed: true)` | no OOXML countdown — every item's collector-rendered numeral emits as a literal `number_text` run (faithful order); an item without one (unmappable pattern) or a nested reversed enum keeps `w:numPr` forward numbering | `"enum-reversed"` literalized | same |
| enum with an empty pattern | literal `number_text` runs (all levels, e.g. function numbering whose text was rendered) | same | same |
| `/ term: desc` | plain paragraphs: bold term run + separator + description | same | same |

## Tables and grids

| Typst | OOXML | C1 | C2/C3 |
|-------|-------|----|-------|
| `#table` | `w:tbl` — `tblGrid` column widths, `gridSpan`, `vMerge`, `tblCellMar` (inset), `tblHeader` (header rows), per-cell `w:vAlign`, cell `w:jc`, nested tables | same | same |
| `#table(stroke:)` / `hline`/`vline` | `w:tblBorders` (uniform) or per-cell `w:tcBorders` | same | same |
| `table.cell(fill:)` | `w:shd w:fill` | skipped + `"table-cell-fill"` | `w:shd` |
| `#table(column-gutter:, row-gutter:)` | emulated with transparent `tcBorders` | skipped + `"table-gutter"` | gutter borders |
| unresolved cell grid | falls back to a paragraph stream + `"table-grid-unresolved"` (all levels) | same | same |
| `#grid` | `w:tbl` via the pure `grid_plan` lowering when the planner changes the structure (row coalescing, column merging, `trHeight`, `tcBorders`); a grid it leaves unchanged lowers to a plain `w:tbl` with no `trHeight` | flattened to paragraphs + `"grid-layout"` | `w:tbl` |
| grid > 63 columns | split into stacked tables + `"grid-column-split"` (all levels) | flattened | split |
| row heights | rows the grid planner rewrites get exact `trHeight` from Typst layout; Word reflows ordinary grid/table rows | flattened | `trHeight` on planned rows only |

## Figures, equations, images

| Typst | OOXML | C1 | C2/C3 |
|-------|-------|----|-------|
| `#figure` | body + caption paragraph; `gap` → spacing; label → bookmark; supplement + number in the caption; C3: a single-piece caption number renders as ` SEQ <Figure\|Table> [\* fmt] ` with the rendered numeral cached | same | C2: literal number / C3: ` SEQ ` field |
| `figure(placement: top/bottom)` explicit | lone-table body → `w:tblpPr` floating table (caption becomes a grid-spanning row); other bodies → anchored `wps:txbx` at the measured position | body kept in flow + `"figure-placement"`/`"placed-content"` | `w:tblpPr` / anchored `txbx`; unmeasured → flow fallback + `"placed-content"` |
| `$x$` / `$ x $` | `m:oMath` inline / `m:oMathPara` block; equation numbering `(1)`; C3: a single-piece numbering pattern renders as ` SEQ equation [\* fmt] ` with the rendered numeral cached | same | C2: literal number / C3: ` SEQ ` field |
| `#image` (raster) | `w:drawing` + `wp:inline` + `a:blip` (`pic:pic`) | same | same |
| `#image` (SVG) | PNG `a:blip/@r:embed` with `a:extLst/a:ext/asvg:svgBlip/@r:embed` pointing to the vector bytes (no `mc:AlternateContent`) | PNG bytes only + `"vector-image"` | PNG base plus SVG extension |
| `image(fit: "cover")` | `a:srcRect` l/t/r/b derived from the measured clip window; the declared box stays the `wp:extent` and the full bytes stay embedded | same | same |
| `image(fit: "contain"/"stretch")` | fit aspect dropped; display size still comes from the measured frame | + `"image-fit"` | silently dropped |
| `block(clip: true)`/`box(clip:)` + negative `inset:` | `a:srcRect` l/t/r/b from the clip window vs the image bounds (composes with a cover fit) | same | same |
| `image(alt:)` | Inline pictures carry `wp:docPr/@descr` from the alt text | + `"image-alt"` (dropped) | `descr` on inline pictures |
| PDF image source | skipped + `"pdf-image"` (all levels — the IR path has no rasterizer; the legacy path rasterizes PDF pages) | same | same |

## Footnotes

| Typst | OOXML | C1 | C2/C3 |
|-------|-------|----|-------|
| `#footnote` | real `w:footnote` part + `w:footnoteRef` marker style + `w:footnoteReference` | same | same |
| decimal numbering | Default decimal `w:footnotePr` numbering (writer omits `w:numFmt`) | same | same |
| circled (`①`) numbering | `w:numFmt decimalEnclosedCircle` | same | same |
| `counter(footnote).update(0)` in a page header | `w:numRestart eachPage` | same | same |
| unsupported numbering pattern (e.g. `(n)`) | literal marker text + `"footnote-numbering"` | literal + report | footnote still emitted with decimal/`numFmt` numbering — the custom marker silently becomes decimal |
| `footnote.entry(clearance/gap/indent/separator)` | `w:footnotePr` spacing + `w:separator` line stroke | same | same |
| footnote marker inside a context where a `w:footnoteReference` cannot live | literal text + `"footnote"` (all levels) | same | same |
| `#footnote.entry` bodies | collected from the `FootnoteElem` pairs and patched from the `PagedDocument`; a missing entry body → `"footnote-entry-not-found"` | same | same |

## Bibliography and citations

| Typst | OOXML | C1 | C2/C3 |
|-------|-------|----|-------|
| `#bibliography` | Editable entries; `customXml/item1.xml` carries the source data. When no entry has custom display, C3 adds a block `w:sdt` with the `BIBLIOGRAPHY` field and cached entries. A custom display stays as editable paragraphs + `"bibliography-field"` at C3 rather than being replaced on refresh. | literal paragraphs | C2 literal; C3 field when eligible |
| bibliography entries | Use semantic bibliography markers and displayed content when available; fall back to styled CSL entries + `"bibliography-display"` if reconstruction fails. Preserve entry bookmarks, links and measured hanging layout where available. Failure to generate or locate the bibliography can report `"bibliography-entries"`. | same | same |
| `@key` / `#cite` | C3 can wrap the rendered marker in a `CITATION` complex field. A resolved entry bookmark is linked with `w:hyperlink/@w:anchor`; `customXml/item1.xml` carries source metadata. | literal marker | C2 literal; C3 field when eligible |
| `#cite(form:)` | rendered form literal | same | same |

## Outlines and cross-references

| Typst | OOXML | C1 | C2/C3 |
|-------|-------|----|-------|
| `#outline()` | title paragraph + ` TOC \o "1-N" \h \z \u ` complex field | same | same |
| `outline(target:)` non-heading | title paragraph only (entries unrecoverable) | + `"outline-target"` | silently title-only |
| `outline(indent:)` custom | TOC field without per-level indent | + `"outline-indent"` | silently dropped |
| `#link(url)` | `w:fldSimple` `HYPERLINK` for ordinary URLs; `w:hyperlink r:id` with a part-local External relationship for targets requiring URI encoding, or style-preserving links | same | same |
| `@label` | ` REF <label> \h ` complex field with the rendered text cached | same | same |
| `@label` with `supplement: auto`/`Page` | ` PAGEREF <label> \h ` | same | same |
| unresolved label/link target | literal text + `"unresolved-link"` (all levels) | same | same |
| `context counter(page)` in body text | ` PAGE ` field with cached display text | same | same |
| `context counter(page).final()` | C3: ` NUMPAGES ` field with the rendered total cached; after a page-number restart the final logical number differs from the physical count ` NUMPAGES ` refreshes to — the cache stays the Typst value + `"page-counter-final"` | literal + `"page-counter-value"` | C3 emits the field |
| arithmetic on `counter(page)` | literal + `"page-counter-value"` (all levels) | same | same |
| `counter(footnote)` references | literal + `"footnote-counter-reference"` (C1/C2). C3: ` NOTEREF <label> \h ` with the rendered marker cached; the labelled `FootnoteElem` carries a `w:bookmarkStart`/`w:bookmarkEnd` of the label's name — a reference whose label never lands on a footnote stays literal + the same report | same | C3 emits the field |
| labels on headings/figures/equations | `w:bookmarkStart`/`w:bookmarkEnd` | same | same |
| `#quote` | left-indented paragraphs; attribution appended | same | same |

## Page setup

| Typst | OOXML | C1 | C2/C3 |
|-------|-------|----|-------|
| `#set page(width/height/margin)` | `w:pgSz` + `w:pgMar`; `flipped` → `w:orient="landscape"` | same | same |
| `#set page(columns:)` | `w:cols w:num` + `w:cols w:space` (gutter) | same | same |
| `#set page(header-ascent/footer-descent)` | `w:header`/`w:footer` in each `w:pgMar`; use supplied distances first, otherwise emit the half-inch default. Binding `w:gutter` is zero, independent of column gutter. | same | same |
| `#set align(horizon/bottom)` page | `w:vAlign` in `w:sectPr` | same | same |
| `#set page(numbering: "…")` | `w:pgNumType w:fmt` per section; unmappable pattern → `"page-numbering-pattern"` and glyph-based detection (all levels) | same | same |
| page number restart | `w:pgNumType w:start` when the rendered footer numbering diverges, stamped once on the section that began the restart — `continuous` breaks inside it (`#columns`) keep `w:fmt` without re-initiating `w:start` | same | same |
| `#set page(header:/footer:)` | `w:headerReference`/`w:footerReference` (default, first, even) + `w:titlePg`/`w:evenAndOddHeaders`; generated page-number footer slots contain ` PAGE ` fields. C3 caches the heading query result inside ` STYLEREF "Heading N" `, preserving surrounding literal runs when the query span can be located. | page-1 zone kept; unrepresented variants report `"header-footer-variant"` | three slots; mid-document variants beyond what the slots express → `"header-footer-variant"` report |
| `#set page(margin: (inside/outside))` two-sided | `w:mirrorMargins` in `settings.xml` when any section declares inside/outside (`two_sided` accumulates over sections); `w:pgMar` left/right are inside/outside | + `"mirrored-margins"`, no flag | flag emitted; a doc mixing inside/outside with one-sided asymmetric margins → `"mirrored-margins"` approximated (the flag is document-global) |
| `#set page(foreground:/background:)` | `wp:anchor` (never `wp:inline`) inside the default/even/first **header** part selected by the layer's page-variation shape; `positionH`/`positionV` `relativeFrom="page"`; text editable in `wps:txbx`, images as `pic:pic`, solid rects as `a:solidFill` shapes; background units get `behindDoc="1"`, foreground `behindDoc="0"` | furniture text literalized into the header part + `"page-foreground"` | anchored; transformed/clipped groups keep their content but lose the transform + `"page-foreground"`; unrepresentable per-page variation keeps page 1's layer on `first` + `"page-foreground"` approximated |
| `page(fill:)` solid | full-page `wp:anchor` `a:solidFill` shape, `behindDoc="1"`, in the default header part | + `"page-foreground"` literalized | non-solid fills → `"page-foreground"` skipped |
| `#place` inside the header/footer zones | `wp:anchor` paragraph in the same header/footer part (`relativeFrom="page"`), coexisting with the zone's literal/field paragraphs | + `"placed-content"` | anchored in the zone part |
| `#set document(title/author/description/keywords/date)` | `docProps/core.xml` (`dc:title`, `dc:creator`, `dc:description`, `cp:keywords`, `dcterms:created`); title falls back to the first heading | same | same |
| `context document.<field>` reads | C3: ` DOCPROPERTY <name> ` with the rendered text cached — `title`→`Title`, `author`→`Author`, `keywords`→`Keywords`, `description`→`Comments`, `date`→`CreateDate`; other fields stay literal | same | C3 emits the field |
| `context datetime.today()` | C3: ` DATE ` with the rendered text cached and a `\@` switch for a representable explicit format; unmappable/computed formats report `date-format-switch` | literal | C3 emits the field |

## Sections, breaks, placement

| Typst | OOXML | C1 | C2/C3 |
|-------|-------|----|-------|
| mid-document `#set page(...)` change | `w:sectPr` transitions ending each section (`nextPage`, or `continuous` when only numbering/format changes) | sections collapsed to a single section + `"section-break"` report | `w:sectPr` per section |
| `#pagebreak` | `w:br w:type="page"`; `weak:` honored — no break when the page is already fresh, and runs of consecutive breaks coalesce | same | same |
| `#pagebreak(to: odd/even)` | `w:sectPr w:type` `oddPage`/`evenPage` (emitted at **all** levels — it is an authored break, not a style change) | same | same |
| `#linebreak` | `w:br`; a paragraph whose breaks are all plain (unjustified) pins `w:jc="left"` — Word stretches `w:br`-terminated lines under an inherited `both`; `linebreak(justify: true)` keeps the paragraph stretchable | same | same |
| `#colbreak` | `w:br w:type="column"` | same | same |
| `#columns(n)[…]` block | a `continuous` `w:sectPr` pair around the block carrying `w:cols w:num` + `w:space` (gutter resolved against the text width); nested inside a table cell, figure, quote, … the body is kept flat + `"column-block"` | same | same |
| `#place(float: true)` | `wp:anchor` `w:drawing` with measured page-absolute position; `wrapTopAndBottom` or `wrapSquare`, `distT`/`distB` from `clearance:`, `behindDoc="0"`; a lone-table body → `w:tblpPr` floating table instead | body kept in flow + `"placed-content"` report | anchored float / `w:tblpPr`; unmeasured → flow fallback + `"placed-content"` |
| `#place` non-float | `wp:anchor` at the measured extent (no page-edge expansion): `wp:wrapNone`, `behindDoc` from paint order (behind when the page paints more content after the place); text in editable `wps:txbx` (`w:jc` pinned, `bodyPr wrap="none"` + `a:noAutofit`), images as `pic:pic`, a filled `rect` as `a:solidFill`; an unfilled text box fills with the colour painting under it (latest covering placed `rect`, else solid `page(fill:)`, else paper) so it covers its underlay; a lone-table body → `w:tblpPr`; a box fully inside a side margin anchors `relativeFrom="margin"` | + `"placed-content"` | anchored overlay / `w:tblpPr`; unmeasured → flow fallback + `"placed-content"` |
| `float(dx:/dy:)` offsets | included in the anchor position | n/a | same |

## Shapes, lines, transforms

| Typst | OOXML | C1 | C2/C3 |
|-------|-------|----|-------|
| `#rect`/`#square`/`#ellipse`/`#circle`/`#polygon`/`#curve`/`#path` | `wp:inline` + `wps:wsp` drawing: `prstGeom` (`rect`/`roundRect`/`round1Rect`/`round2SameRect`/`round2DiagRect`/`ellipse`) or `a:custGeom` (polygon/curve paths, quads → cubics), solid fill → `a:solidFill`, stroke → `a:ln` (width/cap/join/`custDash`; [compound line syntax](spec-ecma376/drawing.md#line-properties)), body in editable `wps:txbx` | body text + `"shape"` report | drawn; unrepresentable pieces report `"shape"` Approximated; unresolvable geometry falls back to `w:shd`-shaded body |
| Default full-width horizontal `#line` in ordinary flow | paragraph `w:pBdr` bottom border | same | same |
| Other `#line` (short, offset, styled, angled, vertical, or measured in a stack) | `wp:inline` + `wps:wsp` — preset `line` + `a:xfrm` flips or `a:custGeom`; compatible lines in a stack can share one shape with separate subpaths, preserving their spacing | skipped + `"shape"` | drawn, with `"shape"` for detected losses |
| `#move`/`#rotate`/`#scale`/`#skew` | content kept, transform dropped + `"moved-content"` / `"rotated-content"` / `"scaled-content"` / `"skewed-content"` (all levels) | same | same |
| `#hide` | content and its space dropped + `"hidden-content"` (all levels) | same | same |
| `#box`/`#block` width/baseline/fill/stroke | `"box-width"`/`"box-baseline"`/`"box-decoration"`/`"block-width"`/`"block-decoration"` (all levels); body kept | same | same |
| `#pad` | non-negative absolute/em horizontal insets → paragraph `w:ind`; top/bottom insets → first/last paragraph `w:spacing` before/after; the inherited weak paragraph gap is retained unless an explicit outer `block(below:)` replaces it; negative or ratio components and padding around non-paragraph bodies remain unrepresented | same | same |
| `#repeat`/`#layout`/`#state`/`#counter` displays / `#context` | literalized where the rendered text is available + `"context-content"`/`"layouter-body"`/`"page-counter-value"`; otherwise dropped | same | same |

## Output rules

**C1 skip semantics.** "Skip" means skip the unsupported *aspect*, never the
whole element when something expressible remains: S1 drop the property, keep
the text (overline, cell fill); S2 drop position/structure, keep the text in
flow (floats, placed content, shapes); S3 drop the element only when nothing
expressible remains (textless shapes); S4 fall back to a representable form
(list/enum numbering, heading numbering); S5 keep the compile-time literal
when the dynamic aspect is unrepresentable (counter displays — losing
dynamicism is not itself reported). C1 never silently drops text.

**C2 rules.** Tricks never degrade an existing semantic mapping (headings stay
`Heading` styles, lists stay `w:numPr`, text stays searchable runs); a trick
is additive — removing it leaves valid content, not an empty shell; no fake
semantics (literal numbers are text, not `w:numPr`); when a trick cannot be
measured it falls back to the C1 form plus a report, never a misplaced trick.

**Layering.** A C3 field's cached result preserves the corresponding C2
literal. Only C3 emits `w:updateFields` in `settings.xml`.

**Aspect-level SVG embedding.** The C2 fallback ladder's last resort, when no
native or shape encoding exists, is embedding the construct as an SVG part
(`asvg:svgBlip` + PNG fallback) — with an `Approximated` degradation noting
the text is no longer in Word's text layer. It must not replace a native
encoding that exists.

## Degradation report

The conversion returns the document and degradations, each with a source span,
construct, action and message. `--degradation-report <path>` writes the report
as JSON.
