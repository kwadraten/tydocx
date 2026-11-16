// A function-valued page numbering has no `w:pgNumType` equivalent: the
// format falls back to glyph detection and the declaration is reported.
#set page(numbering: n => "(" + str(n) + ")")

#lorem(150)
#pagebreak()
#lorem(150)
