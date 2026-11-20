#table(
  columns: 2,
  fill: (col, row) => if row == 0 { rgb("#ddeeff") },
  [Header A], [Header B],
  [Body A], [Body B],
)

#table(
  columns: 2,
  [Plain A], table.cell(fill: rgb("#ffcccc"))[Shaded B],
)

#table(
  columns: 2,
  fill: gradient.linear(rgb("#ff0000"), rgb("#0000ff")),
  [Gradient A], [Gradient B],
)
