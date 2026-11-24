#set page(numbering: "i")

= Start <start>

#pagebreak()

#pagebreak()

Current: #context counter(page).display().

Upper: #context counter(page).display("I").

Back to start: #context counter(page).display(at: <start>).

Both: #context counter(page).display("1 / 1", both: true).

Total: #context counter(page).final().

Start state: #context counter(page).at(<start>).first().
