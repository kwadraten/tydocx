// A line that ends mid-span of a merged cell cannot be honoured at its
// declared extent — widening to cell edges is reported as a degradation.
#set text(font: "Libertinus Serif")

#table(
  columns: 3,
  stroke: none,
  table.cell(colspan: 2)[A1], [B1],
  table.hline(y: 1, start: 1, end: 3, stroke: 1pt + red),
  [A2], [B2], [C2],
)
