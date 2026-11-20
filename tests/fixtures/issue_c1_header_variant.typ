// C1 M7: the header text changes on page 2 — C1 keeps page 1's header.
#set page(header: [First header])
First page content.

#pagebreak()
#set page(header: [Second header])
Second page content.
