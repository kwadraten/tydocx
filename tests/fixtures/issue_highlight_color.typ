// #24: highlight fill passes through — the declared color, not hardcoded yellow.
// Default fill stays Word yellow; a gradient fill has no run-level OOXML
// equivalent and falls back to yellow; an arbitrary solid fill becomes
// character shading with the exact hex; fill: none emits no highlight at all.
// (Typst's `blue` keyword is #0074D9, not #0000FF, so the named-blue case
// uses an explicit rgb value.)
#highlight[default text] #highlight(fill: rgb("#ffcccc"))[pink text] #highlight(fill: rgb(0, 0, 255))[blue text] #highlight(fill: gradient.linear(red, blue))[gradient text] #highlight(fill: none)[plain text]
