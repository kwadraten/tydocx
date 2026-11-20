#set text(font: "Libertinus Serif", size: 11pt)
#set page(paper: "a4")

#let item(body) = {
  set par(hanging-indent: 2em)
  body
}

Normal paragraph.

#item[Hanging item body text here.]
