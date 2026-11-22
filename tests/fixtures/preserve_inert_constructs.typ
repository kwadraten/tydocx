// Preservation fixture: constructs tydocx emits but does not further model
// must survive a convert + save + reopen cycle unchanged. Each line carries
// one construct class — hyperlink, cross-reference bookmark + REF field,
// and a footnote reference.

#set heading(numbering: "1.")

= Preservation Targets <targets>

A paragraph with a #link("https://example.com/target")[hyperlink body] inside it.

See @targets for the heading this cross-reference points at.

A final sentence carries a footnote.#footnote[Footnote body text.]
