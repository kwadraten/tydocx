#set page(numbering: "1")
Body one.
#pagebreak()
#counter(page).update(7)
#set page(numbering: "i")
Body two.
#context counter(page).final()
