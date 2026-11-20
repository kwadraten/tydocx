// `#show footnote.entry: set par(...)` — the entry element is realized in
// the page's footnote area, so its declared first-line-indent/justify are
// read from source and pinned per footnote paragraph.
#set text(font: "Libertinus Serif", size: 11pt)
#set par(justify: true, first-line-indent: (amount: 2em, all: true))
#show footnote.entry: set par(first-line-indent: 0pt, justify: false)

Body text.#footnote[A footnote entry that stays left aligned.]
