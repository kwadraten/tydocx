#let render-case = [
  #show figure.where(kind: "proof"): it => it.body
  #show ref: it => {
    if it.element != none and it.element.func() == figure and it.element.kind == "proof" {
      let data = query(selector(<stamp>).after(it.target)).first()
      link(it.target, data.value)
    } else {
      it
    }
  }

  #metadata([Earlier metadata must not match]) <stamp>

  #figure(kind: "proof", supplement: [Proof], outlined: false)[
    #block[
      First proof body. #metadata([First resolved proof]) <stamp>
    ]
  ] <first-proof>

  #figure(kind: "proof", supplement: [Proof], outlined: false)[
    #block[
      Second proof body. #metadata([Second resolved proof]) <stamp>
    ]
  ] <second-proof>

  References: @first-proof and @second-proof.
]
#if "render-case" in sys.inputs { render-case }
