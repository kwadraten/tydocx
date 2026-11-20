#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompatMode {
    C1,
    #[default]
    C2,
    C3,
}

impl CompatMode {
    #[must_use]
    pub fn from_short_name(name: &str) -> Option<Self> {
        match name {
            "c1" => Some(Self::C1),
            "c2" => Some(Self::C2),
            "c3" => Some(Self::C3),
            _ => None,
        }
    }

    #[must_use]
    pub const fn short_name(self) -> &'static str {
        match self {
            Self::C1 => "c1",
            Self::C2 => "c2",
            Self::C3 => "c3",
        }
    }

    #[must_use]
    pub const fn is_c1(self) -> bool {
        matches!(self, Self::C1)
    }

    #[must_use]
    pub const fn is_c3(self) -> bool {
        matches!(self, Self::C3)
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
