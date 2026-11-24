/// Convert a length in points to Word twips (1 pt = 20 twips), rounded.
///
/// The single place the `f64 → u32` measurement cast is performed. Negative or
/// out-of-range inputs saturate to a valid `u32` (Rust `as` casts saturate),
/// matching the document model's unsigned twip fields.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(in crate::convert) fn pt_to_twips(pt: f64) -> u32 {
    (pt * 20.0).round().max(0.0) as u32
}

/// Typst's default `columns(gutter:)` — `Ratio::new(0.04)` on
/// `ColumnsElem::gutter`, a share of the column region's width.
pub(in crate::convert) const DEFAULT_COLUMNS_GUTTER: typst_library::layout::Rel<
    typst_library::layout::Abs,
> = typst_library::layout::Rel::new(
    typst_library::layout::Ratio::new(0.04),
    typst_library::layout::Abs::zero(),
);

/// The gutter (`w:cols w:space`) a `columns` region reports — its
/// `Rel<Abs>` ratio part resolves against the region's base width, which
/// for both `page(columns:)` and a page-level `#columns` block is the
/// text width (typst-layout `regions.base().x`).
pub(in crate::convert) fn columns_gutter_twips(
    gutter: typst_library::layout::Rel<typst_library::layout::Abs>,
    text_width_pt: f64,
) -> u32 {
    pt_to_twips(
        gutter
            .relative_to(typst_library::layout::Abs::pt(text_width_pt))
            .to_pt(),
    )
}

/// Convert a length in points to Word half-points (1 pt = 2 half-pt), rounded.
/// See [`pt_to_twips`] for the cast rationale.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(in crate::convert) fn pt_to_half_pt(pt: f64) -> u32 {
    (pt * 2.0).round().max(0.0) as u32
}

pub(super) fn numeric_to_twips(n: typst_syntax::ast::Numeric<'_>) -> u32 {
    pt_to_twips(numeric_to_pt(n))
}

pub(super) fn numeric_to_half_pt(n: typst_syntax::ast::Numeric<'_>) -> u32 {
    pt_to_half_pt(numeric_to_pt(n))
}

fn numeric_to_pt(n: typst_syntax::ast::Numeric<'_>) -> f64 {
    let (value, unit) = n.get();
    numeric_value_to_pt(value, unit)
}

pub(super) fn numeric_value_to_pt(value: f64, unit: typst_syntax::ast::Unit) -> f64 {
    match unit {
        typst_syntax::ast::Unit::Pt => value,
        typst_syntax::ast::Unit::Cm => value * 72.0 / 2.54,
        typst_syntax::ast::Unit::Mm => value * 72.0 / 25.4,
        typst_syntax::ast::Unit::In => value * 72.0,
        typst_syntax::ast::Unit::Em => value * 12.0,
        _ => 0.0,
    }
}
