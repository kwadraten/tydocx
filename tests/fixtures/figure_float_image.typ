// A floating figure whose body is NOT a lone table keeps the anchored
// text-box treatment (`wp:anchor` + `wps:txbx`).
#set text(font: "Libertinus Serif")

#figure(
  placement: top,
  caption: [A floating image],
  image("test_image.png", width: 3cm),
)

Body text.
