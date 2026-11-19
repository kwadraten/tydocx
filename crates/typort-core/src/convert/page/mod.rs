//! `PagedDocument` style and page-layout extraction.
//!
//! The submodules mirror the converter's three information sources: rendered
//! frame detection, authoritative source-AST rules, and helpers that reconcile
//! those values onto the document model.

mod body_size;
mod hanging_indent;
mod language;
mod margin;
mod page_ref;
mod reachable;
mod run_style;
mod source_ast;
mod style;
mod units;

use super::{frames, stats};

pub use hanging_indent::{ParHangingRule, collect_par_hanging_indent_rules};
pub use language::{canonicalize_font_names, localize_cjk_fonts};
pub use margin::{
    MarginZone, MarginsPt, ZoneParts, detect_multi_format_page_numbering, detect_page_numbering,
    extract_page_settings, extract_zone_parts, page_number_format_from_declaration,
};
pub use reachable::{collect_reachable_source_texts, extract_import_paths};
pub use run_style::apply_styles_from_paged;
pub(crate) use run_style::{effective_style, effective_weight, extract_non_black_color};
pub use source_ast::{
    PageNumberingDecl, SourceStyleOverrides, extract_show_template_names_from_source,
    extract_source_style_overrides, footnote_counter_reset_in_page_header, transform_bodies_text,
};
pub use style::extract_document_style;

pub(crate) use language::is_cjk_char;
pub(super) use language::{is_cjk_language, lang_region_to_bcp47};
pub(super) use margin::header_footer_variant;
pub(super) use margin::numbering_page_format;
pub(super) use page_ref::{CounterUse, classify_counter_use, counter_page_label};
pub(super) use source_ast::detect_page_setting_change;
pub(super) use style::apply_footnote_text_size;
pub(super) use units::pt_to_twips;
