// REF inside a container body: Typst cannot register labels inside table
// cells at all (cell bodies are layouter closures that never reach the root
// flow — "label does not exist in the document"), so this pins the other
// direction — the REF emitted inside a cell, targeting a top-level label.
#figure(rect(width: 2cm, height: 1cm), caption: [Out figure])<outfig>

#table(
  columns: 1,
  [See @outfig from a cell.],
)
