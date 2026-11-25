// Circled footnote marks produced by a numbering FUNCTION, not a pattern —
// the evaluated marker sequence (①②③) decides decimalEnclosedCircle.
#set text(font: "Libertinus Serif", size: 11pt)
#set page(paper: "a4")
#set footnote(numbering: n => "①②③④⑤⑥⑦⑧⑨⑩".clusters().at(n - 1))

One.#footnote[first] Two.#footnote[second] Three.#footnote[third]
