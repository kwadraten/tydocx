#set page(header: context {
  let hs = query(selector(heading).before(here()))
  if hs.len() > 0 { [Report — #hs.last().body] } else { [Report] }
})
= Intro
Body text.
#pagebreak()
More body.
