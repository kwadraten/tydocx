// Nested imports must resolve relative to the importing file: the main file
// imports a.typ in a subdirectory, which imports b.typ next to it; b.typ's
// set rule must reach the document.
#import "issue_import_nested/a.typ": *

#set text(font: "Libertinus Serif")

Nested import body text.
