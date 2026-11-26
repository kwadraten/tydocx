#let render-case = [
  #columns(2)[
    See @example.
    #show grid.cell.where(x: 1): set text(fill: rgb("#224422"))
    #bibliography(bytes("example:\n  type: Book\n  title: Evidence title\n  author: Example, Pat\n  date: 2020\n"), title: [Nested sources], style: "ieee")
  ]
]
#if "render-case" in sys.inputs { render-case }
