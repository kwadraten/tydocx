// Conformance M5: simple shapes have no DrawingML output — bare shapes are
// dropped (only figure canvases are rasterized to PNG).
#rect(width: 5cm, height: 2cm, fill: rgb("#aaccff"))[Text inside a box]

#line(length: 6cm, stroke: 2pt + red)

#circle(radius: 1cm, fill: green)
