use crate::ooxml::document::{
    AnchoredContent, BlockElement, Document, DocumentStyle, InlineElement, Paragraph,
    ParagraphStyle, Run, for_each_paragraph_in_block_mut,
};

pub(super) fn strip_redundant(doc: &mut Document) {
    let style = &doc.style;
    strip_blocks(&mut doc.body.elements, style);
    for footnote in &mut doc.footnotes {
        for para in &mut footnote.paragraphs {
            strip_inlines(&mut para.inlines, style.footnote_size_half_pt, style);
        }
    }
    for part in [
        &mut doc.header,
        &mut doc.header_first,
        &mut doc.header_even,
        &mut doc.footer,
        &mut doc.footer_first,
        &mut doc.footer_even,
    ]
    .into_iter()
    .flatten()
    {
        for para in &mut part.paragraphs {
            strip_inlines(&mut para.inlines, style.body_size_half_pt, style);
        }
    }
}

fn strip_blocks(blocks: &mut [BlockElement], style: &DocumentStyle) {
    for block in blocks {
        for_each_paragraph_in_block_mut(block, &mut |para| {
            let size = paragraph_size(para, style);
            strip_inlines(&mut para.inlines, size, style);
        });
    }
}

fn paragraph_size(para: &Paragraph, style: &DocumentStyle) -> u32 {
    if para.code_block {
        return style.code_size_half_pt;
    }
    match para.style {
        Some(ParagraphStyle::Heading(level)) => {
            style.heading_sizes[usize::from(level.saturating_sub(1)).min(4)]
        }
        Some(ParagraphStyle::Normal) | None => style.body_size_half_pt,
    }
}

fn strip_inlines(inlines: &mut [InlineElement], size: u32, style: &DocumentStyle) {
    let strip = |run: &mut Run| {
        if run.size_half_pt == Some(size) {
            run.size_half_pt = None;
        }
    };
    for inline in inlines {
        match inline {
            InlineElement::Text(run) => strip(run),
            InlineElement::Field { result, .. } => result.iter_mut().for_each(strip),
            InlineElement::Hyperlink { body, .. } | InlineElement::InternalLink { body, .. } => {
                strip_inlines(body, size, style);
            }
            InlineElement::AnchoredFloat(float) => {
                if let AnchoredContent::Text { blocks, .. } = &mut float.content {
                    strip_blocks(blocks, style);
                }
            }
            InlineElement::Shape(shape) => strip_blocks(&mut shape.blocks, style),
            _ => {}
        }
    }
}
