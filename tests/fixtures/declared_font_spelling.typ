#import "declared_font_spelling_support.typ": imported-text

#set text(font: ("TyDocx Missing Body", "LIBERTINUS SERIF"))
#show heading: set text(font: "TyDocx Missing Heading")

= Heading

#imported-text[Imported styling.]

#text(fill: black)[TyDocx Not a Font Declaration]
#let ignore(font: "") = none
#ignore(font: "TyDocx Not a Font Declaration")
