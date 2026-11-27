//! Shared inline-formatting state for every walker that produces [`Run`]s.
//!
//! Three walks used to carry their own formatting flags (the HTML walk's
//! `InlineFmt`, the Content-tree walk's `InlineCtx`, and positional bools in
//! the footnote walk), each hand-copying its subset onto `Run` fields — a
//! drift class where adding a flag to one walk silently missed the others.
//! This module is the single definition: one flag struct, one tag-transition
//! function, one `Run`-application point.

use crate::ooxml::document::Run;

/// Inline formatting accumulated while walking HTML nodes or the Typst
/// `Content` tree. `Copy`, so walkers thread it by value.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
// Independent formatting flags mirroring `Run`'s rPr booleans; a state machine
// or bitflags would obscure the 1:1 mapping `apply_to` relies on.
#[allow(clippy::struct_excessive_bools)]
pub(super) struct InlineFmt {
    pub bold: bool,
    pub italic: bool,
    pub monospace: bool,
    pub smallcaps: bool,
    pub superscript: bool,
    pub subscript: bool,
}

impl InlineFmt {
    /// Formatting acquired by descending into an element with this tag name.
    /// Accepts HTML tag names and the Typst element names ("strong"/"emph")
    /// used by the introspection-Tag walker — one method serves both walkers
    /// because the HTML tree and the Tag stream carry the same formatting
    /// vocabulary under different spellings.
    pub fn for_tag(self, tag: &str) -> Self {
        Self {
            bold: self.bold || tag == "strong" || tag == "b",
            italic: self.italic || tag == "em" || tag == "i" || tag == "emph",
            monospace: self.monospace || tag == "code",
            ..self
        }
    }

    pub fn bold() -> Self {
        Self {
            bold: true,
            ..Self::default()
        }
    }

    pub fn italic() -> Self {
        Self {
            italic: true,
            ..Self::default()
        }
    }

    /// Copy every flag onto a [`Run`]. The single place formatting state
    /// reaches a `Run` — new flags added to this struct cannot be silently
    /// dropped by one of the walkers.
    pub fn apply_to(self, run: &mut Run) {
        run.bold = self.bold;
        run.italic = self.italic;
        run.monospace = self.monospace;
        run.smallcaps = self.smallcaps;
        run.superscript = self.superscript;
        run.subscript = self.subscript;
    }

    /// Merge these flags onto a [`Run`] without clearing flags it already
    /// carries. Use this when the run was produced by another walk (e.g.
    /// `extract_runs`, which preserves nested smallcaps/super/sub) and this
    /// `InlineFmt` is an *additional* layer (e.g. emphasis wrapping the
    /// content). `apply_to` instead overwrites every flag and would clobber
    /// the inner formatting — see #20.
    pub fn or_into(self, run: &mut Run) {
        run.bold |= self.bold;
        run.italic |= self.italic;
        run.monospace |= self.monospace;
        run.smallcaps |= self.smallcaps;
        run.superscript |= self.superscript;
        run.subscript |= self.subscript;
    }
}

#[cfg(test)]
mod tests {
    use super::InlineFmt;

    #[test]
    fn for_tag_accumulates_and_preserves() {
        let fmt = InlineFmt::default().for_tag("strong");
        assert!(fmt.bold && !fmt.italic);
        let fmt = InlineFmt::default().for_tag("emph"); // Typst tag-name spelling
        assert!(fmt.italic);
        let fmt = InlineFmt::default().for_tag("code");
        assert!(fmt.monospace);
        assert_eq!(InlineFmt::default().for_tag("span"), InlineFmt::default());
        // Flags for_tag doesn't set survive the transition.
        let fmt = InlineFmt {
            smallcaps: true,
            superscript: true,
            ..InlineFmt::default()
        }
        .for_tag("b");
        assert!(fmt.bold && fmt.smallcaps && fmt.superscript);
    }

    #[test]
    fn apply_to_covers_every_flag() {
        let fmt = InlineFmt {
            bold: true,
            italic: true,
            monospace: true,
            smallcaps: true,
            superscript: true,
            subscript: true,
        };
        let mut run = crate::ooxml::document::Run::new("x");
        fmt.apply_to(&mut run);
        assert!(
            run.bold
                && run.italic
                && run.monospace
                && run.smallcaps
                && run.superscript
                && run.subscript
        );
    }

    #[test]
    fn or_into_preserves_existing_flags() {
        let fmt = InlineFmt::italic();
        let mut run = crate::ooxml::document::Run::new("x");
        run.monospace = true;
        run.superscript = true;
        fmt.or_into(&mut run);
        assert!(run.italic && run.monospace && run.superscript && !run.bold);
        // apply_to would have cleared the inner flags — the reason or_into
        // exists (see #20).
        let mut run = crate::ooxml::document::Run::new("x");
        run.monospace = true;
        fmt.apply_to(&mut run);
        assert!(!run.monospace);
    }
}
