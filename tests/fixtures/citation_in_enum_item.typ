// Repro of the KudoNII20 report: any citation inside an enum item body used
// to fail the whole conversion with Typst's "citation could not be located"
// (the CiteGroup is unreachable behind the enum's layouter block in the
// realize pairs). Other containers share the same feed.
+ An item citing @kudo2020.
+ Another citing @smith2019.

#bibliography("citation_shared.bib")
