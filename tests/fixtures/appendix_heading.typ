// Appendix numbering: a set rule switches numbering mid-document. A fresh
// numbering instance whose counter starts at the evaluated value maps to
// w:numPr exactly; a counter update off that sequence cannot — the heading
// keeps its literal prefix, its bookmark, and reports the approximation.
// Body and TOC share the effective alignment (centered here).
#set text(font: "Libertinus Serif", size: 11pt)
#set page(paper: "a4")

#outline()

#show heading: set align(center)
#set heading(numbering: "1.")

= Body Chapter

#counter(heading).update(0)
#set heading(numbering: "A.")

= Appendix One

#counter(heading).update(4)

= Appendix Two
