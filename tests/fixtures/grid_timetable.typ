#set page(width: 16cm, height: 11cm, margin: 1cm)
#set text(size: 8pt, font: "Libertinus Serif")

= Weekly Timetable

A timetable-style grid: a labelled header row, hour-block labels spanning
four 15-minute slot rows each, event cells spanning slot rows/columns with
fills, and hour separator lines. Empty slots have no declared cells.

#grid(
  columns: (1.8cm, 1fr, 1fr, 1fr, 1fr, 1fr),
  rows: (0.7cm,) + ((0.3cm,) * 16),
  inset: 2pt,
  grid.header(
    [Time], [Mon], [Tue], [Wed], [Thu], [Fri],
  ),
  grid.cell(x: 0, y: 1, rowspan: 4, align: center + horizon)[08:00],
  grid.cell(x: 0, y: 5, rowspan: 4, align: center + horizon)[09:00],
  grid.cell(x: 0, y: 9, rowspan: 4, align: center + horizon)[10:00],
  grid.cell(x: 0, y: 13, rowspan: 4, align: center + horizon)[11:00],
  grid.cell(
    x: 1, y: 1, rowspan: 6, colspan: 2,
    fill: rgb("b8d4f0"), align: center + horizon,
  )[Algebra],
  grid.cell(
    x: 4, y: 2, rowspan: 4,
    fill: rgb("c8e6c9"), align: center + horizon,
  )[Physics],
  grid.cell(
    x: 3, y: 9, rowspan: 3, colspan: 2,
    fill: rgb("ffe0b2"), align: center + horizon,
  )[Lab session],
  grid.cell(
    x: 2, y: 14, rowspan: 2,
    fill: rgb("f8bbd0"), align: center + horizon,
  )[PE],
  grid.hline(y: 1, stroke: 0.8pt + black),
  grid.hline(y: 5, stroke: 0.8pt + black),
  grid.hline(y: 9, stroke: 0.8pt + black),
  grid.hline(y: 13, stroke: 0.8pt + black),
)
