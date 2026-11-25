// Faithful transcription of the @preview/i-figured:0.2.4 show rules (the
// package fetch hangs; source verified against the Universe mirror). The
// show rules wrap each labelled figure/equation in a NEW inner element
// carrying a prefixed label (fig:/eqt:), so both labels are live: REFs to
// the prefixed alias previously dangled because only the outer label got a
// bookmark.
#let _prefix = "i-figured-"

#let _typst-numbering = numbering
#let _prepare-dict(it, level, zero-fill, leading-zero, numbering) = {
  let numbers = counter(heading).at(it.location())
  while zero-fill and numbers.len() < level { numbers.push(0) }
  if numbers.len() > level { numbers = numbers.slice(0, level) }
  if not leading-zero and numbers.at(0, default: none) == 0 {
    numbers = numbers.slice(1)
  }

  let dic = it.fields()
  let _ = if "body" in dic { dic.remove("body") }
  let _ = if "label" in dic { dic.remove("label") }
  let _ = if "counter" in dic { dic.remove("counter") }
  dic + (numbering: n => _typst-numbering(numbering, ..numbers, n))
}

#let show-figure(
  it,
  level: 1,
  zero-fill: true,
  leading-zero: true,
  numbering: "1.1",
  extra-prefixes: (:),
  fallback-prefix: "fig:",
) = {
  if type(it.kind) == str and it.kind.starts-with(_prefix) {
    it
  } else {
    let figure = figure(
      it.body,
      .._prepare-dict(it, level, zero-fill, leading-zero, numbering),
      kind: _prefix + repr(it.kind),
    )
    if it.has("label") {
      let prefixes = (table: "tbl:", raw: "lst:") + extra-prefixes
      let new-label = label(prefixes.at(
          if type(it.kind) == str { it.kind } else { repr(it.kind) },
          default: fallback-prefix,
      ) + str(it.label))
      [#figure #new-label]
    } else {
      figure
    }
  }
}

#let show-equation(
  it,
  level: 1,
  zero-fill: true,
  leading-zero: true,
  numbering: "(1.1)",
  prefix: "eqt:",
  only-labeled: false,
  unnumbered-label: "-",
) = {
  if (
    only-labeled and not it.has("label")
    or it.has("label") and (
      str(it.label).starts-with(prefix)
      or str(it.label) == unnumbered-label
    )
    or not it.block
  ) {
    it
  } else {
    let equation = math.equation(
      it.body,
      .._prepare-dict(it, level, zero-fill, leading-zero, numbering),
    )
    if it.has("label") {
      let new-label = label(prefix + str(it.label))
      [#equation #new-label]
    } else {
      let new-label = label(prefix + _prefix + "no-label")
      [#equation #new-label]
    }
  }
}

#show figure: show-figure
#show math.equation: show-equation

= Chapter

#figure(rect(width: 2cm, height: 1cm), caption: [A figure]) <my-figure>

$ x = y $ <my-equation>

See @fig:my-figure and @eqt:my-equation.
