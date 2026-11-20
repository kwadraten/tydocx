// Paragraph-local `par` overrides: a paragraph whose own StyleChain resolves
// a first-line-indent or justification that differs from the document-global
// Normal style carries an explicit w:ind / w:jc override.
#set text(font: "Libertinus Serif", size: 11pt)
#set par(justify: true, first-line-indent: (amount: 2em, all: true))

Body paragraph inherits the document indent and justification.

#block[
  #set par(first-line-indent: 0pt, justify: false)
  A block-scoped paragraph opts out of both values.
]

#block[
  #set par(first-line-indent: 12pt)
  A twelve-point absolute first-line indent.
]

#show figure.caption: it => {
  set par(first-line-indent: 0pt, justify: true)
  align(left, it)
}

#figure(rect(width: 50pt, height: 30pt), caption: [A caption body.])
