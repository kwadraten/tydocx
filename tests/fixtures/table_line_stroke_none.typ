// An explicit `stroke: none` hline removes that boundary: the red table
// stroke must not paint edges where the line says none.
#set text(font: "Libertinus Serif")

#table(
  columns: 2,
  stroke: 1pt + red,
  table.hline(y: 1, stroke: none),
  [A1], [B1],
  [A2], [B2],
)
