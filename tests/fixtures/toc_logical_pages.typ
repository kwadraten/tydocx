#set page(width: 420pt, height: 400pt, margin: (left: 42pt, right: 48pt, top: 40pt, bottom: 42pt))
#set text(font: "Libertinus Serif", size: 12pt)
#set par(first-line-indent: 1.5em)
#set heading(numbering: "1.1")

Cover leaf.

#pagebreak()
#counter(page).update(1)
#set page(numbering: "I", margin: (left: 52pt, right: 58pt, top: 40pt, bottom: 42pt))

#outline(depth: 3, indent: 12pt, title: [Navigation Index])

#pagebreak()
= Front target <front-target>
Front matter body.

#pagebreak()
#counter(page).update(1)
#set page(numbering: "1")

= Main target <main-target>
Main matter body. See @front-target.

#pagebreak()
== Inner target
Inner matter body.

#pagebreak()
=== Final target
Final matter body.
