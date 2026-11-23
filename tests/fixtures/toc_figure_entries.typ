// A figure-targeted outline must carry cached entries (caption text +
// PAGEREF) behind a TOC \c "Figure" field, not only the title.
#set text(font: "Libertinus Serif")

#outline(title: [List of Figures], target: figure)

#figure(rect(width: 2cm, height: 1cm), caption: [First shape])

#figure(rect(width: 2cm, height: 1cm), caption: [Second shape])
