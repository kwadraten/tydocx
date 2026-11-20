// Declared numbering ("1" → decimal) against a misleading footer glyph:
// the footer is static text "i" (a Roman-looking glyph that is NOT the
// page number). The declared value must win over glyph guessing, and the
// static footer must not become a generated page-number footer.
#set page(numbering: "1", footer: [i])

#lorem(150)
#pagebreak()
#lorem(150)
