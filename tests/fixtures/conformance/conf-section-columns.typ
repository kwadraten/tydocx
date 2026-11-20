// Conformance M6: a mid-document column change produces no section break —
// detect_section_breaks only watches page size.
#set page(columns: 1)
#lorem(40)

#set page(columns: 2)
#lorem(40)
