#set text(font: "Libertinus Serif", size: 11pt)
#set page(
  paper: "a4",
  background: [
    #place(left + top, dx: 10mm, dy: 12mm, rect(width: 40mm, height: 12mm, fill: rgb("c8102e")))
    #place(left + top, dx: 10mm, dy: 40mm)[BG-STAMP]
    #place(right + top, dx: -10mm, dy: 60mm, image("test_image.png", width: 20mm))
  ],
)

Body text here.
