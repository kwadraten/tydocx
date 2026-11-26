#show math.equation: it => {
  if not it.block or it.has("label") and str(it.label).starts-with("inner:") {
    it
  } else if it.has("label") {
    let new-label = label("inner:" + str(it.label))
    [#math.equation(it.body, block: true, numbering: "(1)") #new-label]
  } else {
    it
  }
}

$ x = 1 $ <balance>

First reference: @inner:balance. Second reference: @inner:balance.
