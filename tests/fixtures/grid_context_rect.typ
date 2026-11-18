#set page(width: 16cm, margin: 1cm)
#set text(size: 10pt, font: "Libertinus Serif")

= Context-wrapped grid

A grid produced inside `context { … }` and wrapped in `rect` (the shape
timble's timetable uses) reaches the pairs only as an opaque layouter block —
it must still lower to a table.

#context rect(
  stroke: 0.8pt + black,
  inset: 4pt,
  grid(
    columns: (2cm, 1fr, 1fr),
    rows: (0.5cm,) + ((0.3cm,) * 4),
    grid.header([Slot], [Mon], [Tue]),
    grid.cell(x: 0, y: 1, rowspan: 2)[A1],
    grid.cell(x: 1, y: 1, colspan: 2, fill: rgb("b8d4f0"))[A2],
    grid.cell(x: 1, y: 2)[B2],
    grid.cell(x: 2, y: 2)[B3],
    grid.cell(x: 0, y: 3)[C1],
    grid.cell(x: 1, y: 3)[C2],
    grid.cell(x: 2, y: 3)[C3],
    grid.cell(x: 0, y: 4)[D1],
    grid.cell(x: 1, y: 4)[D2],
    grid.cell(x: 2, y: 4)[D3],
  ),
)
