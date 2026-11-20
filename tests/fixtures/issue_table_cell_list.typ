// Test: lists inside table cells must keep their items as separate list
// paragraphs with w:numPr, not merge into one plain-text paragraph
// (upstream #31).

#table(
  columns: 2,
  [Name], [Items],
  [Alice], [- one
- two],
  [Bob], [+ first
+ second],
  [Carol], [
    - outer
      - inner
  ],
)
