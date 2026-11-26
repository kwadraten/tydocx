#show heading.where(level: 1): it => {
  pagebreak(weak: true)
  it
}

= First chapter
A paragraph on the first chapter page.

= Second chapter
A paragraph on the second chapter page.

#pagebreak()

= Third chapter
A paragraph after an already explicit page break.
