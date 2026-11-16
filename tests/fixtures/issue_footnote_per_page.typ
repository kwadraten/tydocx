// Per-page footnote numbering: the source resets the footnote counter in
// the page header, the opt-in Typst documents for per-page numbering.
#set page(header: context counter(footnote).update(0))

#lorem(150)

First footnote here.#footnote[First page note.]

#pagebreak()

#lorem(150)

Second footnote here.#footnote[Second page note.]
