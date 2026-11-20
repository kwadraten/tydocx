// Non-floating `#place` overlays: a placed image anchors behind text, placed
// text stays an editable text box, a placed rect keeps its fill — none of
// them displace the flow.
#set text(font: "Libertinus Serif", size: 11pt)
#set page(paper: "a4", margin: 0pt)

Cover line before.

#place(top + left, dx: 0pt, dy: 0pt)[#image("test_image.png", width: 200pt, height: 100pt)]

#place(top + left, dx: 40pt, dy: 40pt)[Placed Cover Text]

#place(top + left, dx: 0pt, dy: 100pt)[#rect(width: 200pt, height: 30pt, fill: rgb(127, 143, 131))]

After text.

#image("test_image.png", width: 2cm)

Trailing line.
