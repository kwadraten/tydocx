// Control: plain single labels — bookmarks and REFs match without any
// aliasing show rule.
#set heading(numbering: "1")
#set math.equation(numbering: "(1)")

= Chapter <chap>

#figure(rect(width: 2cm, height: 1cm), caption: [A figure])<myfig>

$ x = y $ <myeq>

See @myfig, @myeq, and @chap.
