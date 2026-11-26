# OOXML profile, packaging and namespaces

[Output contract and feature map](../spec-ecma376.md)

## Standards profile

The reference is ECMA-376 fifth edition, **by part**:
[Part 1 (2016)][p1] defines the vocabularies, package-part types, semantic
constraints and Strict schemas; [Part 2 (2021)][p2] defines OPC;
[Part 3 (2015)][p3] defines Markup Compatibility and Extensibility (MCE);
[Part 4 (2016)][p4] defines Transitional features and the composite
Transitional schemas. The [official ECMA index][ecma] identifies the ISO/IEC
29500 family and notes only Part 2 was revised in the December 2021 release.
References to ISO/IEC 29500 in Microsoft protocols identify the ISO edition
used by that protocol, not a new edition pin for this project.

Part 1 §2.1 and Part 4 §2.1 require an OPC package of the applicable class,
schema validity after the specified compatibility processing, and other
constraints beyond schema. Transitional is not just a namespace alias:
among other differences, it admits VML (`w:pict`, Part 4 §14.3.2.2) and
unitless integer percentages in DrawingML (thousandths of one percent;
Part 4 §17.2.2). Strict requires the `%` lexical form for the DrawingML
percentages covered here. Neither profile is implied by `--compat c1`;
all three writer levels use Transitional URIs.

| Prefix | Transitional URI | Strict URI |
|---|---|---|
| `w` | `http://schemas.openxmlformats.org/wordprocessingml/2006/main` | `http://purl.oclc.org/ooxml/wordprocessingml/main` |
| `m` | `http://schemas.openxmlformats.org/officeDocument/2006/math` | `http://purl.oclc.org/ooxml/officeDocument/math` |
| `r` | `http://schemas.openxmlformats.org/officeDocument/2006/relationships` | `http://purl.oclc.org/ooxml/officeDocument/relationships` |
| `a` | `http://schemas.openxmlformats.org/drawingml/2006/main` | `http://purl.oclc.org/ooxml/drawingml/main` |
| `wp` | `http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing` | `http://purl.oclc.org/ooxml/drawingml/wordprocessingDrawing` |
| `pic` | `http://schemas.openxmlformats.org/drawingml/2006/picture` | `http://purl.oclc.org/ooxml/drawingml/picture` |

Authority: [Part 1][p1] Strict XSD bundle and [Part 4][p4] Transitional XSD
bundle (`targetNamespace` in `wml`, `shared-math`, `shared-relationshipReference`,
`dml-main`, `dml-wordprocessingDrawing`, `dml-picture`). This includes
namespace-qualified relationship **attributes**. Relationship **type URIs**
are separate literal values, not QNames, and must also match the profile
(e.g. `/image`, Part 1 §15.2.14; its Transitional spelling is in Part 4
§13.2.14). Do not mix namespace families inside one object.

### Extension namespaces

- `mc` = `http://schemas.openxmlformats.org/markup-compatibility/2006`
  ([Part 3][p3] §7.1), unchanged across profiles.
- `wps` = `http://schemas.microsoft.com/office/word/2010/wordprocessingShape`;
  `asvg` = `http://schemas.microsoft.com/office/drawing/2016/SVG/main`.
  These **two actual namespaces/payloads** are Microsoft extensions
  ([MS-ODRAWXML][od] §§2.13, 2.26). Do not confuse `wps:wsp` with the
  distinct ECMA `wp:wsp` in both fifth-edition schemas.
- `w14` = `http://schemas.microsoft.com/office/word/2010/wordml`.
  [MS-DOCX][ms-docx] §§2.2.1, 2.6.1.17–19 define
  `w14:ligatures`, `w14:numForm` and `w14:numSpacing` with `w14:val`
  and require declaring the extension prefix ignorable when using them
  in `w:rPr`. The writer binds `w14` and `mc` and declares
  `mc:Ignorable="w14"` on each `w:rPr` that carries these properties.
  This scope also covers runs in footnotes, headers, footers, links and
  field results without changing unaffected runs.
- Bind every used prefix in scope **in each XML part**, including prefixes
  inside `mc:Ignorable` and `mc:Choice/@Requires`. Never list a needed core
  namespace as ignorable. `mc:Ignorable="prefix"` permits a consumer that
  does not understand that namespace to ignore markup; it does not supply
  a replacement or make the surviving XML semantically useful. Part 3
  §§7.2–7.4, 9.2 and 9.4 specify removal, `ProcessContent` and
  `MustUnderstand`.
- For an MCE alternate, place `mc:AlternateContent` at a valid substitution
  point (e.g. in the run replacing its drawing); one or more `mc:Choice`
  with unqualified `Requires` (space-separated non-empty list of bound
  prefixes), then at most one `mc:Fallback`. The first supported choice
   wins. If no choice matches and there is no fallback, nothing replaces the wrapper
  ([Part 3][p3] §§7.5–7.7, 9.3–9.4). Do not put the only editable body in
  an ignorable subtree and assume it survives.
- MCE does not preprocess an application-defined extension element's contents
  ([Part 3][p3] §§8–9). The SVG branch uses DrawingML's `a:extLst` and
  a raster base, not `mc:AlternateContent`. [MS-ODRAWXML][od] §2.2 specifies
  `Ignorable`, `AlternateContent`, or an `extLst` integration for base-reader
  compatibility.

The settings writer's `w:compat/w:compatSetting` with
`w:name="compatibilityMode"`, `w:uri="http://schemas.microsoft.com/office/word"`,
`w:val="15"` selects a Microsoft editing feature set ([MS-DOCX][ms-docx]
§2.3.5); it does not change XML namespaces or select a standard
conformance class, does not substitute for MCE, and is independent
of `--compat`.

## Package and relationship invariants

The ZIP entry `word/document.xml` represents OPC part
`/word/document.xml`; `[Content_Types].xml` is a stream, not a part.
Write one entry per part (no OPC-equivalent duplicates), with legal part
names. Each part gets a media type via `Types/Default[@Extension]` or
`Types/Override[@PartName]` (absolute part name; override wins). The
stream's namespace is
`http://schemas.openxmlformats.org/package/2006/content-types`.
The declared type must match the part's actual format. See [Part 2][p2]
§§6.2, 7.2.3.2–7.2.3.5, 7.3.4–7.3.7
and `opc-contentTypes.xsd`.

A `.rels` part uses `http://schemas.openxmlformats.org/package/2006/relationships`
as its default namespace. `Relationships` owns `Relationship` children,
each with unqualified `Id` (XML ID, unique **in that source's `.rels`**),
`Type` (absolute IRI) and `Target` (IRI reference). `TargetMode` defaults to
`Internal`; write `External` for external hyperlinks. Resolve an internal
target against the **source part's** URI, never the `.rels` part URI.
`media/image3.png` from `/word/header1.xml` resolves to
`/word/media/image3.png`, not `/word/_rels/media/image3.png`. A relationship
from the package itself lives in `/_rels/.rels` and resolves relative
targets from `/`. A source part `/path/name.xml` uses
`/path/_rels/name.xml.rels`. Never reuse an ID from another story unless the
same ID actually appears in the owning story's `.rels`. Relationship order
and the spelling `rIdN` are allocator choices, not OOXML semantics
([Part 2][p2] §§6.4.2, 6.5.2–6.5.3 and `opc-relationships.xsd`;
[Part 1][p1] §22.8.2.1 for the relationship reference).

| Writer inventory | Ownership and media types |
|---|---|
| Package root | `officeDocument` → `/word/document.xml`, media type `application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml`; `metadata/core-properties` → `/docProps/core.xml`, media type `application/vnd.openxmlformats-package.core-properties+xml`. Core-property type URI uses the OPC relationship prefix. |
| Main document | Internal `styles`, `fontTable`, `settings`, optional `numbering`, `footnotes`, `header`, `footer`, and `customXml`. Use the matching schema part's media type (`application/vnd.openxmlformats-officedocument.wordprocessingml.{styles,fontTable,settings,numbering,footnotes,header,footer}+xml` for the listed WML support parts). Each `w:headerReference`/`w:footerReference` has matching `w:type` (`default`, `even`, `first`) and main-document `r:id`. `w:titlePg` in the relevant section and `w:evenAndOddHeaders` in settings enable the corresponding first/even slots. |
| Drawing in a body, footnote, header or footer | `r:embed` resolves in the `.rels` of the part containing the `w:drawing`, even inside a table cell, hyperlink or text box. Picture types end in `/image`, use Internal targets and declared `image/png`, `image/jpeg`, or [`image/svg+xml`][iana-svg] as applicable. `wp:docPr/@id` is **not** a relationship ID or media index. |
| External links | `w:hyperlink/@r:id` resolves in the containing story to a `/hyperlink` relationship, `TargetMode="External"` for external URLs; a plain bookmark link or `HYPERLINK` field is not such a reference. |
| Bibliography data | Main document → `/customXml/item1.xml` (`customXml`, default `application/xml`); that part → `itemProps1.xml` (`customXmlProps`, `application/vnd.openxmlformats-officedocument.customXmlProperties+xml`). Preserve source `Tag` values referenced by fields. An embedded XSL copy does not install a Word bibliography style. |

The Transitional relationship type prefix in this table (except the OPC
core-properties type) is
`http://schemas.openxmlformats.org/officeDocument/2006/relationships/`.
Source: [Part 1][p1] §§11.3.3, 11.3.5–7, 11.3.9–12, 15.2.5–6,
15.2.14, 15.3, 17.10, 22.6; [Part 4][p4] §§9.2, 13.2–13.3 and Annex A;
[Part 2][p2] §8 for OPC core properties. `.rels` uses
`application/vnd.openxmlformats-package.relationships+xml`.

For story parts use the WML roots `w:document/w:body`, `w:hdr`, `w:ftr`,
and `w:footnotes/w:footnote`, with paragraph/block content in their
declared groups (`CT_Document`, `CT_Body`, `CT_HdrFtr`, `CT_Footnotes`
in `wml.xsd`). Keep header/footer references on `w:sectPr`, ahead of
page settings. When emitting `w:pgMar`, supply **all** of
`w:top`, `w:right`, `w:bottom`, `w:left`, `w:header`, `w:footer` and
`w:gutter`: `CT_PageMar` requires them. Top/bottom use signed twips;
the other fields use nonnegative lengths. The writer uses each
section's explicit header/footer distances, or the conventional 720
twips from the example in [Part 1][p1] §17.6.11 when unspecified. It
sets the binding gutter to zero because the model does not supply a
binding offset. This does not affect column spacing (`w:cols/@w:space`).
See also [Part 1][p1] §§17.6.17, 17.10.

Omitting a default footer reference allows inheritance from the previous
section ([Part 1][p1] §17.10.2). When recognized page numbering is disabled
after a section with a generated footer, the writer uses a reference to an
empty footer instead; before any numbered section there is nothing to
inherit.

Every explicit relationship reference must resolve to the correct type
and target. An internal target must exist with the expected content type.

[ecma]: https://ecma-international.org/publications-and-standards/standards/ecma-376/
[p1]: https://ecma-international.org/wp-content/uploads/ECMA-376-1_5th_edition_december_2016.zip
[p2]: https://ecma-international.org/wp-content/uploads/ECMA-376-2_5th_edition_december_2021.zip
[p3]: https://ecma-international.org/wp-content/uploads/ECMA-376-3_5th_edition_december_2015.zip
[p4]: https://ecma-international.org/wp-content/uploads/ECMA-376-4_5th_edition_december_2016.zip
[od]: https://learn.microsoft.com/en-us/openspecs/office_standards/ms-odrawxml/06cff208-c6e1-4db7-bb68-665135e5f0de
[ms-docx]: https://learn.microsoft.com/en-us/openspecs/office_standards/ms-docx/b839fe1f-e1ca-4fa6-8c26-5954d0abbccd
[iana-svg]: https://www.iana.org/assignments/media-types/image/svg+xml
