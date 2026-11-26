#let extended(body) = context {
  set page(columns: 2)
  body
}

Front matter before the long abstract.
#pagebreak()
#extended[
  Extended abstract column marker.

  #colbreak()
  Extended abstract continuation marker.
]
