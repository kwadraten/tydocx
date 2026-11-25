// The reviewer's repro: all 32 citation hyperlinks in the paper's C2 DOCX
// concatenated to "[". Built-in gb-7714-2015-numeric wraps the numeral in a
// link element inside the rendered marker, which the old pair-walk marker
// extraction cut off at the first nested Tag::End.
#set text(lang: "zh")
Linpack#cite(<linpack>);基准;
#bibliography("citation_gb7714.bib", style: "gb-7714-2015-numeric", full: true)
