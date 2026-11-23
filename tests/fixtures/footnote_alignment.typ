// A footnote with two paragraphs carrying different alignments: each
// footnote paragraph must keep its own w:jc in footnotes.xml.
#set text(font: "Libertinus Serif")

Body text.#footnote[
#align(center)[Centered paragraph.]

#align(right)[Right paragraph.]
]
