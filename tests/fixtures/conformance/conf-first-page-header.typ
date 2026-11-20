// Conformance M7: the document model has a single header — a first-page
// header cannot differ from the running header.
#set page(
  header: context if counter(page).get().first() == 1 {
    align(center)[*First-page header*]
  } else {
    align(center)[Running header]
  },
)

#lorem(500)
