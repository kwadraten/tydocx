//! Compatibility modes (C1–C3) for the lowering.
//!
//! - **C1** (minimal): only the Typst↔OOXML intersection is compiled; the
//!   rest is skipped per aspect, with a `Degradation` per skipped aspect.
//!   Text is never silently dropped.
//! - **C2** (graphical, the default): tables, shapes, and other tricks
//!   approximate the union's appearance, semantics first.
//! - **C3** (dynamic): C2 plus field codes for content Word can recompute.
//!
//! The modes form a strict inclusion C1 ⊂ C2 ⊂ C3. This turn's lowering
//! treats all three modes identically except for the per-aspect C1 gates
//! the collector records; porting the legacy path's Phase 1 gates onto the
//! IR path is a follow-up (see `docs/fallback-registry.md`).

/// The compatibility mode of one conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompatMode {
    /// Minimal compatibility: intersection only.
    C1,
    /// Graphical compatibility: tricks for the union's appearance.
    #[default]
    C2,
    /// Dynamic compatibility: C2 plus recomputable field codes.
    C3,
}

impl CompatMode {
    /// Parse a CLI short name (`"c1"`, `"c2"`, `"c3"`).
    #[must_use]
    pub fn from_short_name(name: &str) -> Option<Self> {
        match name {
            "c1" => Some(Self::C1),
            "c2" => Some(Self::C2),
            "c3" => Some(Self::C3),
            _ => None,
        }
    }

    /// The CLI short name of this mode.
    #[must_use]
    pub const fn short_name(self) -> &'static str {
        match self {
            Self::C1 => "c1",
            Self::C2 => "c2",
            Self::C3 => "c3",
        }
    }

    /// True for the minimal mode.
    #[must_use]
    pub const fn is_c1(self) -> bool {
        matches!(self, Self::C1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_name_round_trips() {
        for (name, mode) in [
            ("c1", CompatMode::C1),
            ("c2", CompatMode::C2),
            ("c3", CompatMode::C3),
        ] {
            assert_eq!(CompatMode::from_short_name(name), Some(mode));
            assert_eq!(mode.short_name(), name);
        }
        assert_eq!(CompatMode::from_short_name("minimal"), None);
        assert_eq!(CompatMode::default(), CompatMode::C2);
    }
}
