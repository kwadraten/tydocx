// One body image, then a footnote holding a linked image followed by a
// plain image. Both footnote images must embed footnote-local relationship
// ids resolving through word/_rels/footnotes.xml.rels to their own media
// parts — a link wrapper must not re-route the image through
// document.xml.rels.
#set text(font: "Libertinus Serif")

Body image: #image("test_image.png", width: 2cm)

Body text.#footnote[Linked image: #link("https://example.com")[#image("test_image_red.png", width: 1cm)] Plain image: #image("test_image_blue.png", width: 1cm)]
