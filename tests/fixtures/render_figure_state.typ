#let render-case = [
  #let settings = state("settings", none)
  #settings.update((kind: [Theorem context]))
  #figure(kind: "statement", supplement: [Theorem], numbering: "1", context settings.get().kind)
]
#if "render-case" in sys.inputs { render-case }
