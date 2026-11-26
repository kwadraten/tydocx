use std::io::{self, Write};

use quick_xml::Writer;
use quick_xml::events::BytesText;

use super::WriteCtx;
use super::citation::{lang_to_lcid, write_citation_sdt};
use super::fields::{
    write_bookmark_end, write_bookmark_start, write_field, write_field_page, write_field_pageref,
    write_field_ref, write_toc_field,
};
use super::footnotes::write_footnote_ref;
use super::hyperlinks::write_hyperlink;
use super::image::{
    image_rel_id, svg_rel_id, write_anchored_float, write_anchored_line, write_image_inline,
};
use super::math::write_math_inline;
use super::package::{write_font_triple, write_size_pair};
use super::shape::write_inline_shape;
use crate::document::{DocumentStyle, InlineElement, Run};

/// Write one inline element with exactly the body-paragraph semantics.
///
/// Shared by `write_paragraph` and the footnote writer so a footnote
/// hyperlink, image, bookmark, field, or citation is written the same way
/// instead of being silently dropped (the old footnote `write_inline` wrote
/// only Text and Math and returned `Ok(())` for the rest).
///
/// Images are the one exception to the sharing: a footnote image's
/// relationship lives in `word/_rels/footnotes.xml.rels`, not
/// `document.xml.rels`, so the footnote writer intercepts `Image` before
/// calling this and supplies its own relationship id.
pub(super) fn write_inline<W: Write>(
    writer: &mut Writer<W>,
    inline: &InlineElement,
    ctx: &WriteCtx,
) -> io::Result<()> {
    match inline {
        InlineElement::Text(run) => write_run(writer, run, ctx.doc_style),
        InlineElement::FootnoteRef(id) => write_footnote_ref(writer, *id),
        InlineElement::Math {
            omml_xml,
            equation_number: _,
        } => write_math_inline(writer, omml_xml),
        InlineElement::Image(img) => {
            let image_index = ctx.image_counter.get() + 1;
            ctx.image_counter.set(image_index);
            let rid = image_rel_id(image_index, ctx.images, ctx.parts);
            let svg_rid = img
                .svg
                .is_some()
                .then(|| svg_rel_id(image_index, ctx.images, ctx.parts));
            write_image_inline(
                writer,
                img,
                image_index,
                ctx.next_drawing_id()?,
                &rid,
                svg_rid.as_deref(),
            )
        }
        InlineElement::Bookmark { id, name } => write_bookmark_start(writer, *id, name),
        InlineElement::BookmarkEnd { id } => write_bookmark_end(writer, *id),
        InlineElement::FieldRef {
            bookmark_name,
            display_text,
        } => write_field_ref(writer, bookmark_name, display_text),
        InlineElement::FieldPageRef {
            bookmark_name,
            display_text,
        } => write_field_pageref(writer, bookmark_name, display_text),
        InlineElement::FieldPage {
            display_text,
            bold,
            size_half_pt,
        } => write_field_page(writer, display_text, *bold, *size_half_pt),
        InlineElement::Hyperlink {
            url,
            body,
            preserve_style,
        } => write_hyperlink(writer, url, body, *preserve_style, ctx),
        InlineElement::InternalLink { anchor, body } => {
            writer
                .create_element("w:hyperlink")
                .with_attribute(("w:anchor", anchor.as_str()))
                .write_inner_content(|hw| {
                    for inline in body {
                        write_inline(hw, inline, ctx)?;
                    }
                    Ok(())
                })?;
            Ok(())
        }
        InlineElement::PageBreak => write_page_break(writer),
        InlineElement::AnchoredLine(line) => {
            write_anchored_line(writer, line, ctx.next_drawing_id()?)
        }
        InlineElement::AnchoredFloat(float) => {
            write_anchored_float(writer, float, ctx.next_drawing_id()?, ctx)
        }
        InlineElement::Shape(shape) => {
            write_inline_shape(writer, shape, ctx.next_drawing_id()?, ctx)
        }
        InlineElement::ColumnBreak => write_column_break(writer),
        InlineElement::FieldToc {
            max_depth,
            caption_id,
        } => write_toc_field(writer, *max_depth, caption_id.as_deref()),
        InlineElement::Tab => write_tab(writer),
        InlineElement::Citation {
            keys,
            display_text,
            anchor,
        } => {
            let sdt_id = ctx.citation_id_counter.get();
            ctx.citation_id_counter.set(sdt_id + 1);
            let locale_id = lang_to_lcid(&ctx.doc_style.lang_east_asia);
            write_citation_sdt(
                writer,
                keys,
                display_text,
                sdt_id,
                locale_id,
                anchor.as_deref(),
            )
        }
        InlineElement::Field {
            instruction,
            result,
        } => write_field(writer, instruction, result, ctx.doc_style),
    }
}

/// Write a tab character element (`<w:r><w:tab/></w:r>`).
pub(super) fn write_tab<W: Write>(writer: &mut Writer<W>) -> io::Result<()> {
    writer.create_element("w:r").write_inner_content(|w| {
        w.create_element("w:tab").write_empty()?;
        Ok(())
    })?;
    Ok(())
}

pub(super) fn write_page_break<W: Write>(writer: &mut Writer<W>) -> io::Result<()> {
    writer.create_element("w:r").write_inner_content(|w| {
        w.create_element("w:br")
            .with_attribute(("w:type", "page"))
            .write_empty()?;
        Ok(())
    })?;
    Ok(())
}

pub(super) fn write_column_break<W: Write>(writer: &mut Writer<W>) -> io::Result<()> {
    writer.create_element("w:r").write_inner_content(|w| {
        w.create_element("w:br")
            .with_attribute(("w:type", "column"))
            .write_empty()?;
        Ok(())
    })?;
    Ok(())
}

pub(super) fn write_text_run<W: Write>(
    writer: &mut Writer<W>,
    text: &str,
    preserve_space: bool,
) -> io::Result<()> {
    writer.create_element("w:r").write_inner_content(|w| {
        let mut text_element = w.create_element("w:t");
        if preserve_space {
            text_element = text_element.with_attribute(("xml:space", "preserve"));
        }
        text_element.write_text_content(BytesText::new(text))?;
        Ok(())
    })?;
    Ok(())
}

/// Write a forced line break run: `<w:r><w:br/></w:r>`.
fn write_line_break<W: Write>(writer: &mut Writer<W>) -> io::Result<()> {
    writer.create_element("w:r").write_inner_content(|w| {
        w.create_element("w:br").write_empty()?;
        Ok(())
    })?;
    Ok(())
}

fn write_opentype_props<W: Write>(rpr: &mut Writer<W>, run: &Run) -> io::Result<()> {
    if let Some(ligatures) = run.ligatures {
        rpr.create_element("w14:ligatures")
            .with_attribute(("w14:val", ligatures))
            .write_empty()?;
    }
    if let Some(num_form) = run.num_form {
        rpr.create_element("w14:numForm")
            .with_attribute(("w14:val", num_form))
            .write_empty()?;
    }
    if let Some(number_spacing) = run.number_spacing {
        rpr.create_element("w14:numSpacing")
            .with_attribute(("w14:val", number_spacing))
            .write_empty()?;
    }
    Ok(())
}

fn write_run_props<W: Write>(
    rpr: &mut Writer<W>,
    run: &Run,
    doc_style: &DocumentStyle,
) -> io::Result<()> {
    if run.monospace {
        write_font_triple(
            rpr,
            doc_style.code_font.as_str(),
            doc_style.code_font.as_str(),
            false,
        )?;
    } else if run.font_ascii.is_some() || run.font_east_asia.is_some() {
        let ascii = run
            .font_ascii
            .as_deref()
            .unwrap_or(&doc_style.body_font_ascii);
        let east_asia = run
            .font_east_asia
            .as_deref()
            .unwrap_or(&doc_style.body_font_east_asia);
        write_font_triple(rpr, ascii, east_asia, false)?;
    }
    if run.bold {
        rpr.create_element("w:b").write_empty()?;
    }
    if run.italic {
        rpr.create_element("w:i").write_empty()?;
    }
    if run.smallcaps {
        rpr.create_element("w:smallCaps").write_empty()?;
    }
    if let Some(color) = &run.color {
        rpr.create_element("w:color")
            .with_attribute(("w:val", color.as_str()))
            .write_empty()?;
    }
    if let Some(tracking) = run.tracking_twips.filter(|t| *t > 0) {
        rpr.create_element("w:spacing")
            .with_attribute(("w:val", tracking.to_string().as_str()))
            .write_empty()?;
    }
    // A super/subscript run must not carry an explicit reduced size:
    // `w:vertAlign` already shrinks the glyph and raises it by a fraction
    // of the *effective* em, so a pre-shrunk `w:sz` collapses the raise and
    // the mark sits mid-line. Emit vertAlign alone and let the consumer
    // reduce+raise from the inherited body size (ECMA-376 §17.3.2.42).
    if let Some(size) = run
        .size_half_pt
        .filter(|_| !run.superscript && !run.subscript)
    {
        let size_str = size.to_string();
        write_size_pair(rpr, size_str.as_str())?;
    }
    if let Some(position) = run.position_half_pt {
        let val = position.to_string();
        rpr.create_element("w:position")
            .with_attribute(("w:val", val.as_str()))
            .write_empty()?;
    }
    if run.strikethrough {
        rpr.create_element("w:strike").write_empty()?;
    }
    if run.underline {
        rpr.create_element("w:u")
            .with_attribute(("w:val", "single"))
            .write_empty()?;
    }
    if let Some(hl_color) = &run.highlight_color {
        rpr.create_element("w:highlight")
            .with_attribute(("w:val", hl_color.as_str()))
            .write_empty()?;
    }
    if let Some(shading) = &run.shading {
        rpr.create_element("w:shd")
            .with_attribute(("w:val", "clear"))
            .with_attribute(("w:fill", shading.as_str()))
            .write_empty()?;
    }
    if run.superscript {
        rpr.create_element("w:vertAlign")
            .with_attribute(("w:val", "superscript"))
            .write_empty()?;
    }
    if run.subscript {
        rpr.create_element("w:vertAlign")
            .with_attribute(("w:val", "subscript"))
            .write_empty()?;
    }
    write_opentype_props(rpr, run)?;
    Ok(())
}

pub(super) fn write_run<W: Write>(
    writer: &mut Writer<W>,
    run: &Run,
    doc_style: &DocumentStyle,
) -> io::Result<()> {
    if run.line_break {
        return write_line_break(writer);
    }
    writer.create_element("w:r").write_inner_content(|w| {
        if !run.format_key().is_plain() {
            let mut props = w.create_element("w:rPr");
            if run.ligatures.is_some() || run.num_form.is_some() || run.number_spacing.is_some() {
                props = props
                    .with_attribute((
                        "xmlns:w14",
                        "http://schemas.microsoft.com/office/word/2010/wordml",
                    ))
                    .with_attribute((
                        "xmlns:mc",
                        "http://schemas.openxmlformats.org/markup-compatibility/2006",
                    ))
                    .with_attribute(("mc:Ignorable", "w14"));
            }
            props.write_inner_content(|rpr| write_run_props(rpr, run, doc_style))?;
        }
        w.create_element("w:t")
            .with_attribute(("xml:space", "preserve"))
            .write_text_content(BytesText::new(&run.text))?;
        Ok(())
    })?;
    Ok(())
}
