// Under `par(justify: true)` Typst keeps a `#linebreak()`'s line
// non-stretching by default; Word stretches `w:br`-terminated lines under
// `w:jc="both"`, so the paragraph pins `w:jc="left"` — except a
// `linebreak(justify: true)` stays stretchable.
#set text(font: "Libertinus Serif", size: 11pt)
#set par(first-line-indent: 0pt, justify: true)

Editors:#linebreak() Jeanne Féaux de la Croix and#linebreak() Beatrice Penati

A wrapped paragraph that fills and justifies on its own.

Left line.#linebreak(justify: true) Stretched line.
