//! M9: overline via a floating line shape.
//!
//! Typst renders `#overline` as real `Geometry::Line` shapes in the paged
//! frame, inside a `Tag::Start(OverlineElem) .. Tag::End` bracket. For C2/C3
//! each bracket is measured (segment extents, stroke, and the y offset from
//! the text line's top) and injected into the IR as `IrInline::Overline`
//! anchored before the first covered run; the runs' `overline` flag is
//! cleared so lowering emits no degrade. Runs that cannot be matched keep
//! the flag and get the same `Skipped` degradation as C1.

use std::ops::Range;

use tydocx_ir::{IrBlock, IrDocument, IrInline};
use tydocx_ooxml::document::AnchoredLine;
use typst_layout::PagedDocument;
use typst_library::introspection::{Location, Tag};
use typst_library::layout::{Frame, FrameItem, Point};
use typst_library::text::OverlineElem;
use typst_library::visualize::Geometry;
use typst_syntax::{DiagSpan, DiagSpanKind, Source, Span};

use crate::convert::page::extract_non_black_color;
use crate::world::TydocxWorld;

const PT_TO_EMU: f64 = 12_700.0;

struct OverlineMeasurement {
    /// One anchored line per deco segment (a wrapped overline draws one
    /// segment per text line).
    lines: Vec<AnchoredLine>,
    /// Source byte range of the `#overline` call, when the span resolves.
    range: Option<Range<usize>>,
    /// The bracket's text, used by the unordered fallback match.
    text: String,
}

struct InProgress {
    range: Option<Range<usize>>,
    text: String,
    /// `(x_start, x_end, y, thickness_emu, color)` per `Geometry::Line`.
    segments: Vec<(f64, f64, f64, u64, Option<String>)>,
    /// `(baseline_y, ascender)` of the first text item in the bracket.
    line_top: Option<f64>,
}

/// Measure overline brackets in the paged frames, then splice them into the
/// IR. Runs inside a matched bracket lose their `overline` flag; unmatched
/// runs keep it and degrade at lowering.
pub fn patch_overlines(doc: &mut IrDocument, paged: &PagedDocument, world: &TydocxWorld) {
    let has_overline = ir_has_overline(doc);
    if !has_overline {
        return;
    }
    let source = world.main_source();
    let mut measurements = Vec::new();
    for page in paged.pages() {
        collect_overlines(
            &page.frame,
            source,
            &mut Vec::new(),
            &mut measurements,
            Point::zero(),
        );
    }
    if measurements.is_empty() {
        return;
    }
    let mut state = PatchState {
        measurements: &measurements,
        used: vec![false; measurements.len()],
        source,
    };
    patch_overlines_in_blocks(&mut doc.body, &mut state);
    for footnote in &mut doc.footnotes {
        for para in &mut footnote.body {
            patch_overlines_in_inlines(&mut para.inlines, &mut state);
        }
    }
}

fn ir_has_overline(doc: &IrDocument) -> bool {
    fn inlines_have(inlines: &[IrInline]) -> bool {
        inlines.iter().any(|inline| match inline {
            IrInline::Text(run) => run.overline,
            IrInline::Link { body, .. }
            | IrInline::Shape { body, .. }
            | IrInline::Field { body, .. } => inlines_have(body),
            _ => false,
        })
    }
    fn blocks_have(blocks: &[IrBlock]) -> bool {
        blocks.iter().any(|block| match block {
            IrBlock::Paragraph(para) => inlines_have(&para.inlines),
            IrBlock::Heading { body, .. } => inlines_have(body),
            IrBlock::Table(table) => table
                .rows
                .iter()
                .flat_map(|row| &row.cells)
                .any(|cell| blocks_have(&cell.body)),
            IrBlock::Grid(grid) => grid
                .rows
                .iter()
                .flat_map(|row| &row.cells)
                .any(|cell| blocks_have(&cell.body)),
            IrBlock::Figure { body, caption, .. } => blocks_have(body) || inlines_have(caption),
            IrBlock::List(list) => list.items.iter().any(|item| blocks_have(&item.body)),
            IrBlock::Enum(enumer) => enumer.items.iter().any(|item| blocks_have(&item.body)),
            IrBlock::Quote { body }
            | IrBlock::Place { body, .. }
            | IrBlock::Shape { body, .. }
            | IrBlock::Columns { body, .. } => blocks_have(body),
            IrBlock::Bibliography { title, entries } => {
                blocks_have(title) || entries.iter().any(|entry| inlines_have(&entry.body))
            }
            IrBlock::Outline { title, entries, .. } => {
                blocks_have(title) || entries.iter().any(|entry| inlines_have(&entry.title))
            }
            IrBlock::Equation(_) | IrBlock::Raw(_) | IrBlock::PageBreak { .. } => false,
        })
    }
    blocks_have(&doc.body)
        || doc
            .footnotes
            .iter()
            .flat_map(|note| &note.body)
            .any(|para| inlines_have(&para.inlines))
}

fn span_byte_range(source: &Source, span: Span) -> Option<Range<usize>> {
    match DiagSpan::from(span).get() {
        DiagSpanKind::Number { id, num, sub_range } if id == source.id() => {
            source.range(num, sub_range)
        }
        DiagSpanKind::Range { id, range } if id == source.id() => Some(range),
        _ => None,
    }
}

/// Walk a frame, collecting one measurement per `OverlineElem` tag bracket.
/// `stack` holds the brackets currently open; a closing `Tag::End` pops the
/// innermost bracket with a matching location.
fn collect_overlines(
    frame: &Frame,
    source: &Source,
    stack: &mut Vec<(Location, InProgress)>,
    out: &mut Vec<OverlineMeasurement>,
    offset: Point,
) {
    for (pos, item) in frame.items() {
        let absolute = Point::new(offset.x + pos.x, offset.y + pos.y);
        match item {
            FrameItem::Tag(Tag::Start(content, _)) => {
                if let Some(overline) = content.to_packed::<OverlineElem>()
                    && let Some(location) = content.location()
                {
                    stack.push((
                        location,
                        InProgress {
                            range: span_byte_range(source, overline.span()),
                            text: String::new(),
                            segments: Vec::new(),
                            line_top: None,
                        },
                    ));
                }
            }
            FrameItem::Tag(Tag::End(location, ..)) => {
                if let Some(index) = stack.iter().rposition(|(loc, _)| loc == location) {
                    let (_, done) = stack.remove(index);
                    if let Some(measurement) = finish_measurement(done) {
                        out.push(measurement);
                    }
                }
            }
            FrameItem::Text(text) => {
                if let Some((_, open)) = stack.last_mut() {
                    open.text.push_str(&text.text);
                    if open.line_top.is_none() {
                        let ascender = text.font.metrics().ascender.at(text.size).to_pt();
                        open.line_top = Some(absolute.y.to_pt() - ascender);
                    }
                }
            }
            FrameItem::Shape(shape, _) => {
                if let Some((_, open)) = stack.last_mut()
                    && let Geometry::Line(target) = shape.geometry
                    && let Some(stroke) = &shape.stroke
                {
                    let x0 = absolute.x.to_pt();
                    let x1 = x0 + target.x.to_pt();
                    #[allow(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "stroke thickness is a small positive pt value"
                    )]
                    let thickness = (stroke.thickness.to_pt() * PT_TO_EMU).round() as u64;
                    open.segments.push((
                        x0.min(x1),
                        x0.max(x1),
                        absolute.y.to_pt(),
                        thickness.max(1),
                        extract_non_black_color(&stroke.paint),
                    ));
                }
            }
            FrameItem::Group(group) => {
                collect_overlines(&group.frame, source, stack, out, absolute);
            }
            FrameItem::Image(..) | FrameItem::Link(..) => {}
        }
    }
}

fn finish_measurement(done: InProgress) -> Option<OverlineMeasurement> {
    let line_top = done.line_top?;
    let mut lines = Vec::with_capacity(done.segments.len());
    for (x0, x1, y, thickness, color) in &done.segments {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_possible_wrap,
            reason = "page coordinates are small pt values; the wrap can only occur on absurd documents"
        )]
        let line = AnchoredLine {
            x_emu: (x0 * PT_TO_EMU).round() as i64,
            y_emu: ((y - line_top) * PT_TO_EMU).round() as i64,
            length_emu: ((x1 - x0) * PT_TO_EMU).round() as u64,
            thickness_emu: *thickness,
            color: color.clone(),
        };
        lines.push(line);
    }
    if lines.is_empty() {
        return None;
    }
    Some(OverlineMeasurement {
        lines,
        range: done.range,
        text: done.text,
    })
}

struct PatchState<'a> {
    measurements: &'a [OverlineMeasurement],
    used: Vec<bool>,
    source: &'a Source,
}

fn patch_overlines_in_blocks(blocks: &mut [IrBlock], state: &mut PatchState<'_>) {
    for block in blocks {
        match block {
            IrBlock::Paragraph(para) => patch_overlines_in_inlines(&mut para.inlines, state),
            IrBlock::Heading { body, .. } => patch_overlines_in_inlines(body, state),
            IrBlock::Table(table) => {
                for row in &mut table.rows {
                    for cell in &mut row.cells {
                        patch_overlines_in_blocks(&mut cell.body, state);
                    }
                }
            }
            IrBlock::Grid(grid) => {
                for row in &mut grid.rows {
                    for cell in &mut row.cells {
                        patch_overlines_in_blocks(&mut cell.body, state);
                    }
                }
            }
            IrBlock::Figure { body, caption, .. } => {
                patch_overlines_in_blocks(body, state);
                patch_overlines_in_inlines(caption, state);
            }
            IrBlock::List(list) => {
                for item in &mut list.items {
                    patch_overlines_in_blocks(&mut item.body, state);
                }
            }
            IrBlock::Enum(enumer) => {
                for item in &mut enumer.items {
                    patch_overlines_in_blocks(&mut item.body, state);
                }
            }
            IrBlock::Quote { body }
            | IrBlock::Place { body, .. }
            | IrBlock::Shape { body, .. }
            | IrBlock::Columns { body, .. } => {
                patch_overlines_in_blocks(body, state);
            }
            IrBlock::Bibliography { title, entries } => {
                patch_overlines_in_blocks(title, state);
                for entry in entries {
                    patch_overlines_in_inlines(&mut entry.body, state);
                }
            }
            IrBlock::Outline { title, entries, .. } => {
                patch_overlines_in_blocks(title, state);
                for entry in entries {
                    patch_overlines_in_inlines(&mut entry.title, state);
                }
            }
            IrBlock::Equation(_) | IrBlock::Raw(_) | IrBlock::PageBreak { .. } => {}
        }
    }
}

fn patch_overlines_in_inlines(inlines: &mut Vec<IrInline>, state: &mut PatchState<'_>) {
    // Pass 1: match runs to measurements by source-span containment.
    let mut i = 0;
    while i < inlines.len() {
        let matched = match &inlines[i] {
            IrInline::Text(run) if run.overline => run
                .span
                .and_then(|span| span_byte_range(state.source, span))
                .and_then(|range| {
                    state.measurements.iter().position(|m| {
                        m.range
                            .as_ref()
                            .is_some_and(|mr| mr.start <= range.start && range.end <= mr.end)
                    })
                }),
            _ => None,
        };
        if let Some(mi) = matched {
            if !state.used[mi] {
                state.used[mi] = true;
                for (offset, line) in state.measurements[mi].lines.iter().enumerate() {
                    inlines.insert(i + offset, IrInline::Overline(line.clone()));
                }
                i += state.measurements[mi].lines.len();
            }
            if let IrInline::Text(run) = &mut inlines[i] {
                run.overline = false;
            }
        }
        i += 1;
    }

    // Pass 2: leftover overlined runs take leftover measurements in document
    // order, gated by a text equality check so a mismatched bracket is not
    // attached to the wrong text.
    let mut i = 0;
    while i < inlines.len() {
        let is_overlined = |inline: &IrInline| matches!(inline, IrInline::Text(r) if r.overline);
        if !is_overlined(&inlines[i]) {
            i += 1;
            continue;
        }
        let start = i;
        let mut text = String::new();
        while i < inlines.len() {
            match &inlines[i] {
                IrInline::Text(run) if run.overline => text.push_str(&run.text),
                IrInline::Space if inlines.get(i + 1).is_some_and(is_overlined) => {}
                _ => break,
            }
            i += 1;
        }
        let end = i;
        let mut next = (0..state.measurements.len()).filter(|mi| !state.used[*mi]);
        let found = next.find(|mi| state.measurements[*mi].text == text);
        if let Some(mi) = found {
            state.used[mi] = true;
            for (offset, line) in state.measurements[mi].lines.iter().enumerate() {
                inlines.insert(start + offset, IrInline::Overline(line.clone()));
            }
            for inline in &mut inlines[start + state.measurements[mi].lines.len()
                ..end + state.measurements[mi].lines.len()]
            {
                if let IrInline::Text(run) = inline {
                    run.overline = false;
                }
            }
            i += state.measurements[mi].lines.len();
        }
    }
}
