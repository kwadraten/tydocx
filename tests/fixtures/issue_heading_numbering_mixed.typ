// Issue: a document mixing numbering styles — a mappable heading numbering
// followed by an unmappable one. The mappable headings get Word numbering
// (w:numPr); the last keeps its literal number with a degradation.
#set heading(numbering: "1.1")

= Second

== Second One

#set heading(numbering: "一、")

= 附录
