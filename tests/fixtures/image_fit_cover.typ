// `image(fit: "cover")` fills the declared box and clips the overflow:
// the tall target crops top+bottom of the square source, the wide target
// crops left+right.
#set text(font: "Libertinus Serif")

#image("test_image.png", width: 100pt, height: 50pt, fit: "cover")

#image("test_image_red.png", width: 50pt, height: 100pt, fit: "cover")
