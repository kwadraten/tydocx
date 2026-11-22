// A margin-less cover page whose whole content is body `#place` calls —
// like the manuscript's cover. With a declared `page(foreground:)`, the
// furniture classifier must not sweep the cover's places into the
// first-page header: they are body content, anchored once in
// `word/document.xml`. The declared foreground keeps painting on the
// ordinary pages that follow.
#set text(font: "Libertinus Serif", size: 11pt)
#set page(
  width: 200pt,
  height: 200pt,
  margin: 20pt,
  foreground: place(top + left, dx: 150pt, dy: 30pt)[#text(size: 6pt)[DOI-MARK]],
)

#[
  #set page(margin: 0pt, header: none, footer: none)
  #place(top + left, dx: 0pt, dy: 0pt)[#image("test_image.png", width: 200pt, height: 200pt, fit: "cover")]
  #place(top + left, dx: 0pt, dy: 60pt)[#rect(width: 200pt, height: 30pt, fill: rgb(127, 143, 131))]
  #place(top + left, dx: 0pt, dy: 120pt)[#rect(width: 200pt, height: 40pt, fill: white)]
  #place(top + left, dx: 40pt, dy: 68pt)[Cover title]
  #place(top + left, dx: 30pt, dy: 130pt)[Cover author]

  #metadata((kind: "source", id: "b001")) <source-block>
  #place(top + left, dx: 150pt, dy: 160pt)[
    #metadata((kind: "image", id: "logo")) <logo>
    #image("test_image_red.png", width: 30pt)
  ]

  #pagebreak()
]

#pagebreak()

Body page after the cover.
