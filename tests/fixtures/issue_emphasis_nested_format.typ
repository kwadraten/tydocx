// #20: emphasis wrapping nested formatting must keep the inner flags.
// Before the fix, handle_emphasis_tag overwrote the runs with bold/italic
// only, dropping the superscript/smallcaps the Content walk had extracted.
_Emph with #super[sup] inside_ and *strong with #smallcaps[sc] inside*.
