// Out-of-scope pins for the literal route: a numbering pattern OOXML cannot
// map ("①") leaves every item without `number_text`, and a reversed enum
// nested under a `w:numPr` parent keeps the numbering path. Both stay on
// `w:numPr` — forward-numbered — and both report `enum-reversed`.
#set enum(reversed: true, numbering: "①")
+ alpha
#enum.item(9)[beta]
+ gamma

+ outer a
  #set enum(reversed: true)
  + inner x
  + inner y
  + inner z
+ outer b
