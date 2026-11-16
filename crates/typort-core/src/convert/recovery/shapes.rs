//! Detection of dropped pure shapes (M5, S3).
//!
//! The line-attribution path in [`super::lines`] only sees shapes that
//! contain text. A shape with no text (`#rect` as a color band, `#circle`
//! as a bullet) leaves no text item behind, so it is invisible to recovery
//! — this pass scans the paged frames for it directly.

use std::collections::HashSet;

use typst::introspection::Tag;
use typst::layout::{Frame, FrameItem};
use typst_layout::PagedDocument;
use typst_library::text::HighlightElem;
use typst_library::visualize::Geometry;
use typst_syntax::Span;

use crate::compat::{Degradation, DegradationAction};

/// Brackets whose shapes are not dropped shapes: a table's shapes are its
/// borders and fills (the table itself is kept), a highlight's shape is the
/// background of kept text, and a grid's shapes are cell fills of a grid
/// whose loss is already reported as `grid-layout`.
enum ShapeBracket {
    Table,
    Highlight,
    Grid,
}

/// Detect pure shapes dropped by C1 (M5, S3): `Rect`/`Curve` geometry
/// shapes with a visible fill or stroke, outside table/highlight/grid
/// brackets, whose enclosing shape/box element did not already report its
/// text as dropped (`skip_spans`, the recovered lines' shape-text and
/// box-decoration attributions — without it, a rect with recovered text
/// would be reported twice).
///
/// `Line` geometry is never reported: underlines, strikethroughs, and
/// horizontal rules are either kept (as runs/rules) or already covered.
/// A shape with neither fill nor stroke draws nothing, so it is not a loss.
pub(super) fn detect_dropped_shapes(
    paged: &PagedDocument,
    skip_spans: &HashSet<Span>,
) -> Vec<Degradation> {
    let mut degradations = Vec::new();
    for (page_idx, page) in paged.pages().iter().enumerate() {
        collect_dropped_shapes(&page.frame, page_idx, skip_spans, &mut degradations);
    }
    degradations
}

fn collect_dropped_shapes(
    frame: &Frame,
    page_idx: usize,
    skip_spans: &HashSet<Span>,
    degradations: &mut Vec<Degradation>,
) {
    let mut stack: Vec<(typst::introspection::Location, ShapeBracket)> = Vec::new();
    super::super::frames::visit_frame_items(frame, true, &mut |_, item| match item {
        FrameItem::Tag(Tag::Start(content, _)) => {
            let bracket = if content
                .to_packed::<typst_library::model::TableElem>()
                .is_some()
            {
                Some(ShapeBracket::Table)
            } else if content.to_packed::<HighlightElem>().is_some() {
                Some(ShapeBracket::Highlight)
            } else if content
                .to_packed::<typst_library::layout::GridElem>()
                .is_some()
            {
                Some(ShapeBracket::Grid)
            } else {
                None
            };
            if let Some(bracket) = bracket
                && let Some(location) = content.location()
            {
                stack.push((location, bracket));
            }
        }
        FrameItem::Tag(Tag::End(location, ..)) => {
            if stack.last().is_some_and(|(top, _)| top == location) {
                stack.pop();
            }
        }
        FrameItem::Shape(shape, span) => {
            if !stack.is_empty() {
                return;
            }
            let kind = match &shape.geometry {
                Geometry::Line(_) => return,
                Geometry::Rect(_) => "rectangle",
                Geometry::Curve(_) => "curve",
            };
            if shape.fill.is_none() && shape.stroke.is_none() {
                return;
            }
            let span = if span.is_detached() {
                None
            } else {
                Some(*span)
            };
            if let Some(span) = span
                && skip_spans.contains(&span)
            {
                return;
            }
            let message = match span {
                Some(_) => format!("{kind} dropped"),
                None => format!("{kind} dropped (page {})", page_idx + 1),
            };
            degradations.push(Degradation::new(
                span,
                "shape",
                DegradationAction::Skipped,
                message,
            ));
        }
        _ => {}
    });
}
