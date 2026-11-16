// C1 U5/U6/U19/U24/U25: figure placement, outline target/indent,
// list marker, reversed enum, image fit/alt.
#figure(placement: bottom, image("test_image_red.png", width: 3cm), caption: [Red square])

#outline(target: figure)

#outline(indent: 2em)

#set list(marker: [--])
- alpha
- beta

#set enum(reversed: true)
+ one
+ two

#image("test_image_red.png", width: 3cm, fit: "contain", alt: "red square")
