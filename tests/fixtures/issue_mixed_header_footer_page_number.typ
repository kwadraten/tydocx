// Mixed margin zones: the header carries a section title after the page
// number ("1 标题"), the footer a "Page 1" label. Both page numbers must
// become PAGE fields, the surrounding text stays literal.
#set page(header: [#context counter(page).display() 标题])
#set page(footer: [Page #context counter(page).display()])

#lorem(150)
#pagebreak()
#lorem(150)
#pagebreak()
#lorem(150)
