// Two footnotes each carrying a different image: each footnote's drawing
// must embed its own image (footnote 1 -> rId1/media image1, footnote 2 ->
// rId2/media image2), not the first image twice.
#set text(font: "Libertinus Serif")

First body text.#footnote[Red image: #image("test_image_red.png", width: 2cm)]

Second body text.#footnote[Blue image: #image("test_image_blue.png", width: 4cm)]
