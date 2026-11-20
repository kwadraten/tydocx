// Test: the body-size detectors weight by glyph count, not byte length (#27).
// The CJK title carries more bytes than the ASCII body text (each CJK glyph
// is three bytes), but fewer glyphs. Byte weighting would pick the title's
// size as the body size and put a spurious size override on body runs; glyph
// weighting picks the 10.5pt body size, so body runs carry none.
#set text(font: "Liberation Serif", size: 10.5pt)

= 这是一个用于测试字形加权的很长的中文标题一二三四五六七八九十甲乙丙丁

Body marker sentence for the weighting test. Short ASCII body text.
