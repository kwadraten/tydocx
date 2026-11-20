// `figure(scope: "parent")` in a two-column page floats across the column
// bounds at page top. This pins the current behavior — an anchored float at
// the measured page position, not real column spanning.
#set page(columns: 2)
#set text(font: "Libertinus Serif")

#figure(
  placement: top,
  scope: "parent",
  caption: [Wide results],
  table(columns: 2, [a], [b], [1], [2]),
)

#lorem(80)
