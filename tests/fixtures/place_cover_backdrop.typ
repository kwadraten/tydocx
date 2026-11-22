// Cover geometry like the manuscript's: a band rect, then two title text
// boxes above it. Each box must carry the band's fill (so it covers what
// paints under it) and keep its measured extent — never the page-sized
// box emitted for LibreOffice's benefit.
#set text(font: "Libertinus Serif", size: 11pt)
#set page(width: 200pt, height: 200pt, margin: 20pt)

#place(top + left, dx: 0pt, dy: 60pt)[#rect(width: 200pt, height: 30pt, fill: rgb(127, 143, 131))]

#place(top + left, dx: 40pt, dy: 68pt)[Earlier title]

#place(top + left, dx: 30pt, dy: 66pt)[Cover title]
