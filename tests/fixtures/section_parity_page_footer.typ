#set page(margin: (inside: 70pt, outside: 60pt, top: 65pt, bottom: 50pt))
#let generated-title(body) = heading(outlined: false, body)

Cover leaf.

#pagebreak(weak: true, to: "odd")
Declaration leaf.

#pagebreak()
#pagebreak()
#counter(page).update(1)
#set page(numbering: "I")

#generated-title[Front title]
Preface leaf.

#pagebreak(weak: true, to: "odd")
#generated-title[Contents title]
Contents leaf.

#pagebreak()
#pagebreak()
#counter(page).update(1)
#set page(numbering: "1")
#show heading.where(level: 1): it => {
  pagebreak(weak: true)
  it
}

= First chapter
The first chapter body.

= Second chapter
The second chapter body.
