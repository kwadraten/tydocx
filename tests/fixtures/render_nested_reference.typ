#let render-case = [
  #set heading(numbering: "1")

  #columns(2)[
    = Nested chapter <nested-chapter>

    See @nested-chapter.
  ]
]
#if "render-case" in sys.inputs { render-case }
