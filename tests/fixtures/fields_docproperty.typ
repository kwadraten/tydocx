#set document(
  title: "Specimen",
  author: "An Author",
  keywords: ("alpha", "beta"),
  description: "A description",
  date: datetime(year: 2021, month: 3, day: 4),
)

#context document.title

#context document.author.join(", ")

#context document.keywords.join(", ")

#context document.description

#context document.date.display("[year]")
