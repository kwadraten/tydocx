#let badge(paint, width: 36pt) = image(
  bytes("<svg xmlns='http://www.w3.org/2000/svg' width='60' height='40'>"
    + "<path d='M0 0H60V40H0Z' fill='" + paint + "'/></svg>"),
  format: "svg",
  width: width,
)

#set page(
  paper: "a4",
  header: place(left + top, dx: 10pt, dy: 4pt, badge("#0cab3b")),
  footer: place(left + top, dx: 10pt, dy: 4pt, badge("#1d4cda")),
)
#set par(first-line-indent: (amount: 2em, all: true))

Body text before the pictures.

#align(center, badge("#ae18ab", width: 83.455pt))

#place(right + top, float: true, badge("#e0251b"))

Body text after the pictures.
