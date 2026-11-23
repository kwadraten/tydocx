// With a row gutter, `hline(y: 0, position: bottom)` sits *below* the
// first row (in the gutter), not above it — after gutter collapse the
// edge belongs to the boundary between row 1 and row 2.
#set text(font: "Libertinus Serif")

#table(
  columns: 2,
  row-gutter: 1em,
  stroke: none,
  [A1], [B1],
  table.hline(y: 0, position: bottom, stroke: 1pt + red),
  [A2], [B2],
)
