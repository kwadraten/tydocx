// Conformance M4: #grid is not lowered to a Word table — its text is
// recovered as plain paragraphs joined by tab stops.
#grid(
  columns: (1fr, 1fr, 1fr),
  gutter: 1em,
  [Cell A1], [Cell B1], [Cell C1],
  [Cell A2], [Cell B2], [Cell C2],
)
