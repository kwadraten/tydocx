// `#outline(target: heading.where(level: 1))` must emit its resolved
// entries, not just the title. The selector restriction is reported as a
// degradation because a field update regenerates every level.
#set text(font: "Libertinus Serif")

#outline(title: [Top Level], target: heading.where(level: 1))

= Alpha

== Beta one

= Gamma
