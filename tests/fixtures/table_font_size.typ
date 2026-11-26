// `show table: set text(size: 9pt)` lands on the pairs INSIDE the table's
// tag group — the tag pair's own chain stays at the body size. Cell runs
// must carry the scoped `w:sz`, not inherit the page default.
#set text(size: 11pt)
#show table: set text(size: 9pt)

Body text stays 11pt.

#table(columns: 2, [Name], [Value])
