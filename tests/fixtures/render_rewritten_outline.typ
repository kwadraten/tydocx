#set heading(numbering: "1")
#show heading.where(level: 1): it => box[§ #counter(heading).display() #it.body]
#outline()
= First chapter
Some body.
= Second chapter
More body.
