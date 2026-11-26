#set page(numbering: "I")
#let first-kind = figure.where(kind: "first")
#let second-kind = figure.where(kind: "second")

#context {
  for target in (first-kind, second-kind) {
    if query(target).len() > 0 {
      outline(title: none, target: target)
    }
  }
}

#pagebreak()
#figure([First specimen body], kind: "first", supplement: [Sample], caption: [Zygote index marker])
#pagebreak()
#figure([Second specimen body], kind: "second", supplement: [Sample], caption: [Quartz index marker])
