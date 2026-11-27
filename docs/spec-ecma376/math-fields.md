# OMML, fields, bookmarks and hyperlinks

[Output contract and feature map](../spec-ecma376.md) · [Profile and part ownership](package.md)

## Where math is allowed

Core authority: [Part 1][p1] §§22.1.2.77–79, 22.1.2.87 and Annex A.6.1
(`shared-math.xsd`), plus `wml.xsd` (`CT_P`, `EG_PContent`,
`EG_RunLevelElts`) in the [Transitional bundle][p4]. Word is stricter
than the schema: [MS-OI29500][oi] §§2.1.1687–1688 documents that math
zones outside a `w:p`, math without `m:oMath`, and nested math roots
can prevent opening the file. Use these project forms:

| Writer branch | Ordered output |
|---|---|
| Inline equation | `w:p → …, m:oMath, …`. It is paragraph content, a sibling of ordinary runs, not the child of `w:r`. May occur in a legal run-level wrapper such as `w:hyperlink` within the paragraph. |
| Unnumbered display | `w:p → w:pPr?, m:oMathPara → m:oMathParaPr?, m:oMath+`. The writer emits one `m:oMath`; a display zone is not a replacement for the containing WML paragraph. |
| Numbered display | `w:p` with `w:pPr/w:tabs` (center, then right), followed by `w:r/w:tab`, **bare** `m:oMath`, `w:r/w:tab`, then the number as WML text or a WML `SEQ` field. Keep the label's bookmark around the referenceable number, not around the *whole* equation or the whole line. Delimiters are outside the numeral range when `REF` expects the bare numeral. |
| Nested blocks/stories | Use the corresponding `w:p` in a body, table cell, `w:txbxContent` or footnote. Bind `m` in the owning XML part. A header/footer top-level `Math` is not a supported current writer branch; math in an explicitly dispatched nested graphic/text box has its own WML paragraph but the same OPC owner. |

Word may merge adjacent `m:oMath` or adjacent `m:oMathPara` in a paragraph
([MS-OI29500][oi] §§2.1.1687–1688); do not depend on preserved
adjacency to distinguish zones. The tabbed number form is a layout
choice, not a requirement to add an equation number to OMML. Paragraph
tab stops are WML twips, not DrawingML EMUs.

`writer/math.rs` accepts pre-serialized OMML from `tydocx::math` and writes
it as raw XML. **Input contract for this boundary:** produce one complete,
balanced OMML root, with no declaration, DTD, unrelated siblings, or
unbound prefixes. Escape math text as XML text (the `&` alignment
token is `&amp;` in markup). The numbered branch takes a single `m:oMath`
zone, with or without its `m:oMathPara` wrapper.

### Child ordering for math writers

The following uses `?` for optional and `+` for one or more. Except
for `w:rPr` the table names use `m:`; properties precede arguments.
Sources: [Part 1][p1] §22.1.2 (individual element definitions),
`shared-math.xsd` (`CT_OMath`, `CT_OMathArg` and the indicated types).
An argument holds math elements, not another math root; required
arguments may be empty where allowed (e.g. hidden degree or missing
limit), not omitted.

| Object / type | Ordered children |
|---|---|
| `r` / `CT_R` | `rPr?`, Word run properties if used, then run contents such as `t` (writer uses `m:t`). `m:sty/@m:val` accepts `p`, `b`, `i`, `bi`. |
| `f` / `CT_F` | `fPr?`, `num`, `den` |
| `rad` / `CT_Rad` | `radPr?`, `deg`, `e`; degree visibility via `radPr/degHide` |
| `nary` / `CT_Nary` | `naryPr?`, `sub`, `sup`, `e` (operand); don't append an intended operand as detached siblings |
| `sSub`, `sSup`, `sSubSup` / `CT_SSub`, `CT_SSup`, `CT_SSubSup` | respective `*Pr?`, then `e sub`, `e sup`, or `e sub sup` |
| `sPre` / `CT_SPre` | `sPrePr?`, `sub`, `sup`, `e` |
| `d` / `CT_D` | `dPr?`, `e+`; choose `begChr`/`endChr` in the properties |
| `m` / `CT_M`, `mr` / `CT_MR` | `mPr?`, `mr+`; each row contains `e+` cells |
| `eqArr` / `CT_EqArr` | `eqArrPr?`, `e+` (one per row) |
| `func` / `CT_Func` | `funcPr?`, `fName`, `e` |
| `limLow`, `limUpp` / `CT_LimLow`, `CT_LimUpp` | respective `*Pr?`, `e`, `lim` |
| `acc`, `bar`, `groupChr`, `borderBox`, `box`, `phant` | respective `*Pr?`, `e` (`CT_Acc`, `CT_Bar`, `CT_GroupChr`, `CT_BorderBox`, `CT_Box`, `CT_Phant`) |

`settings.xml/w:settings` has a separate `m:mathPr` with document
defaults such as `m:mathFont m:val="Cambria Math"` ([Part 1][p1]
§§22.1.2.61–62, `CT_MathPr` and `CT_Settings`). That is the writer's
chosen default, not an ECMA requirement that all consumers use Cambria
Math. `CT_R` and `m:ctrlPr` (§22.1.2.23) also provide WML formatting
hooks; absence of a Typst-to-OMML paint mapping is not proof that OMML
forbids color. Preserve editable math at all compat levels where
representable; use the math conversion's `MathNote`/degradation path
for dropped or approximated constructs.

## Field, bookmark and link contract

Core authority: [Part 1][p1] §§17.13.6.1–2, 17.16.1–2,
17.16.18–19, 17.16.22–23 and `wml.xsd` (`CT_Bookmark`,
`CT_MarkupRange`, `CT_FldChar`, `CT_SimpleField`, `CT_Hyperlink`).
The field language is parsed **after** decoding XML; quote/escape
field arguments for that language independently of escaping XML.

```text
complex: w:r/w:fldChar[@w:fldCharType="begin"]
         w:r/w:instrText[@xml:space="preserve"]
         w:r/w:fldChar[@w:fldCharType="separate"]
         cached paragraph/run content in its legal WML context
         w:r/w:fldChar[@w:fldCharType="end"]

simple:  w:fldSimple[@w:instr] → optional w:fldData, legal paragraph content
link:    w:hyperlink[@w:anchor OR @r:id] → legal paragraph content
range:   w:bookmarkStart[@w:id,@w:name] … w:bookmarkEnd[@w:id=<same id>]
```

- A complex field must close in its own document story. `separate`
  is optional in core but written by tydocx for cached results. The
  begin/instruction/separate/result/end sequence must balance even when
  the result spans paragraphs (TOC or bibliography); `w:fldChar` and
  `w:instrText` are run children, not bare block children.
- `w:fldSimple/@w:instr` is required; optional `w:fldLock`/`w:dirty`
  are not used by the simple-link writer. Result content is real WML,
  not a second copy of the instruction. The C3 conversion must keep
  the C2 cached result; if it cannot be represented without semantic
  loss, keep the literal content instead. `w:updateFields` belongs in
  `settings.xml` only at C3 (project policy; [Part 1][p1] §17.15.1.90);
  refresh is consumer behavior, not a guarantee that Typst computed the
  same value as the field (e.g. logical vs physical page count).
- Pair bookmarks by `w:id` in document order. Use unique names,
  allocate noncolliding IDs, normalize/truncate once, and use the
  **final emitted name** in every `REF`, `PAGEREF`, `NOTEREF` and
  `w:anchor` reference. Core allows longer names, but Word limits
  names to 40 characters and resolves duplicates differently from
  the standard ([MS-OI29500][oi] §2.1.356(b)–(c)); check collisions
  after normalization. Unresolved targets use literal text +
  `unresolved-link`, not a field that refreshes to an error.
- Internal link: `w:hyperlink/@w:anchor` (bookmark name, no leading
  `#`), no relationship. External relationship link:
  `w:hyperlink/@r:id` → part-local `/hyperlink`, `TargetMode="External"`;
  URI encoding is separate from XML escaping. Do not write both
  attributes for a single branch: the standard ignores `anchor`
  when `r:id` is set, while Word appends it to the relationship URL
  ([MS-OI29500][oi] §2.1.521). Ordinary external links can instead
  use `w:fldSimple w:instr='HYPERLINK "URL"'`, no relationship needed.
  Either form can own the run containing a drawing without moving
  image references to another part.
- C1/C2 citations keep the literal marker (and bookmark link when
  resolved); C3 `CITATION` is run-level `w:sdt` with `w:sdtPr/w:citation`,
  then `w:sdtContent` containing the complex field, optionally linked
  **cached** marker. Eligible C3 `BIBLIOGRAPHY` uses a block-level SDT with
  `w:sdtPr` bibliography/gallery properties and `w:sdtContent` with
  opening, result and closing paragraphs. C1/C2 and C3 entries with
  custom display remain editable paragraphs without that field.
  The field's source tags
  must match the attached `b:Sources` payload, not the anchor's name.
  See [Part 1][p1] §§17.5.2.2/.4/.11/.13, 17.16.5.7–8, 22.6
  and [MS-OI29500][oi] §2.1.464 for citation implementation details.
- A native footnote reference is `w:r/w:footnoteReference[@w:id]`
  resolving to the matching `w:footnote` in the related footnotes
  part. Its visible in-note mark comes from `w:r/w:footnoteRef`;
  a label on that mark is a candidate `NOTEREF` bookmark. Keep
  separator entries distinct from authored note IDs; the writer
  reserves IDs 0 and 1 for its own separators (project choice).
  See [Part 1][p1] §§11.3.7, 17.11.10, 17.11.13–15.

Verify the operation and its switches against the actual field
definitions: [Part 1][p1] §17.16.5 `DATE` (.13), `DOCPROPERTY`
(.14), `HYPERLINK` (.25), `NOTEREF` (.40), `NUMPAGES` (.42),
`PAGE` (.44), `PAGEREF` (.45), `REF` (.51), `SEQ` (.56),
`STYLEREF` (.59), `TOC` (.68). C1/C2 already emit baseline fields
such as `REF`, `PAGEREF`, `PAGE`, `TOC` and simple `HYPERLINK`;
C3 adds the dynamic conversions described in the [feature map](../spec-ecma376.md).

[p1]: https://ecma-international.org/wp-content/uploads/ECMA-376-1_5th_edition_december_2016.zip
[p4]: https://ecma-international.org/wp-content/uploads/ECMA-376-4_5th_edition_december_2016.zip
[oi]: https://learn.microsoft.com/en-us/openspecs/office_standards/ms-oi29500/1fd4a662-8623-49c0-82f0-18fa91b413b8
