#set page(width: 16cm, height: 12cm, margin: 1cm)
#set text(size: 10pt, font: "Libertinus Serif")
#show grid.cell: set text(size: 6pt)

#grid(
  columns: (1.6cm, 1fr, 1fr),
  rows: (auto,) + (1fr,) * 8,
  inset: 2pt,
  grid.cell(
    x: 0, y: 0, colspan: 3,
    align: center + horizon, fill: rgb("eeeeee"),
    stroke: (bottom: 0.5pt),
  )[#text(size: 16pt)[Timetable]],
  grid.cell(x: 0, y: 1, rowspan: 4, align: center + horizon)[8],
  grid.cell(
    x: 0, y: 5, rowspan: 4, align: center + horizon,
    stroke: (top: (paint: luma(170), thickness: 0.5pt, dash: "dashed")),
  )[9],
  grid.cell(
    x: 1, y: 1, rowspan: 2, fill: rgb("b8d4f0"),
    align: center + horizon, stroke: (right: 0.5pt),
  )[Alpha],
  grid.cell(
    x: 2, y: 2, rowspan: 6, fill: rgb("c8e6c9"),
    align: center + horizon, stroke: (left: 0.5pt),
  )[Beta],
)
