// An SVG image inside a footnote body: footnotes.xml must carry the
// mc:AlternateContent SVG choice + PNG fallback with its own relationship
// namespace (footnotes.xml.rels), and both media parts must ship.
#set text(font: "Libertinus Serif")

Body text.#footnote[Footnote vector: #image("tiny.svg", width: 2cm)]
