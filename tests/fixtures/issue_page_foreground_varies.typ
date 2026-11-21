#set text(font: "Libertinus Serif", size: 11pt)
#set page(
  paper: "a4",
  foreground: place(left + top, dx: 10mm, dy: 10mm, context [
    PAGE #counter(page).get().first() MARK
  ]),
)

First body.
#pagebreak()
Second body.
#pagebreak()
Third body.
