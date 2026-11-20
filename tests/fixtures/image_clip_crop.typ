// `block(clip: true)` + negative `box(inset:)` — the justwhitee-notes
// `#crop` idiom — and the lynx-style `block(clip: true, inset: (bottom: …))`
// form. The image keeps its full extent; the clip window shows a
// sub-rectangle → `a:srcRect`.
#set text(font: "Libertinus Serif")

#block(clip: true)[
  #box(inset: (top: -20pt, bottom: -10pt, left: -10pt, right: -50pt))[
    #image("test_image.png", width: 200pt, height: 100pt, fit: "stretch")
  ]
]

#block(clip: true, inset: (bottom: -30pt))[
  #image("test_image_blue.png", width: 200pt, height: 100pt)
]
