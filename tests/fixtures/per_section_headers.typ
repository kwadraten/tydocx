// Per-section header parts (HPL-AI): the cover section carries no header
// while the abstract and body sections each render their own header, so the
// mid-body w:sectPr and the final w:sectPr must reference different
// word/headerN.xml parts and the cover's sectPr none at all.
#set text(font: "Libertinus Serif", size: 11pt)
#set page(paper: "a4", margin: 40mm)

Cover page without a header.

#pagebreak()
#set page(
  margin: (left: 20mm, right: 20mm),
  header: text(9pt)[Abstract Header],
)

Abstract body.

#pagebreak()
#set page(
  margin: (left: 15mm, right: 15mm),
  header: text(9pt)[Body Header],
)

Body text.
