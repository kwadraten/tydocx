#set page(width: 420pt, height: 840pt, margin: 30pt)
#set text(font: "Libertinus Serif")

Horizontal segment

#line(
  length: 64pt,
  stroke: (paint: rgb("#a0332380"), thickness: 2pt, dash: (3pt, 2pt), cap: "round"),
)

Vertical segment

#line(end: (0pt, 42pt), stroke: (paint: blue, thickness: 1.5pt, cap: "square"))

Diagonal segment

#line(angle: 30deg, length: 48pt, stroke: (paint: green, thickness: 0.5pt))

Decorated text

#underline[Underline run] and #strike[Strike run].

#underline(stroke: 2pt + red, extent: 1pt)[Custom underline]
#strike(stroke: 0.7pt + blue, offset: -3pt)[Custom strike]

Rounded shape

#rect(
  width: 62pt,
  height: 30pt,
  radius: 4pt,
  stroke: (paint: purple, thickness: 1.25pt, dash: (3pt, 2pt), join: "bevel"),
)[Editable body]

Native edges

#table(
  columns: 2,
  stroke: none,
  table.hline(y: 1, stroke: 1pt + red),
  table.vline(x: 1, stroke: 1pt + blue),
  [One], [Two], [Three], [Four],
)

#grid(
  columns: 2,
  stroke: none,
  grid.hline(y: 1, stroke: 0.7pt + green),
  grid.vline(x: 1, stroke: 1pt + purple),
  [Five], [Six], [Seven], [Eight],
)

Math owners

$ frac(1, 2) + sqrt(x) $

Unmapped stroke aspects

#line(length: 41pt, stroke: (thickness: 1pt, dash: (array: (3pt, 2pt), phase: 1pt)))
