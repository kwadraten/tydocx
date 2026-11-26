#let register(file, title) = bibliography(file, title: title)
#outline(title: none)

= Part One

Cites the first library @alpha2021.

#register("bibliography_multiple_a.bib", [References A])

= Part Two

Cites the second library @beta2022.

#register("bibliography_multiple_b.bib", [References B])
