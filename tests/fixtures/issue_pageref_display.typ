#set page(numbering: "1")

= Introduction <intro>

Some text on the first page.

#pagebreak()

The current page is #context counter(page).display().

Here is #context counter(page).at(here()).

One more than the introduction page: #context (counter(page).at(<intro>).first() + 1).

Footnote count: #context counter(footnote).display().
