// A centered standalone image paragraph must keep its `w:jc` alignment —
// `image_paragraph` used to build the paragraph without the paragraph
// properties `collect_paragraph` applies.
#set page(width: 10cm, height: 10cm, margin: 1cm)

#align(center)[#image("test_image.png", width: 2cm)]
