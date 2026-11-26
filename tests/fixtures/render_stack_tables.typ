#set page(width: 420pt, height: 320pt, margin: 20pt)
#set text(size: 11pt)
#set figure.caption(position: top)

#show figure: it => {
  if it.has("label") and it.label == <stack-source> {
    let rebuilt = figure(it.body, caption: it.caption)
    [#rebuilt <stack-reemitted>]
  } else {
    it
  }
}

Before the stack: @stack-first and @stack-reemitted.

#align(center, stack(dir: ltr)[
  #figure(
    table(
      columns: 2,
      stroke: 0.5pt,
      [First cell], [A],
    ),
    caption: [First stack caption],
  ) <stack-first>
][
  #h(50pt)
][
  #set text(size: 16pt)
  #figure(
    table(
      columns: 2,
      stroke: none,
      table.hline(stroke: 0.5pt),
      [Second cell], [B],
    ),
    caption: [Second stack caption],
  ) <stack-source>
])

After the stack.
