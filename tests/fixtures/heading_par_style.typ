// A scoped `#set par(first-line-indent: 0pt)` inside a show rule must reach
// the heading paragraph as `w:ind`, exactly as it does for body paragraphs.
// The manuscript's `show heading.where` helper wraps `it.body` in a block
// carrying the set rule.
#set text(font: "Libertinus Serif", size: 11pt)
#set par(first-line-indent: 2em, justify: true)
#show heading.where(level: 1): it => block(above: 1.6em, below: 1.4em, breakable: false)[#set par(first-line-indent: 0pt); #text(size: 20pt, weight: 630, it.body)]

= Chapter One

A body paragraph inherits the document indent.

A later body paragraph inherits too.
