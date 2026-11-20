// A lone `table` inside a non-floating `#place` lowers to a `w:tblpPr`
// positioned table rather than a text box.
#set text(font: "Libertinus Serif")

#place(top + left, dx: 30pt, dy: 40pt)[
  #table(columns: 2, [key], [value], [alpha], [1])
]

Body text.
