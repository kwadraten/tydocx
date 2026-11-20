// An import path with `..` must resolve relative to the importing file:
// d.typ lives in a subdirectory and imports the shared file one level up.
#import "issue_import_parent_dir/d.typ": *

#set text(font: "Libertinus Serif")

Parent-directory import body text.
