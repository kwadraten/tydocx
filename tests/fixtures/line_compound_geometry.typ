#set page(width: 420pt, height: 840pt, margin: 30pt)
#set text(font: "Libertinus Serif")

#let horizontal(len: 83pt, stroke: 1.25pt + maroon) = line(length: len, stroke: stroke)
#let vertical() = line(end: (0pt, 44pt), stroke: 0.75pt + blue)

Aligned horizontal

#stack(horizontal(), v(2.5pt), horizontal())

Aligned vertical

#stack(dir: ltr, spacing: 1.5pt, vertical(), vertical())

Different length

#stack(horizontal(), v(2.5pt), horizontal(len: 74pt))

Different paint

#stack(horizontal(), v(2.5pt), horizontal(stroke: 1.25pt + green))

Another exact gap

#stack(horizontal(), v(3pt), horizontal())

Overlapping strokes

#stack(horizontal(), v(1pt), horizontal())

Nonflat caps

#let rounded = horizontal(stroke: (paint: maroon, thickness: 1.25pt, cap: "round"))
#stack(rounded, v(2.5pt), rounded)

Longitudinal offset

#stack(
  horizontal(),
  v(2.5pt),
  line(start: (2pt, 0pt), end: (85pt, 0pt), stroke: 1.25pt + maroon),
)

Triple group

#stack(horizontal(), v(2.5pt), horizontal(), v(2.5pt), horizontal())

End of line examples
