// Test: page margins in em must resolve against the body size (upstream #32),
// not a hardcoded 12pt base. 2em at 10.5pt body = 21pt = 420 twips.

#set text(size: 10.5pt)
#set page(margin: 2em)

Hello.
