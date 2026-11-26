#show figure.where(kind: "proof"): it => it.body
#show ref: it => {
  if it.element != none and it.element.func() == figure and it.element.kind == "proof" {
    let data = query(selector(<stamp>).after(it.target)).first()
    link(it.target, data.value)
  } else {
    it
  }
}

#figure(kind: "proof", supplement: [Proof], outlined: false)[
  #block[
    Proof body marker. #metadata([Resolved proof]) <stamp>
  ]
] <proof>

Reference: @proof.
