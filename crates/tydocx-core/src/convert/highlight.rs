//! Mapping from a Typst highlight `fill` to its OOXML representation.
//!
//! `w:highlight` only accepts the sixteen named highlight colors, so an
//! arbitrary solid fill is emitted as character shading (`w:shd`) with the
//! exact hex instead — the same background the author declared, without
//! lossy nearest-color guessing.

use typst_library::visualize::Paint;

/// How a highlight fill is written into a run's properties.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HighlightStyle {
    /// One of OOXML's sixteen named highlight colors; written as `w:highlight`.
    Named(&'static str),
    /// Arbitrary solid color as a 6-digit uppercase hex; written as `w:shd`.
    Shading(String),
    /// No fill (`fill: none`): the run carries no highlight at all.
    None,
}

/// Typst's default highlight fill (`HighlightElem::fill` default in
/// typst-library): a semi-transparent yellow. Word's yellow highlight is its
/// equivalent — the existing tests pin `#highlight[x]` to `w:highlight`
/// yellow, and the default is yellow in intent, so it maps to the named
/// color rather than to shading with the raw hex.
const DEFAULT_FILL_HEX: &str = "#fffd11a1";

/// The sixteen `w:highlight` values (`ST_HighlightColor`) with their RGB hex.
const NAMED_HIGHLIGHTS: [(&str, &str); 16] = [
    ("000000", "black"),
    ("0000FF", "blue"),
    ("00FFFF", "cyan"),
    ("00FF00", "green"),
    ("FF00FF", "magenta"),
    ("FF0000", "red"),
    ("FFFF00", "yellow"),
    ("FFFFFF", "white"),
    ("000080", "darkBlue"),
    ("008080", "darkCyan"),
    ("008000", "darkGreen"),
    ("800080", "darkMagenta"),
    ("800000", "darkRed"),
    ("808000", "darkYellow"),
    ("808080", "darkGray"),
    ("C0C0C0", "lightGray"),
];

/// Map a highlight `fill` to its OOXML representation. Pure: no I/O, no
/// global state. The hex comparison ignores alpha, matching the existing
/// foreground-color convention (`extract_non_black_color`).
#[must_use]
pub fn highlight_style(fill: Option<&Paint>) -> HighlightStyle {
    let Some(fill) = fill else {
        return HighlightStyle::None;
    };
    let Paint::Solid(color) = fill else {
        // Gradients and tilings have no run-level OOXML equivalent; keep the
        // historical fallback instead of dropping the highlight silently.
        return HighlightStyle::Named("yellow");
    };
    let hex = color.to_hex();
    if hex.eq_ignore_ascii_case(DEFAULT_FILL_HEX) {
        return HighlightStyle::Named("yellow");
    }
    let rgb = hex
        .strip_prefix('#')
        .map_or("", |digits| &digits[..6.min(digits.len())])
        .to_uppercase();
    if let Some((_, name)) = NAMED_HIGHLIGHTS.iter().find(|(hex, _)| *hex == rgb) {
        HighlightStyle::Named(name)
    } else {
        HighlightStyle::Shading(rgb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use typst_library::visualize::Color;

    fn solid(r: u8, g: u8, b: u8, a: u8) -> Paint {
        Paint::Solid(Color::from_u8(r, g, b, a))
    }

    #[test]
    fn default_fill_maps_to_yellow() {
        assert_eq!(
            highlight_style(Some(&solid(0xFF, 0xFD, 0x11, 0xA1))),
            HighlightStyle::Named("yellow")
        );
    }

    #[test]
    fn named_colors_map_to_highlight() {
        assert_eq!(
            highlight_style(Some(&solid(0xFF, 0xFF, 0x00, 0xFF))),
            HighlightStyle::Named("yellow")
        );
        assert_eq!(
            highlight_style(Some(&solid(0x00, 0x00, 0xFF, 0xFF))),
            HighlightStyle::Named("blue")
        );
        assert_eq!(
            highlight_style(Some(&solid(0x80, 0x00, 0x00, 0xFF))),
            HighlightStyle::Named("darkRed")
        );
    }

    #[test]
    fn arbitrary_solid_color_maps_to_shading() {
        assert_eq!(
            highlight_style(Some(&solid(0xFF, 0xCC, 0xCC, 0xFF))),
            HighlightStyle::Shading("FFCCCC".into())
        );
    }

    #[test]
    fn no_fill_maps_to_none() {
        assert_eq!(highlight_style(None), HighlightStyle::None);
    }
}
