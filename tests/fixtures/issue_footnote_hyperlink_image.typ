// A footnote carrying an external hyperlink and an image: the link must
// survive as a hyperlink (not plain text) and the image as an embedded
// drawing in footnotes.xml.
#set text(font: "Libertinus Serif")

Body text with a footnote.#footnote[See #link("https://example.com")[the example site] and this image: #image("test_image.png", width: 3cm)]
