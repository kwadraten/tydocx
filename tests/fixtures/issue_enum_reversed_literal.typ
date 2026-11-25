// A reversed enum's `w:numPr` can only count forward, so a reversed enum
// whose numerals the collector rendered (`number_text`) is literal at every
// compat level — the faithful output — and still reports `enum-reversed`.
#set enum(reversed: true)
+ first
+ second
+ third

#set enum(reversed: true, numbering: "1.")
+ alpha
+ beta
+ gamma
