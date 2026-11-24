#set page(footer: context {
  let hs = query(selector(heading).before(here()))
  if hs.len() > 0 { [#hs.last().body — end] }
})
= Intro
Body text.
#pagebreak()
More body.
