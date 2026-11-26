// A table.cell whose body is a lone `#rect` — the cover-page label/value
// pattern — folds the rect's bottom stroke onto `tcBorders` and its inset
// onto `tcMar`, keeping the inner content as plain `w:p`/`w:r` runs. No
// `wps:txbx` text box may appear inside the table. The second table's
// gradient-filled rect still folds (tcBorders) but must report a
// `cell-rect` degradation — a non-solid fill has no `w:shd`.
#set page(width: 10cm, height: 10cm, margin: 1cm)

#table(
  columns: (30%, 70%),
  table.cell[#rect(width: 100%, stroke: (bottom: 1pt + black), inset: 8pt)[Label]],
  table.cell[#rect(width: 100%, stroke: (bottom: 1pt + black), inset: 8pt)[Value]],
)

#table(
  columns: (100%),
  table.cell[#rect(width: 100%, stroke: (bottom: 1pt + black), fill: gradient.linear(red, blue))[Shaded]],
)
