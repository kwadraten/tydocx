// `figure(placement:)` with a lone `table` body — the float lowers to a
// `w:tblpPr` floating table (the elsevier-cas-unofficial idiom); the caption
// travels with the table as a grid-spanning borderless row.
#set text(font: "Libertinus Serif")

#figure(
  placement: top,
  caption: [Floating results],
  table(
    columns: 3,
    [Method], [Score], [Rank],
    [A], [0.9], [1],
    [B], [0.8], [2],
  ),
)

Body text after the figure.
