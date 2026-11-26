#set text(font: "Libertinus Serif")

// Both installed fonts have a glyph for every character. The explicit coverage
// sends the dash to the second family even within this single text element.
#text(font: (
  (name: "Libertinus Serif", covers: "latin-in-cjk"),
  "New Computer Modern",
  "TyDocx Missing Tail",
))[A—B]
