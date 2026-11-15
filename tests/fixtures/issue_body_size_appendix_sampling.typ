// Test: the run-style body-size baseline samples the same first pages as the
// document-style detector (#27). The appendix (9pt, most of the document's
// glyphs) must not move the baseline away from the 10.5pt body: body runs
// carry no size override. Before the fix the baseline came from all pages,
// so the appendix dominated and every body run got a spurious size override.
#set text(font: "Liberation Serif", size: 10.5pt)

Body marker sentence for the sampling test.

#lorem(1800)

#set text(size: 9pt)

#lorem(4000)
