// Conformance M1: enum numbering patterns are dropped — all ordered lists
// render as decimal "%1." (see lists.rs, numbering.rs).
#set enum(numbering: "a)")
+ First item
+ Second item

#set enum(numbering: "i.")
+ Third item
+ Fourth item
