#let section(title, body) = {
  heading(numbering: none, title)
  body
}

#outline(title: none)

#context {
  for (heading-title, index-title, kind) in (
    ([First section], [First inventory], "first"),
    ([Second section], [Second inventory], "second"),
  ) {
    section(heading-title, outline(title: index-title, target: figure.where(kind: kind)))
  }
}

#{
  show outline: set heading(outlined: true)
  outline(title: [Visible index], target: figure.where(kind: "first"))
}

#figure([First sample], kind: "first", supplement: [Sample], caption: [First caption])
#figure([Second sample], kind: "second", supplement: [Sample], caption: [Second caption])
