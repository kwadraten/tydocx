// Declared page numbering, no footer: the `pgNumType` format must come
// from the declaration ("i" → lowerRoman), and the declaration alone must
// not conjure a footer part. (Without `footer: none`, Typst renders its
// default numbered footer, so the declaration would not be "alone".)
#set page(numbering: "i", footer: none)

#lorem(150)
#pagebreak()
#lorem(150)
