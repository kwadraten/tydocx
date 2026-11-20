#set page(header: context {
  let hs = query(selector(heading).before(here()))
  if hs.len() > 0 { hs.last().body }
})

= Intro

First page body.

#pagebreak()

Second page body.
