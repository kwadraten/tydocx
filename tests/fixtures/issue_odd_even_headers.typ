#set text(font: "Libertinus Serif", size: 11pt)
#set page(
  paper: "a4",
  margin: (top: 20mm, bottom: 20mm),
  header: context {
    if calc.even(counter(page).get().first()) {
      align(left, text(9pt)[Even Title #h(1em) #counter(page).display()])
    } else {
      align(right, text(9pt)[Odd Title #h(1em) #counter(page).display()])
    }
  },
)

Page one body.

#pagebreak()

Page two body.

#pagebreak()

Page three body.
