#set page(width: 180pt, height: 160pt, margin: 20pt)
#set text(font: "Libertinus Serif", size: 10pt)
#set par(leading: 0.65em)

Before.

#pad(top: 12pt, bottom: 18pt, left: 8pt, right: 6pt)[Padded text.]

#block(above: 10pt, below: 12pt)[
  #pad(top: 2pt, bottom: 3pt)[Nested padded text.]
]

#pad(top: 20pt, bottom: 22pt)[
  #table(columns: 1)[Cell]
  Paragraph after a leading table.
]

After.

#pagebreak()

#[
  #set par(spacing: 30pt, leading: 0pt)

  #block(above: 0pt, below: 12pt)[#pad(bottom: 3pt)[Explicit gap before B.]]
  #block(above: 0pt)[B follows the explicit gap.]

  #block(above: 0pt, below: 12pt)[
    #pad(bottom: 2pt)[#pad(bottom: 3pt)[Two physical bottom insets.]]
  ]
  #block(above: 0pt)[After two bottom insets.]

  #block(above: 0pt, below: 12pt)[
    #block(above: 0pt, below: 7pt)[#pad(bottom: 3pt)[Explicit inner gap.]]
    #block(above: 0pt)[#pad(bottom: 4pt)[Trailing padded paragraph.]]
  ]
  #block(above: 0pt)[After the trailing paragraph.]
]
