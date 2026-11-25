// Symbol footnote marks ("*","†") map to no w:numFmt — each footnote keeps a
// literal custom mark (customMarkFollows) on both the body reference and the
// note head, and the fallback reports a footnote-numbering degradation.
#set text(font: "Libertinus Serif", size: 11pt)
#set page(paper: "a4")
#set footnote(numbering: "*")

Star.#footnote[star note] Dagger.#footnote[dagger note]
