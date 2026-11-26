#set page("a4", margin: (x: 89pt, y: 69pt))
#set text(font: "Libertinus Serif", size: 15pt)

#let mark = bytes("<svg xmlns='http://www.w3.org/2000/svg' width='58' height='72'><rect width='58' height='72' fill='purple'/></svg>")

#let declaration() = {
  v(6pt)
  align(center, image(mark, width: 58pt))
  v(-12pt)
  align(center, text(size: 24pt, weight: "bold", "Declaration of authorship\nStatement of integrity"))
  v(48pt)

  block[
    #set text(size: 15pt)
    #set par(justify: true, first-line-indent: (amount: 2em, all: true), leading: 2.42em)

    Ledger number 101 records the original work submitted by the author. All sources that informed the analysis are credited where their work is used. The author has checked the record and accepts responsibility for any error. This document sets out the terms under which the record may be reviewed. Each entry identifies its author and its date.
  ]
  v(76pt)

  grid(
    columns: (1fr, 150pt),
    [],
    align(left)[
      #set text(size: 15pt)

      Signature:

      Record number:

      Date of signature:
    ],
  )
}

#declaration()

#pagebreak()

Ordinary *styled* #text(fill: blue)[words] and another run
on a new source line in the same paragraph.

Explicit first#linebreak()Explicit second

#block[
  #set par(leading: 1em + 2pt, spacing: 3em)
  Scoped first paragraph.

  Scoped second paragraph.
]

#table(columns: 1, [Cell *styled* words before a #text(fill: blue)[boundary

  Cell second] continues here.

  #par[Cell third]])

#par(leading: 1em + 2pt)[With a tall inline image #box(image(mark, width: 58pt)) on this line.]
