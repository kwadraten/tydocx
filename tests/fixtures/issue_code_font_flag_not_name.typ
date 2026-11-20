// Test: code-font detection reads the font's MONOSPACE flag (#26), not its
// family name. The code face here is "Zed Sans" — a monospace face whose name
// contains none of the old substrings ("mono", "courier", "consol",
// "fira code", "source code"); the old name check would have missed it and
// fallen back to "Courier New". Font: tests/fonts/ZedSans.ttf, a printable
// ASCII subset of DejaVu Sans Mono renamed for this test.
#set text(font: "Liberation Serif", size: 10.5pt)

Ordinary body prose, set in the proportional body face. Several sentences
of it, so the body font dominates the rendered font histogram and the code
face is only ever the code face. The quick brown fox jumps over the lazy
dog while the body text keeps flowing across the page.

#show raw: set text(font: "Zed Sans")

Here is `inline code` in the middle of a sentence.

#raw("let x = 42;")
