#let theorem = counter("theorem")
#for _ in range(2) {
  theorem.step()
  figure(kind: "statement", numbering: none, context [Theorem #theorem.display()])
}
