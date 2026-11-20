//! IR walkers shared by the conformance tests: variant coverage and the
//! paragraph-text census over `IrDocument`.

use std::collections::BTreeSet;

use typort_ir::{IrBlock, IrDocument, IrEnumItem, IrInline, IrListItem, IrParagraph};

/// One paragraph's worth of text the IR carries, plus the generated prefixes
/// the lowerer may emit in front of it (numbering, markers, captions).
pub struct IrText {
    /// Paragraph texts in document order (each a concatenation of its
    /// inline text).
    pub paragraphs: Vec<String>,
    /// Generated text fragments not part of any paragraph's inline body:
    /// heading `numbering_text`, literal enum `number_text`, list markers,
    /// bibliography markers, footnote markers, page-field displays. The
    /// census allows an output paragraph to begin with one of these.
    pub generated: BTreeSet<String>,
    /// Text the IR carries that Word renders out-of-band instead of
    /// `document.xml` — bibliography entry bodies (emitted from
    /// `customXml/sources.xml` by Word's bibliography field). The census
    /// accounts these as a named degradation, not a drop.
    pub out_of_band: Vec<String>,
}

fn inline_text(inlines: &[IrInline], out: &mut String, generated: &mut BTreeSet<String>) {
    for inline in inlines {
        match inline {
            IrInline::Text(run) => out.push_str(&run.text),
            IrInline::Link { body, .. } => inline_text(body, out, generated),
            IrInline::Ref { display_text, .. }
            | IrInline::Cite {
                rendered: display_text,
                ..
            }
            | IrInline::PageField { display_text, .. } => out.push_str(display_text),
            IrInline::FootnoteMarker { rendered, .. } => {
                generated.insert(rendered.clone());
            }
            IrInline::Space => out.push(' '),
            IrInline::Shape { body, .. } | IrInline::Field { body, .. } => {
                inline_text(body, out, generated)
            }
            IrInline::LineBreak | IrInline::Tab | IrInline::PageBreak | IrInline::ColumnBreak => {
                out.push(' ')
            }
            IrInline::Image(_) | IrInline::Math { .. } | IrInline::Overline(_) => {}
        }
    }
}

fn paragraph_text(para: &IrParagraph, generated: &mut BTreeSet<String>) -> String {
    let mut text = String::new();
    inline_text(&para.inlines, &mut text, generated);
    text
}

fn item_blocks(items: &[IrListItem]) -> impl Iterator<Item = &IrBlock> {
    items.iter().flat_map(|item| item.body.iter())
}

fn enum_item_blocks(items: &[IrEnumItem]) -> impl Iterator<Item = &IrBlock> {
    items.iter().flat_map(|item| item.body.iter())
}

fn block_texts<'a>(
    blocks: &mut dyn Iterator<Item = &'a IrBlock>,
    paragraphs: &mut Vec<String>,
    generated: &mut BTreeSet<String>,
    out_of_band: &mut Vec<String>,
) {
    for block in blocks {
        match block {
            IrBlock::Paragraph(para) => paragraphs.push(paragraph_text(para, generated)),
            IrBlock::Heading {
                body,
                numbering_text,
                ..
            } => {
                let mut text = String::new();
                inline_text(body, &mut text, generated);
                paragraphs.push(text);
                if let Some(number) = numbering_text {
                    generated.insert(format!("{number} "));
                }
            }
            IrBlock::Table(table) => block_texts(
                &mut table
                    .rows
                    .iter()
                    .flat_map(|row| row.cells.iter())
                    .flat_map(|cell| cell.body.iter()),
                paragraphs,
                generated,
                out_of_band,
            ),
            IrBlock::Grid(grid) => block_texts(
                &mut grid
                    .rows
                    .iter()
                    .flat_map(|row| row.cells.iter())
                    .flat_map(|cell| cell.body.iter()),
                paragraphs,
                generated,
                out_of_band,
            ),
            IrBlock::Figure {
                body,
                caption,
                supplement,
                ..
            } => {
                block_texts(&mut body.iter(), paragraphs, generated, out_of_band);
                let mut caption_text = String::new();
                inline_text(caption, &mut caption_text, generated);
                if !caption_text.trim().is_empty() {
                    if let Some(supplement) = supplement {
                        generated.insert(format!("{supplement} "));
                    }
                    paragraphs.push(caption_text);
                }
            }
            IrBlock::List(list) => {
                if let Some(markers) = &list.marker_texts {
                    for marker in markers {
                        generated.insert(marker.clone());
                    }
                }
                block_texts(
                    &mut item_blocks(&list.items),
                    paragraphs,
                    generated,
                    out_of_band,
                );
            }
            IrBlock::Enum(enumer) => {
                for item in &enumer.items {
                    if let Some(number) = &item.number_text {
                        generated.insert(format!("{number} "));
                    }
                }
                block_texts(
                    &mut enum_item_blocks(&enumer.items),
                    paragraphs,
                    generated,
                    out_of_band,
                );
            }
            IrBlock::Equation(equation) => {
                if let Some(number) = &equation.number {
                    generated.insert(number.display.clone());
                }
            }
            IrBlock::Quote { body } | IrBlock::Place { body, .. } | IrBlock::Shape { body, .. } => {
                block_texts(&mut body.iter(), paragraphs, generated, out_of_band);
            }
            IrBlock::Raw(raw) => {
                for line in &raw.lines {
                    let mut text = String::new();
                    for run in line {
                        text.push_str(&run.text);
                    }
                    paragraphs.push(text);
                }
            }
            IrBlock::Outline { title, entries, .. } => {
                generated
                    .insert("Right-click and update field to see table of contents.".to_owned());
                let mut title_text = String::new();
                inline_text(title, &mut title_text, generated);
                if !title_text.trim().is_empty() {
                    paragraphs.push(title_text);
                }
                for entry in entries {
                    let mut text = String::new();
                    inline_text(&entry.title, &mut text, generated);
                    generated.insert(text);
                    if let Some(page) = &entry.page_number {
                        generated.insert(page.clone());
                    }
                }
            }
            IrBlock::Bibliography { entries } => {
                for entry in entries {
                    if let Some(marker) = &entry.marker {
                        generated.insert(format!("{marker} "));
                    }
                    let mut text = String::new();
                    inline_text(&entry.body, &mut text, generated);
                    if !text.trim().is_empty() {
                        out_of_band.push(text);
                    }
                }
            }
            IrBlock::PageBreak { .. } => {}
        }
    }
}

/// Collect every paragraph text and generated fragment an `IrDocument`
/// carries, including footnote bodies.
pub fn ir_texts(doc: &IrDocument) -> IrText {
    let mut texts = IrText {
        paragraphs: Vec::new(),
        generated: BTreeSet::new(),
        out_of_band: Vec::new(),
    };
    block_texts(
        &mut doc.body.iter(),
        &mut texts.paragraphs,
        &mut texts.generated,
        &mut texts.out_of_band,
    );
    for footnote in &doc.footnotes {
        generated_from_footnote(footnote, &mut texts);
    }
    texts
}

fn generated_from_footnote(footnote: &typort_ir::IrFootnote, texts: &mut IrText) {
    texts.generated.insert(footnote.rendered.clone());
    for para in &footnote.body {
        let text = paragraph_text(para, &mut texts.generated);
        texts.paragraphs.push(text);
    }
}

/// The set of `IrBlock` variant names a document uses, including nested
/// container bodies.
pub fn block_variants(doc: &IrDocument) -> BTreeSet<&'static str> {
    variant_walk(doc).0
}

/// The set of `IrInline` variant names a document uses, walked separately
/// from block names because `IrBlock::Shape` and `IrInline::Shape` share the
/// label "Shape" — a single merged set cannot tell which one a corpus hit.
pub fn inline_variants(doc: &IrDocument) -> BTreeSet<&'static str> {
    variant_walk(doc).1
}

fn variant_walk(doc: &IrDocument) -> (BTreeSet<&'static str>, BTreeSet<&'static str>) {
    let mut blocks = BTreeSet::new();
    let mut inlines = BTreeSet::new();
    let mut stack: Vec<&IrBlock> = doc.body.iter().collect();
    while let Some(block) = stack.pop() {
        blocks.insert(match block {
            IrBlock::Paragraph(_) => "Paragraph",
            IrBlock::Heading { .. } => "Heading",
            IrBlock::Table(_) => "Table",
            IrBlock::Grid(_) => "Grid",
            IrBlock::Figure { .. } => "Figure",
            IrBlock::List(_) => "List",
            IrBlock::Enum(_) => "Enum",
            IrBlock::Equation(_) => "Equation",
            IrBlock::Quote { .. } => "Quote",
            IrBlock::Raw(_) => "Raw",
            IrBlock::Outline { .. } => "Outline",
            IrBlock::Bibliography { .. } => "Bibliography",
            IrBlock::Place { .. } => "Place",
            IrBlock::PageBreak { .. } => "PageBreak",
            IrBlock::Shape { .. } => "Shape",
        });
        match block {
            IrBlock::Table(t) => stack.extend(
                t.rows
                    .iter()
                    .flat_map(|row| row.cells.iter())
                    .flat_map(|cell| cell.body.iter()),
            ),
            IrBlock::Grid(g) => stack.extend(
                g.rows
                    .iter()
                    .flat_map(|row| row.cells.iter())
                    .flat_map(|cell| cell.body.iter()),
            ),
            IrBlock::Figure { body, .. }
            | IrBlock::Quote { body }
            | IrBlock::Place { body, .. }
            | IrBlock::Shape { body, .. } => stack.extend(body.iter()),
            IrBlock::List(list) => stack.extend(item_blocks(&list.items)),
            IrBlock::Enum(enumer) => stack.extend(enum_item_blocks(&enumer.items)),
            IrBlock::Outline { title, entries, .. } => {
                inline_variant_names(title.iter(), &mut inlines);
                for entry in entries {
                    inline_variant_names(entry.title.iter(), &mut inlines);
                }
            }
            _ => {}
        }
        inline_variant_names(block_inlines(block), &mut inlines);
    }
    for footnote in &doc.footnotes {
        for para in &footnote.body {
            inline_variant_names(para.inlines.iter(), &mut inlines);
        }
    }
    (blocks, inlines)
}

fn block_inlines(block: &IrBlock) -> &[IrInline] {
    match block {
        IrBlock::Paragraph(para) => &para.inlines,
        IrBlock::Heading { body, .. } => body,
        IrBlock::Figure { caption, .. } => caption,
        _ => &[],
    }
}

fn inline_variant_names<'a>(
    inlines: impl IntoIterator<Item = &'a IrInline>,
    variants: &mut BTreeSet<&'static str>,
) {
    for inline in inlines {
        variants.insert(match inline {
            IrInline::Text(_) => "Text",
            IrInline::Link { .. } => "Link",
            IrInline::Ref { .. } => "Ref",
            IrInline::Cite { .. } => "Cite",
            IrInline::FootnoteMarker { .. } => "FootnoteMarker",
            IrInline::LineBreak => "LineBreak",
            IrInline::Space => "Space",
            IrInline::Image(_) => "Image",
            IrInline::Math { .. } => "Math",
            IrInline::PageBreak => "PageBreak",
            IrInline::ColumnBreak => "ColumnBreak",
            IrInline::Tab => "Tab",
            IrInline::PageField { .. } => "PageField",
            IrInline::Shape { .. } => "Shape",
            IrInline::Field { .. } => "Field",
            IrInline::Overline(_) => "Overline",
        });
        match inline {
            IrInline::Link { body, .. }
            | IrInline::Shape { body, .. }
            | IrInline::Field { body, .. } => {
                inline_variant_names(body.iter(), variants);
            }
            _ => {}
        }
    }
}
