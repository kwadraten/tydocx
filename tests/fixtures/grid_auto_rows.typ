// An ordinary auto-sized grid is not a timetable: its rows must not carry
// the planner's `trHeight exact` — Word reflows row heights itself.
#set text(font: "Libertinus Serif", size: 11pt)

#grid(
  columns: (2.4em, 1fr, 2.4em),
  column-gutter: 0.5em,
  [01], [Alpha entry wraps onto a second line when narrow], [x1],
  [02], [Beta], [x2],
)
