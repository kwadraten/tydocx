#set text(font: "Libertinus Serif", size: 11pt)
#set page(
  paper: "a4",
  foreground: place(left + top, dx: 10mm, dy: 10mm, context {
    if counter(page).get().first() == 1 [FIRSTPAGE] else [RESTPAGE]
  }),
)

First body.
#pagebreak()
Second body.
