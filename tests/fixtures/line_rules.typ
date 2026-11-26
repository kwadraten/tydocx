// Cover-style rules: a 200%-wide `#line` must NOT collapse to a
// column-width paragraph border — it takes the DrawingML path. An
// in-width styled line folds to `w:pBdr` keeping its stroke (w:sz/w:color),
// never a substituted `double`. The `#v(-2pt)` between the wide lines
// cannot map and must report a spacing degradation, not drop silently.
#set page(width: 10cm, height: 10cm, margin: 1cm)

Before.

#line(length: 200%)

#v(-2pt)

#line(length: 200%)

#line(length: 100%, stroke: 2pt + red)

#line(length: 100%)

#line(length: 100%, stroke: (dash: "dashed", thickness: 1pt))

#line(length: 100%, start: (10%, 0pt))

#line(length: 100%, stroke: 1.5pt + gradient.linear(red, blue))

#line(length: 100%, stroke: (thickness: 1.5pt, cap: "round"))

After.
