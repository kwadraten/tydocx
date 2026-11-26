#let render-case = [
  See @float-target.

  #place(top, float: true)[
    #figure(rect(width: 40pt, height: 15pt), caption: [Float anchor]) <float-target>
  ]
]
#if "render-case" in sys.inputs { render-case }
