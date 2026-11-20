// A drafting-style margin note: a `#place`d box that sits entirely inside
// the page's left margin anchors `relativeFrom="margin"` so it tracks the
// text-area edge.
#set page(
  width: 400pt,
  height: 300pt,
  margin: (left: 100pt, right: 30pt, top: 30pt, bottom: 30pt),
)
#set text(font: "Libertinus Serif", size: 10pt)

#place(top + left, dx: -80pt, dy: 20pt)[
  #box(width: 70pt, inset: 3pt)[#text(size: 7pt)[Margin note text]]
]

Body text of the page.
