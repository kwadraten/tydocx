// Conformance M3: #place with float has no wp:anchor output — the writer
// only emits wp:inline; placed content survives only via recovery.
#place(top + right, float: true)[*Floated callout*]

#lorem(40)
