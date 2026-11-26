#set page(width: 255pt, height: 465pt, margin: 24pt)
#set text(font: "Libertinus Serif", size: 12pt)
#outline(title: none)

Citations @smith2020 and @knuth1997.

#[
  #show heading: it => align(center, block(text(fill: rgb("#365A21"))[Register: #it.body]))
  #show grid.cell.where(x: 0): it => text(fill: rgb("#802020"), if it.y == 0 { "[i]" } else { "[WIDE-12]" })
  #show grid.cell.where(x: 1): it => {
    if it.y == 0 [
      Rewritten article [S]. An editable entry with enough words to span several lines.
      #text("https://example.org/literal") and
      #link("https://example.org/live")[#text(fill: rgb("#375A20"))[Live destination]].
    ] else [
      Rewritten book. The second reference also spans several lines to test the same
      body column with a wider marker.
    ]
  }
  #bibliography("bibliography_basic.bib", title: [Research register], style: "ieee")
]
