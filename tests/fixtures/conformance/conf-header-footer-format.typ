// Conformance L3: header/footer content is extracted from the first page's
// margin zones as plain text runs — font, size, colour and bold are lost.
#set page(
  header: [*Bold header* with #text(fill: red)[red text]],
  footer: [Footer note],
)

#lorem(60)
