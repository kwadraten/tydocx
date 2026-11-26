#set page(width: 240pt, height: 180pt, margin: 20pt)

#let rule(width, paint) = line(length: width, stroke: 1pt + paint)
#let ruled(body) = context {
  let size = measure(body)
  stack(
    dir: ttb,
    body,
    v(3pt),
    rule(size.width, red),
    v(2pt),
    rule(size.width, blue),
  )
}

#align(center, ruled(box(width: 72pt)[Measured title]))

Following body.
