// Outline title tri-state: `auto` resolves the localized default ("Contents"
// under lang "en"), explicit `none` emits no title block, custom content is
// kept verbatim. Each heading-styled title is `outlined: false` — it must
// carry w:outlineLvl=9 so a TOC field update skips it.
#set text(font: "Libertinus Serif", size: 11pt)
#set page(paper: "a4")

#outline()

#pagebreak()

#outline(title: none)

#pagebreak()

#outline(title: [My Custom Contents])

#pagebreak()

= Alpha

Body text.
