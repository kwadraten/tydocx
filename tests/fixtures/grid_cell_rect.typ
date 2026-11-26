#let field(body, stroke: none) = rect(
  width: 100%,
  inset: (x: 0pt, bottom: 1pt),
  stroke: stroke,
  body,
)
#let line = (bottom: 0.5pt + rgb("#13579B"))

#align(center, block(width: 318pt, grid(
  columns: (72pt, 1fr, 72pt, 1fr),
  column-gutter: -3pt,
  row-gutter: 11.5pt,
  [Plain label],
  field[10304260814203019984],
  rect(width: 45pt, radius: 4pt, fill: rgb("#D9E1F2"))[Rounded],
  [Tail],
  [Spanning label],
  grid.cell(colspan: 3, align: center + bottom, field(stroke: line)[Spanning value]),
)))

#block(width: 318pt, grid(
  columns: (1fr, 1fr, 1fr),
  rows: (auto, auto, auto, 12pt),
  [One],
  [Two],
  [Three],
  grid.cell(colspan: 2, field(stroke: line)[Planned value]),
  [Four],
  grid.cell(colspan: 2, stroke: (bottom: 1pt + red), field(stroke: line)[Own edge]),
  field(stroke: line)[Shared edge],
  grid.cell(x: 2, y: 3, stroke: (top: 1pt + green))[Neighbor],
))

#rect(width: 100%, inset: 0pt, stroke: none)[Outside grid]
