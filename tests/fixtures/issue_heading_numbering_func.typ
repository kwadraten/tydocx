// Issue L2: function numbering cannot map to a w:numFmt — the number Typst
// rendered is kept as literal text and a heading-numbering degradation is
// reported. The number is never dropped.
#set heading(numbering: (..nums) => [Sec.#nums.pos().sum()])

= Alpha

= Beta
