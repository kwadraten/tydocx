// Bare shapes draw as real DrawingML at C2/C3: preset geometry where Word
// has one, `custGeom` paths for polygons and curves, `wps:txbx` bodies.
#set page(width: 10cm, height: 20cm, margin: 1cm)
#set text(font: "Libertinus Serif")

#rect(width: 4cm, height: 2cm, fill: rgb("#aaccff"), radius: 2mm)[Rect body text]

#square(size: 1cm, fill: green)

#circle(radius: 5mm, stroke: 2pt + red)

#ellipse(width: 3cm, height: 1cm, fill: blue)

#polygon((0pt, 0pt), (30pt, 10pt), (60pt, 0pt), fill: yellow, stroke: 1pt + black)

#curve(
  curve.move((0pt, 0pt)),
  curve.line((40pt, 0pt)),
  curve.cubic((60pt, 20pt), (80pt, -20pt), (100pt, 0pt)),
  stroke: 1.5pt + blue,
)

#line(start: (0pt, 0pt), end: (2cm, 1cm), stroke: 1pt + black)

#line(length: 100%)

= Heading with #rect(width: 1em, height: 0.5em, fill: orange) inline

#rect(width: 2cm, height: 1cm, fill: gradient.linear(red, blue))

Trailing paragraph.
