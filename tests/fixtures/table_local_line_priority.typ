// A local red hline must win over the blue cell stroke on the edges it
// covers — Typst paints explicit lines above cell borders.
#set text(font: "Libertinus Serif")

#table(
  columns: 2,
  stroke: none,
  table.cell(stroke: (bottom: 4pt + blue))[A1], [B1],
  table.hline(y: 1, stroke: 1pt + red),
  [A2], [B2],
)
