#set page(margin: (top: 65pt, bottom: 66pt, x: 70pt))

Unnumbered cover.

#pagebreak(weak: true)
Unnumbered declaration.

#set page(numbering: "I")
#counter(page).update(1)

Roman front matter.

#set page(footer: none)
Number tracked but hidden.

#set page(footer: auto)
Number visible again.

#set page(numbering: "1")
#counter(page).update(1)
Arabic main matter.
