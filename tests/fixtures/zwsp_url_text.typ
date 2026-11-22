// Typst's URL break hints (zero-width space / word joiner) reach the
// realized text verbatim; Word finds its own break opportunities, so the
// DOCX path drops them rather than showing gaps after each `/`, `.`, `_`.
#set text(font: "Libertinus Serif", size: 11pt)

The archived source is #link("https://en.wikipedia.org/")[https:#sym.zws/#sym.zws en.#sym.zws wikipedia.#sym.zws org/#sym.zws archive] and a#sym.wj joined#sym.wj pair.
