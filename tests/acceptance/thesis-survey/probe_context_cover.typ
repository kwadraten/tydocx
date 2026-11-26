#let mark = context {
  let use-alternate = query(<config>).first().value
  if use-alternate [Alternate cover mark] else [Primary cover mark]
}
#metadata(false) <config>
#set page(header: none)
#grid(columns: (1fr,), mark)

Body after cover.
