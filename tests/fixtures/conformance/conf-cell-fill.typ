// Conformance M2: table cell fill has no w:shd output — TableCell carries
// no shading field.
#table(
  columns: 2,
  fill: (x, y) => if x == 0 { rgb("#ddeeff") },
  [Header A], [Header B],
  [Cell 1], [Cell 2],
)
