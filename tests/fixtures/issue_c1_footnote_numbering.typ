// C1 U26: footnote numbering "(n)" has no OOXML equivalent.
#set footnote(numbering: n => "(" + str(n) + ")")
Text with a footnote.#footnote[First note]

More text.#footnote[Second note]
