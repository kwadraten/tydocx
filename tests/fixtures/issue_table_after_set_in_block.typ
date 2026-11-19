#set text(font: "Libertinus Serif", size: 11pt)
#block(above: 8pt, below: 10pt, breakable: false)[
#set text(size: 9.5pt)
#table(columns: (1fr, 52pt, 40pt),
  inset: (x: 4pt, y: 5pt), align: (x, y) => if x == 0 { left } else { right },
  stroke: none, table.hline(stroke: 0.75pt),
  [],
  [#text(weight: 630)[Km2]],
  [#text(weight: 630)[%]],
  table.hline(stroke: 0.4pt),
  [#text("Row label")], [#text("481,513")], [#text("44.2")],
)
]
