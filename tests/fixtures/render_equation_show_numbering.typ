#set page(paper: "a4", margin: (left: 30mm, right: 31.4mm, top: 28mm, bottom: 28mm))
#set text(font: "Libertinus Serif", size: 12pt)
#set math.equation(supplement: none)
#set heading(numbering: "1.")

// Re-emit an equation under a new label, including the repeated default
// label that is legal until used in a reference.
#show math.equation.where(block: true): it => {
  let name = if it.has("label") { str(it.label) } else { "automatic" }
  if name == "-" or name.starts-with("display:") {
    return it
  }
  let chapter = counter(heading).at(it.location()).first()
  let new-label = label("display:" + name)
  [
    #math.equation(it.body, block: true, numbering: n => numbering("(1.1)", chapter, n))
    #new-label
  ]
}

= Equations

Forward: @display:first.

$ x + y = z $ <first>
$ x + y = z $

Unnumbered:

$ x + y = z $ <->

Backward: @display:first.

$ x + y = z $
