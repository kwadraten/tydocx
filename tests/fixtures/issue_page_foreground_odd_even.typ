#set text(font: "Libertinus Serif", size: 11pt)
#set page(
  paper: "a4",
  foreground: place(left + top, dx: 10mm, dy: 10mm, context {
    if calc.odd(counter(page).get().first()) [ODD] else [EVEN]
  }),
)

First body.
#pagebreak()
Second body.
#pagebreak()
Third body.
