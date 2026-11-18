// Conformance M9: the overline is emitted as a floating line shape
// (wp:anchor + wps line preset geometry) above the text line — the same
// object Typst itself draws — not as an underline.
#set text(font: "Libertinus Serif")

Before #overline[overlined text] after.
