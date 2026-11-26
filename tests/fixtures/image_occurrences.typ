#let badge(w, h: auto) = image("test.svg", width: w, height: h)
#set page(header: place(left + top, badge(12pt)))
#set text(size: 12pt)
#set par(first-line-indent: (amount: 2em, all: true), justify: true)

Ordinary indented prose.

= Before the picture

#badge(20pt)

#align(center, badge(44pt, h: 22pt))

#pagebreak()

#figure(
  align(right, badge(32pt, h: 48pt)),
  caption: [Same source, different crop],
)
