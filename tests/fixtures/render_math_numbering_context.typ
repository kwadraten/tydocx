#let section = counter("section")
#section.update(2)
#set math.equation(numbering: number => context numbering("(1.1)", section.get().first(), number))

Equation reference: @balance.

$ x = 1 $ <balance>

Final editable paragraph.
