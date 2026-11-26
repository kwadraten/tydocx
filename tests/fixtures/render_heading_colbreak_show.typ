#show heading.where(level: 1): it => {
  colbreak(weak: true)
  block(text(18pt)[Part #counter(heading).display()])
  block(text(20pt, it.body))
}
#set heading(numbering: "1")
#outline()

= Eider precise heading

One content paragraph.

= Quartz precise heading

A second content paragraph.
