#let steps = counter(figure.where(kind: "proof"))
#show figure.where(kind: "proof"): it => it.body
#steps.update(())
#figure(kind: "proof", supplement: [Proof], context [
  Proof number #steps.at(here()).first().
]) <example>

Body after proof.
