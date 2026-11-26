# DrawingML, pictures and Word shapes

[Output contract and feature map](../spec-ecma376.md) · [Profile and namespaces](package.md)

Sequences below are ordered; `?` means zero or one, `+` one or more,
`|` an exclusive alternative. **Core** means the indicated ECMA XSD or
prose; **writer** means the narrower tydocx contract, not an ECMA rule.

## Drawing ownership and positioning

Core: [Part 1][p1] §§17.3.3.9, 20.1.2.2.16–17, 20.4.2.3–8,
20.4.2.10–13, 20.4.2.15–20; [Part 4][p4] Annex A,
`wml.xsd` (`CT_Drawing`) and `dml-wordprocessingDrawing.xsd`
(`CT_Inline`, `CT_Anchor`, `CT_PosH`, `CT_PosV`, `EG_WrapType`).

```text
w:p / w:r / w:drawing
  wp:inline      → wp:extent, wp:effectExtent?, wp:docPr, wp:cNvGraphicFramePr?, a:graphic
  OR wp:anchor  → wp:simplePos, wp:positionH, wp:positionV, wp:extent,
                  wp:effectExtent?, ONE wrap, wp:docPr, wp:cNvGraphicFramePr?, a:graphic
a:graphic       → a:graphicData[@uri] → ONE payload (writer restriction)
```

- Emit exactly one inline/anchor per drawing. The drawing is run content;
  do not emit it as a sibling of `w:r` or place an OMML object inside it.
  A `w:hyperlink`, field result, table cell, text box or another story
  does not change the owning run or relationship part.
- Both containers accept unqualified `distT`, `distB`, `distL`, `distR`
  (`ST_WrapDistance`, unsigned 32-bit EMUs); those distances do not
  affect inline layout. The anchor requires `relativeHeight` (unsigned
  32-bit), `behindDoc`, `locked`, `layoutInCell` and `allowOverlap`
  (XML booleans). `hidden` and `simplePos` are optional booleans in the
  XSD; the writer explicitly sets `simplePos="0"` and still emits the
  required `wp:simplePos x="0" y="0"`. Each position requires `relativeFrom`
  and exactly one `wp:align` or `wp:posOffset`. Use signed 32-bit EMUs for
  the latter, including negative offsets.
- `ST_RelFromH` allows `margin`, `page`, `column`, `character`,
  `leftMargin`, `rightMargin`, `insideMargin`, `outsideMargin`.
  `ST_RelFromV` allows `margin`, `page`, `paragraph`, `line`,
  `topMargin`, `bottomMargin`, `insideMargin`, `outsideMargin`.
  The writer uses `page` or `margin` horizontally; vertically `page`
  for furniture/floats and `line` for an overline.
- Core wrap choice: `wp:wrapNone`, `wp:wrapSquare`, `wp:wrapTight`,
  `wp:wrapThrough` or `wp:wrapTopAndBottom`. The writer selects None
  for no flow displacement (overlays/furniture/overlines), Square
  with required `wrapText="bothSides"`, or TopAndBottom. Its measured
  clearance goes on the anchor's `distT`/`distB`; don't emit a
  competing wrap-level distance. Use `relativeHeight` for order
  within the selected `behindDoc` layer, not to encode a third layer.
- `wp:extent` requires `cx` and `cy`; `wp:effectExtent`, if present,
  requires `l`, `t`, `r`, `b` (`a:ST_Coordinate`; currently zero).
  `wp:docPr` requires unqualified `id`, `name`; optional `descr`
  carries accessible text. Allocate unique drawing IDs across stories
  ([Part 1][p1] §20.4.2.5 forbids duplicates within a document).
  Keep them `1…2147483646` for Word; the schema's
  `ST_DrawingElementId` is unsigned, but Word rejects zero and larger
  values ([MS-OI29500][oi] §2.1.1351). A name or ID inside
  `pic:cNvPr` does not replace `wp:docPr`. The writer shares one drawing
  counter across document, footnotes, headers and footers in serialization
  order. Image indexes still resolve relationships in their owning part.
  No endnote part is currently emitted.
- `a:graphicData/@uri` identifies the payload vocabulary, not a
  relationship or media URL. Its wildcard in both ECMA XSDs is
  **strict** (`CT_GraphicalObjectData`). Select the Transitional picture
  URI `http://schemas.openxmlformats.org/drawingml/2006/picture`
  for `pic:pic`, or `http://schemas.microsoft.com/office/word/2010/wordprocessingShape`
  for Microsoft `wps:wsp` ([MS-ODRAWXML][od] §2.2.7.3).

### Units and shape properties

Core: [Part 1][p1] §§20.1.2.1, 20.1.7.6, 20.1.9,
20.1.10.16–19, 20.1.10.42 and `dml-main.xsd` (`CT_Transform2D`,
`CT_Point2D`, `CT_PositiveSize2D`, `CT_ShapeProperties`, `EG_Geometry`).

- Length: **914400 EMU/in, 360000 EMU/cm, 12700 EMU/pt**.
  Do unit conversion before serializing XML; neither pixels nor WML
  twips nor the percentage scale are EMUs. The writer uses integral EMUs.
- `a:xfrm` orders `a:off?` before `a:ext?` (writer emits both).
  `a:off/@x,@y` are signed `ST_Coordinate` (integer branch
  `−27273042329600…27273042316900`); use `(0,0)` for the local
  transform and the `wp:position*` owner for page placement.
  `a:ext/@cx,@cy` and `wp:extent/@cx,@cy` are `ST_PositiveCoordinate`
  (`0…27273042316900`, **zero is legal**). Match the two sizes for current
  pictures/shapes; require positive visible picture dimensions, but
  allow a horizontal or vertical line to have one zero axis.
  Word and PowerPoint cap these coordinates at signed 32-bit limits
  ([MS-OI29500][oi] §§2.1.1319, 2.1.1330).
- `rot` (`ST_Angle`, 60000ths of a degree), `flipH` and `flipV` are
  optional with defaults zero/false/false. Use flips rather than
  negative extents for preset lines. An unsigned extent does not make
  signed path points or signed anchor offsets illegal.
- In both `pic:spPr` and `wps:spPr`, `CT_ShapeProperties` orders optional
  `a:xfrm`, one geometry, one **shape** fill, `a:ln`, effects, `a:scene3d`,
  `a:sp3d`, `a:extLst`. Do not confuse the shape fill with the line fill.
  Current filled shapes use `a:solidFill/a:srgbClr[@val]` with six hex
  digits (no `#`); the bare shape emits `a:noFill` for no fill.
- Preset `a:prstGeom` requires `prst` (`ST_ShapeType`) and permits
  `a:avLst?` of `a:gd` (`name`, `fmla` required). Current presets are
  `rect`, `roundRect`, `round1Rect`, `round2SameRect`, `round2DiagRect`,
  `ellipse`, `line`; use named `val N` guides from the official
  `OfficeOpenXML-DrawingMLGeometries.zip` in [Part 1][p1], not guessed names.
  Custom `a:custGeom` orders `a:avLst?`, `a:gdLst?`, `a:ahLst?`,
  `a:cxnLst?`, `a:rect?`, required `a:pathLst`. Current output uses
  `a:rect l="l" t="t" r="r" b="b"` and one nonempty path, with optional
  nonnegative `w`/`h` defining the path's coordinate space (not page
  offsets). Start a subpath with `a:moveTo/a:pt`; next use
  `a:lnTo/a:pt`, `a:cubicBezTo` (control, control, endpoint `a:pt`),
  or `a:close` as appropriate. Each `a:pt` has `x`,`y` (adjustable
  coordinates; the writer emits signed integer literals). The schema
  also has other path primitives not emitted by this writer.

### Line properties

Core: [Part 1][p1] §§20.1.2.2.24, 20.1.8.21–22, 20.1.8.38,
20.1.8.48, 20.1.8.57, 20.1.10.15/.31/.35/.39/.46/.49;
`dml-main.xsd`: `CT_LineProperties`, `EG_LineFillProperties`,
`EG_LineDashProperties`, `EG_LineJoinProperties`, `CT_DashStopList`,
`CT_DashStop`, `CT_LineEndProperties`.

```text
a:ln[@w?, @cap?, @cmpd?, @algn?]
  (a:noFill | a:solidFill | a:gradFill | a:pattFill)?
  (a:prstDash | a:custDash)?
  (a:round | a:bevel | a:miter)?
  a:headEnd?
  a:tailEnd?
  a:extLst?
```

| Item | Encoding and limits |
|---|---|
| `w` · `ST_LineWidth` | Unqualified integer EMUs, `0…20116800` inclusive. Word treats zero width as a minimum-width line, not as no paint ([MS-OI29500][oi] §2.1.1326). A zero-width shape stroke emits `a:ln/a:noFill`. |
| `cap` · `ST_LineCap` | `flat`, `rnd`, `sq` (flat/butt, round, square). Core prose says square if omitted. Current inline strokes set it explicitly. |
| `cmpd` · `ST_CompoundLine` | `sng` (single, default in prose), `dbl` (two equal-width lines), `thickThin`, `thinThick`, `tri`. Part 1's type prose mentions text/underlines; the XSD applies it through `CT_LineProperties` to shape outlines, and [MS-OI29500][oi] §2.1.1318 explicitly confirms Office usage on shape outlines. |
| `algn` · `ST_PenAlignment` | `ctr` or `in`; not set by the writer. Office shape semantics: [MS-OI29500][oi] §2.1.1328. |
| Line fill | At most one. `a:solidFill/a:srgbClr[@val]` is the current paint, optionally with child `a:alpha/@val` (opacity, `ST_PositiveFixedPercentage`, `0…100000` in writer units; [Part 1][p1] §§20.1.2.3.1/.32, [Part 4][p4] §§17.2.2.3/.8). An unsupported non-solid paint currently falls back to black and should be reported. `a:ln/a:noFill` explicitly suppresses the line; omission of `a:ln` supplies no direct override and may allow a style/default to apply. A line fill is not a `blipFill` or `grpFill`. |
| Preset dash · `ST_PresetLineDashVal` | `a:prstDash/@val`: `solid`, `dot`, `dash`, `lgDash`, `dashDot`, `lgDashDot`, `lgDashDotDot`, `sysDash`, `sysDot`, `sysDashDot`, `sysDashDotDot`. Attribute optional in XSD; specify it for a deterministic selected preset. Do not combine with custom dash. |
| Custom dash | `a:custDash/a:ds` in pattern order, each with required `d` and `sp`, lengths relative to the **line width**. `ST_PositivePercentage` admits zero; the Transitional numeric member represents thousandths of a percent (`100000` = one line width) and is limited to `0…2147483647` by the XSD's integer base. Core Strict uses e.g. `100%`. XSD admits an empty list; the writer contract is to omit the dash element for a solid stroke rather than write an empty or non-advancing pattern. There is no phase attribute. |
| Join and miter | At most one join. `a:miter/@lim` optional, `ST_PositivePercentage` (same lexical/unit rules); use a meaningful finite limit. Miter is not a cap and must follow dash. |
| Ends | `a:headEnd` applies to the line's start and `a:tailEnd` to its end, in that order. Each has optional `type`: `none` (default), `triangle`, `stealth`, `diamond`, `oval`, `arrow`; optional `w` and `len`: `sm`, `med`, `lg`, **not** EMU widths. Writer currently emits neither end. |

Percentage authority: [Part 4][p4] §§17.2.2.2, 17.2.2.4, 17.2.2.9
and Annex A (composite schema); [MS-OI29500][oi] §2.1.1329 is Office
lexical behavior, not the Strict definition.

Reusable example: one editable, double-stroked shape outline with total
line-width **parameter** 1 pt (not a guarantee of each rail's thickness):

```xml
<a:ln w="12700" cap="flat" cmpd="dbl">
  <a:solidFill><a:srgbClr val="000000"/></a:solidFill>
  <a:prstDash val="solid"/>
  <a:miter lim="400000"/>
</a:ln>
```

`dbl` does not define a gap-to-rail ratio or independent rail styling.
Preserve separate paths or objects for independently positioned lines.
`w:pBdr` is a different vocabulary (`w:bottom` with `CT_Border`, eighth-points
and `ST_Border`;
[Part 1][p1] §§17.3.1.7, 17.3.1.24, 17.18.2); it is not `a:ln`.
Likewise, run decorations are `w:rPr/w:u` (`ST_Underline`), `w:strike` or
`w:dstrike` ([Part 1][p1] §§17.3.2.9/.37/.40, 17.18.99); do not put
`a:ln` or an arbitrary dash sequence in a WML run property.

## Picture payload

Core: [Part 1][p1] §§15.2.14, 20.2.2.1–6, 20.1.8.13,
20.1.8.55–56 and the `dml-picture.xsd` / `dml-main.xsd` definitions
`CT_Picture`, `CT_PictureNonVisual`, `CT_BlipFillProperties`, `CT_Blip`,
`AG_Blob`, `CT_RelativeRect`, `CT_StretchInfoProperties`.

```text
pic:pic
  pic:nvPicPr → pic:cNvPr[@id,@name], pic:cNvPicPr
  pic:blipFill → a:blip[@r:embed], a:srcRect?, a:stretch/a:fillRect
  pic:spPr → a:xfrm/(a:off, a:ext), a:prstGeom[@prst="rect"]/a:avLst
```

The three `pic:` children and two non-visual children are required by
schema. The fill schema permits optional blip/crop and a choice of tile
or stretch (with optional `fillRect`), but the project always embeds a
blip and stretches to the empty `fillRect`'s default bounds. For crop
use `l,t,r,b` (`ST_Percentage`, default `0%`) as separate offsets from
the corresponding source edges; positive crops inward, negative
outsets. The project clips to a nonempty source intersection before
emitting a crop, keeps the original bytes, and uses the visible size
as the extent. Crop does not change relationships. `r:embed` denotes
an internal image; `r:link` denotes an externally linked image and is
not emitted. Both are optional in the XSD, but the project cannot
emit an empty image reference. A `blip` may have optional
`cstate` (`ST_BlipCompression`, writer omits it) and effects before its
extension list.

### PNG with SVG extension

**Microsoft**, not a core SVG vocabulary:
[MS-ODRAWXML][od] §§1.3.3, 2.26.1.1, 2.26.3.1, 5.24
(`CT_SVGBlip`, `a:AG_Blob`); core extension slot:
[Part 1][p1] §§20.1.2.2.14–15, `CT_OfficeArtExtension`,
`CT_OfficeArtExtensionList` (`a:ext` requires `uri` and has a lax
wildcard). Preserve this writer shape in inline and anchored pictures:

```xml
<a:blip r:embed="rIdRaster">
  <a:extLst>
    <a:ext uri="{96DAC541-7B7A-43D3-8B79-37D633B846F1}">
      <asvg:svgBlip xmlns:asvg="http://schemas.microsoft.com/office/drawing/2016/SVG/main"
                    r:embed="rIdVector"/>
    </a:ext>
  </a:extLst>
</a:blip>
```

The two IDs point to distinct Internal image relationships from the same
owning story, PNG and SVG respectively. Older consumers can paint the
main PNG without reading the extension. Do not make the primary blip
point to SVG, emit two drawings, or put the SVG bytes inline as XML.

## Microsoft Word shape payload

**Microsoft** [MS-ODRAWXML][od] §§2.2.7.3, 2.13.1.1,
2.13.3.1–3, 5.2. In the target profile the parent is `a:graphicData`
with the Microsoft shape URI, not the core `wp:wsp` namespace.

```text
wps:wsp → (wps:cNvSpPr | wps:cNvCnPr), wps:spPr, wps:style?, wps:extLst?,
          (wps:txbx | wps:linkedTxbx)?, wps:bodyPr
wps:txbx → w:txbxContent, wps:extLst?   (tydocx Transitional subset)
```

- On this direct `a:graphicData` branch, **omit** `wps:cNvPr` per
  §2.13.3.3; when used in a group the same protocol instead requires it
  (groups are not emitted here). Use `wps:cNvSpPr`; `txBox="1"` is the
  writer's text-box signal, not `w:txBox`. No-text shapes omit `wps:txbx`;
  they still require `wps:bodyPr`. Optional `normalEastAsianFlow` is
  boolean, defaults to false and is not emitted.
- `wps:txbx` is the first/only box in a text story; subsequent linked boxes
  would use `wps:linkedTxbx`, which is not implemented. Microsoft permits
  optional `id` (`unsignedShort`, default `0`); if emitting explicit
  story IDs, keep them unique and link only to an existing story. For
  this Transitional branch `w:txbxContent` is WML 2006 (called `w12` in
  the protocol). Do not substitute `a:txBody`, `wps:txbxContent`, or a
  WML Strict-namespace copy. The protocol specifies the Microsoft `wne`
  variant in place of `w12` for its Strict extension format.
- `w:txbxContent` contains WML **blocks**, not naked runs or OMML.
  `CT_TxbxContent`/`EG_BlockLevelElts` admit `w:p`, `w:tbl` and block
  `w:sdt`; a table is not prohibited by syntax. The converter's
  lone-table placement uses the separate WML `w:tbl/w:tblPr/w:tblpPr`
  branch ([Part 1][p1] §17.4.57). Prefer a real block for editable text
  or a textless shape without a text box; avoid an empty story. No
  nested text boxes, VML or references to other WML stories (notes and
  comments) in the text box; see the analogous core text-box constraints
  in [Part 1][p1] §20.4.2.38 and the WML container in [Part 4][p4]
  §14.9.1.1 / `wml.xsd`. Office restrictions on textbox placement in
  footnotes/endnotes/comments are noted in [MS-OI29500][oi] §2.1.1779
  for the Transitional VML form; do not generalize it into an XSD ban
  on all `wps` placements.
- `wps:bodyPr` uses `a:CT_TextBodyProperties` (core `dml-main.xsd`).
  Insets `lIns,tIns,rIns,bIns` are signed `ST_Coordinate32` (writer
  emits EMUs); `wrap` is `none` or `square`, `anchor="t"` top;
  `vertOverflow="overflow"` and `horzOverflow="overflow"` are valid.
  One autofit choice at most, before any scene/text-3D/extensions.
  Fixed-size inline shape text uses `square` / `a:noAutofit` with
  measured insets. No-wrap anchored text uses `none` / `a:noAutofit`
  with zero insets; wrapping anchors use `square` / `a:spAutoFit`
  with zero insets. Omitting a fill is not the same syntax as `a:noFill`.

The ECMA `wp:wsp` ([Part 1][p1] §20.4.2.42,
`dml-wordprocessingDrawing.xsd`) uses `wp:txbx/wp:txbxContent` and is
distinct from Microsoft's `wps:txbx/w:txbxContent`. The writer emits
the Microsoft shape directly. [MS-ODRAWXML][od] §2.2.7.3 specifies the
MCE alternative with a `wps` choice and `w:pict` fallback.

## Branch selection and failure behavior

| Branch | Writer output and required fallback |
|---|---|
| Inline/anchored raster and vector picture | `pic:pic` in the selected inline/anchor. C1 PNG only for SVG + `vector-image`; C2/C3 PNG + Microsoft SVG payload. |
| Inline bare shape/line | C2/C3 `wps:wsp`, preset/custom geometry, shape paint, optional ordered line and editable body. C1 keeps body in flow + `shape`. At C2/C3 unresolved geometry goes to shaded/flow body; report `shape` for approximation (paint, unsupported phase, per-side stroke, even-odd fill, outset). |
| Anchored overline | `wp:anchor` + line preset, horizontal `page` / vertical `line`, `cy=0`, `wp:wrapNone`, solid stroke. C1 or unmeasured C2/C3 drops the line only + `overline`; prose survives. |
| Float, overlay, furniture | Anchored image, editable box, or solid-fill shape in its drawing story; lone-table placement can use `w:tblpPr` instead. C1 or unmeasured placement falls back to flow/literal furniture, with `placed-content` or `page-foreground` as applicable. C2/C3 do not invent measurements or duplicate visible text. |
| Paragraph rule | Existing WML border branch is independent of DrawingML. Do not flatten independently positioned lines into a paragraph border solely because they are horizontal. |

[p1]: https://ecma-international.org/wp-content/uploads/ECMA-376-1_5th_edition_december_2016.zip
[p4]: https://ecma-international.org/wp-content/uploads/ECMA-376-4_5th_edition_december_2016.zip
[od]: https://learn.microsoft.com/en-us/openspecs/office_standards/ms-odrawxml/06cff208-c6e1-4db7-bb68-665135e5f0de
[oi]: https://learn.microsoft.com/en-us/openspecs/office_standards/ms-oi29500/1fd4a662-8623-49c0-82f0-18fa91b413b8
