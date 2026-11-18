#set text(font: "Libertinus Serif", size: 11pt)
#set page(
  paper: "a4",
  margin: (top: 20mm, bottom: 20mm),
  header: context {
    if counter(page).get().first() > 1 {
      text(9pt)[Doc Title]
    }
  },
)

Page one body.

#pagebreak()

Page two body.
