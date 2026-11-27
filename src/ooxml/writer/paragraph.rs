use std::io::{self, Write};

use quick_xml::Writer;

use super::WriteCtx;
use super::document::write_section_break;
use super::fields::{write_fld_char, write_instruction_text_run, write_toc_inline};
use super::math::{strip_math_para, write_equation_number, write_math_inline};
use super::package::{Indentation, two_em_hanging_twips, write_indentation};
use super::run::{write_inline, write_tab};
use crate::ooxml::document::{Alignment, HangingIndent, InlineElement, Paragraph};

#[derive(Clone, Copy)]
enum ParagraphKind<'a> {
    Plain,
    TocEntry(Option<&'a str>),
}

pub(super) fn write_paragraph<W: Write>(
    writer: &mut Writer<W>,
    para: &Paragraph,
    ctx: &WriteCtx,
) -> io::Result<()> {
    write_paragraph_kind(writer, para, ctx, ParagraphKind::Plain)
}

pub(super) fn write_toc_paragraph<W: Write>(
    writer: &mut Writer<W>,
    para: &Paragraph,
    ctx: &WriteCtx,
    instruction: Option<&str>,
) -> io::Result<()> {
    write_paragraph_kind(writer, para, ctx, ParagraphKind::TocEntry(instruction))
}

#[allow(
    clippy::too_many_lines,
    reason = "writes the schema-ordered paragraph XML"
)]
fn write_paragraph_kind<W: Write>(
    writer: &mut Writer<W>,
    para: &Paragraph,
    ctx: &WriteCtx,
    kind: ParagraphKind<'_>,
) -> io::Result<()> {
    writer.create_element("w:p").write_inner_content(|w| {
        let has_style = para.style.is_some();
        let has_list = para.list_info.is_some();
        let has_num = has_list || para.num_pr.is_some();
        let has_alignment = para.alignment.is_some();
        let has_left_indent = para.left_indent.is_some() || para.right_indent.is_some();
        let has_keep = para.keep_lines || para.keep_next;
        let has_code_block = para.code_block;
        let has_section_break = para.section_break.is_some();
        // Determine if we need to suppress the inherited first-line indent
        let suppress_indent = para.suppress_indent
            || (has_alignment
                && matches!(para.alignment, Some(Alignment::Center | Alignment::Right)));
        // Check if this paragraph has a numbered equation (needs right tab stop)
        let has_eq_number = para.inlines.iter().any(|i| {
            matches!(
                i,
                InlineElement::Math {
                    equation_number: Some(_),
                    ..
                }
            )
        });
        let has_hanging = para.hanging_indent.is_some();
        let has_hrule = para.horizontal_rule;
        let has_tab_stops = para.left_tab_stop.is_some() || !para.tab_stops.is_empty();
        let has_spacing = para.spacing_before.is_some()
            || para.spacing_after.is_some()
            || para.line_twips.is_some()
            || para.min_line_twips.is_some();
        if has_style
            || has_num
            || has_alignment
            || suppress_indent
            || has_eq_number
            || has_hanging
            || has_left_indent
            || has_code_block
            || has_section_break
            || has_hrule
            || para.shading.is_some()
            || has_tab_stops
            || para.break_latin_words
            || has_spacing
            || has_keep
            || para.mark_size_half_pt.is_some()
            || para.first_line_indent.is_some()
            || para.justify.is_some()
        {
            w.create_element("w:pPr").write_inner_content(|ppr| {
                if has_code_block {
                    ppr.create_element("w:pStyle")
                        .with_attribute(("w:val", "CodeBlock"))
                        .write_empty()?;
                } else if let Some(style) = &para.style {
                    let style_id = style.ooxml_value();
                    ppr.create_element("w:pStyle")
                        .with_attribute(("w:val", style_id.as_ref()))
                        .write_empty()?;
                }
                if para.keep_next {
                    ppr.create_element("w:keepNext").write_empty()?;
                }
                if para.keep_lines {
                    ppr.create_element("w:keepLines").write_empty()?;
                }
                if let Some(li) = para.list_info.as_ref().or(para.num_pr.as_ref()) {
                    let id_str = li.id.to_string();
                    let lvl_str = li.level.to_string();
                    ppr.create_element("w:numPr").write_inner_content(|num| {
                        num.create_element("w:ilvl")
                            .with_attribute(("w:val", lvl_str.as_str()))
                            .write_empty()?;
                        num.create_element("w:numId")
                            .with_attribute(("w:val", id_str.as_str()))
                            .write_empty()?;
                        Ok(())
                    })?;
                }
                if has_hrule {
                    ppr.create_element("w:pBdr").write_inner_content(|bdr| {
                        bdr.create_element("w:bottom")
                            .with_attribute(("w:val", "single"))
                            .with_attribute(("w:sz", "6"))
                            .with_attribute(("w:space", "1"))
                            .with_attribute(("w:color", "auto"))
                            .write_empty()?;
                        Ok(())
                    })?;
                }
                if let Some(fill) = &para.shading {
                    ppr.create_element("w:shd")
                        .with_attribute(("w:val", "clear"))
                        .with_attribute(("w:color", "auto"))
                        .with_attribute(("w:fill", fill.as_str()))
                        .write_empty()?;
                }
                // Emit tab stops for numbered equations (right-aligned at page width)
                // or for recovered grid/multi-column layouts
                if has_eq_number {
                    // Word's own numbered-equation layout: a center tab at the middle
                    // of the text area centers the equation, and a right tab at the
                    // right margin holds the number. (This is what `#`-numbering in
                    // Word's equation editor produces.)
                    let center_pos = (ctx.content_width_twips / 2).to_string();
                    let right_pos = ctx.content_width_twips.to_string();
                    ppr.create_element("w:tabs").write_inner_content(|tabs| {
                        tabs.create_element("w:tab")
                            .with_attribute(("w:val", "center"))
                            .with_attribute(("w:pos", center_pos.as_str()))
                            .write_empty()?;
                        tabs.create_element("w:tab")
                            .with_attribute(("w:val", "right"))
                            .with_attribute(("w:pos", right_pos.as_str()))
                            .write_empty()?;
                        Ok(())
                    })?;
                } else if has_tab_stops {
                    ppr.create_element("w:tabs").write_inner_content(|tabs| {
                        if let Some(pos) = para.left_tab_stop {
                            tabs.create_element("w:tab")
                                .with_attribute(("w:val", "left"))
                                .with_attribute(("w:pos", pos.to_string().as_str()))
                                .write_empty()?;
                        }
                        for &pos in &para.tab_stops {
                            let pos_str = pos.to_string();
                            let mut tab = tabs
                                .create_element("w:tab")
                                .with_attribute(("w:val", "right"))
                                .with_attribute(("w:pos", pos_str.as_str()));
                            if matches!(kind, ParagraphKind::TocEntry(_)) {
                                tab = tab.with_attribute(("w:leader", "dot"));
                            }
                            tab.write_empty()?;
                        }
                        Ok(())
                    })?;
                }
                if para.break_latin_words {
                    ppr.create_element("w:wordWrap")
                        .with_attribute(("w:val", "0"))
                        .write_empty()?;
                }
                if has_spacing {
                    write_spacing(ppr, para)?;
                }
                // Emit indent: left indent (blockquote), hanging (bibliography), list, or suppress first-line
                // When the Normal style carries an East-Asian char-based
                // first-line indent (firstLineChars), a per-paragraph override
                // that zeroes firstLine must also zero firstLineChars, else
                // Word's char-based value wins and the zeroing is ignored. When
                // None, emit exactly the historical attribute set (byte-identical).
                let zero_chars = ctx.doc_style.first_line_indent_chars.is_some();
                // A resolved `par.first-line-indent` override folds into
                // `w:ind`: positive amounts emit `w:firstLine` (+ the
                // char-based twin for em indents or a `0` that clears a
                // char-based Normal indent); a negative amount (outdent)
                // emits `w:hanging` instead, summed into the branch's own
                // hanging. Indent suppression (post-heading, equation
                // numbers) zeroes the first-line amount but keeps an outdent.
                let force_zero = suppress_indent || has_eq_number;
                let (fl_line, fl_chars, fl_extra_hanging) = match para.first_line_indent {
                    Some(fl) if fl.twips >= 0 => {
                        if force_zero {
                            (0, 0, 0)
                        } else {
                            (fl.twips, fl.chars.unwrap_or(0), 0)
                        }
                    }
                    Some(fl) => (
                        0,
                        0,
                        u32::try_from(fl.twips.unsigned_abs()).unwrap_or(u32::MAX),
                    ),
                    None => (0, 0, 0),
                };
                let fl_line_str = fl_line.to_string();
                let fl_chars_str = fl_chars.to_string();
                let fl_hanging_str = fl_extra_hanging.to_string();
                let has_fl_override = para.first_line_indent.is_some();
                let fl_first_line_chars =
                    has_fl_override
                        .then_some(fl_chars_str.as_str())
                        .filter(|_| {
                            para.first_line_indent
                                .is_some_and(|fl| fl.chars.is_some() || zero_chars)
                        });
                if para.left_indent.is_some() || para.right_indent.is_some() {
                    let left_str = para.left_indent.map(|v| v.to_string());
                    let right_str = para.right_indent.map(|v| v.to_string());
                    write_indentation(
                        ppr,
                        Indentation {
                            left: left_str.as_deref(),
                            right: right_str.as_deref(),
                            hanging: (fl_extra_hanging > 0).then_some(fl_hanging_str.as_str()),
                            first_line_chars: fl_first_line_chars.or(zero_chars.then_some("0")),
                            first_line: has_fl_override
                                .then_some(fl_line_str.as_str())
                                .or(Some("0")),
                        },
                    )?;
                } else if has_list {
                    let list_left = para
                        .list_left_twips
                        .unwrap_or_else(|| two_em_hanging_twips(ctx.doc_style.body_size_half_pt));
                    let list_hanging = para
                        .list_hanging_twips
                        .unwrap_or(list_left / 2)
                        .saturating_add(fl_extra_hanging);
                    let list_left_str = list_left.to_string();
                    let list_hanging_str = list_hanging.to_string();
                    write_indentation(
                        ppr,
                        Indentation {
                            left: Some(list_left_str.as_str()),
                            hanging: Some(list_hanging_str.as_str()),
                            ..Default::default()
                        },
                    )?;
                } else if has_hanging {
                    let bib_indent = match para.hanging_indent.unwrap_or(HangingIndent::Default) {
                        HangingIndent::Default => {
                            two_em_hanging_twips(ctx.doc_style.body_size_half_pt)
                        }
                        HangingIndent::Twips(twips) => twips,
                    };
                    let total_hanging = bib_indent.saturating_add(fl_extra_hanging);
                    let bib_indent_str = bib_indent.to_string();
                    let total_hanging_str = total_hanging.to_string();
                    write_indentation(
                        ppr,
                        Indentation {
                            left: Some(bib_indent_str.as_str()),
                            hanging: Some(total_hanging_str.as_str()),
                            first_line_chars: fl_first_line_chars.or(zero_chars.then_some("0")),
                            first_line: has_fl_override
                                .then_some(fl_line_str.as_str())
                                .or(Some("0")),
                            ..Default::default()
                        },
                    )?;
                } else if suppress_indent || has_eq_number || has_fl_override {
                    write_indentation(
                        ppr,
                        Indentation {
                            hanging: (fl_extra_hanging > 0).then_some(fl_hanging_str.as_str()),
                            first_line_chars: fl_first_line_chars.or(zero_chars.then_some("0")),
                            first_line: has_fl_override
                                .then_some(fl_line_str.as_str())
                                .or(Some("0")),
                            ..Default::default()
                        },
                    )?;
                }
                // Emit alignment: a declared `align` wins; otherwise the
                // resolved `par.justify` pins `both`/`left` so a paragraph
                // never inherits a Normal style justification it did not ask
                // for (Typst `#linebreak()` lines stay unjustified).
                if let Some(alignment) = &para.alignment {
                    ppr.create_element("w:jc")
                        .with_attribute(("w:val", alignment.ooxml_value()))
                        .write_empty()?;
                } else if let Some(justify) = para.justify {
                    ppr.create_element("w:jc")
                        .with_attribute(("w:val", if justify { "both" } else { "left" }))
                        .write_empty()?;
                }
                if let Some(sz) = para.mark_size_half_pt {
                    let sz_str = sz.to_string();
                    ppr.create_element("w:rPr").write_inner_content(|rpr| {
                        rpr.create_element("w:sz")
                            .with_attribute(("w:val", sz_str.as_str()))
                            .write_empty()?;
                        rpr.create_element("w:szCs")
                            .with_attribute(("w:val", sz_str.as_str()))
                            .write_empty()?;
                        Ok(())
                    })?;
                }
                // Emit section break (w:sectPr inside w:pPr)
                if let Some(section) = &para.section_break {
                    write_section_break(ppr, section, ctx.parts)?;
                }
                Ok(())
            })?;
        }
        if let ParagraphKind::TocEntry(Some(instruction)) = kind {
            write_fld_char(w, "begin")?;
            write_instruction_text_run(w, instruction)?;
            write_fld_char(w, "separate")?;
        }
        for inline in &para.inlines {
            if let InlineElement::Math {
                omml_xml,
                equation_number: Some(num),
            } = inline
            {
                // Word-native numbered display equation: a leading tab moves
                // to the center tab (centering the math), the math is emitted
                // as inline <m:oMath> (a block <m:oMathPara> does not coexist
                // with trailing runs on the same line), then a trailing tab
                // moves to the right tab and the number is written there.
                write_tab(w)?;
                write_math_inline(w, strip_math_para(omml_xml))?;
                write_equation_number(w, num)?;
                continue;
            }
            match kind {
                ParagraphKind::Plain => write_inline(w, inline, ctx)?,
                ParagraphKind::TocEntry(_) => write_toc_inline(w, inline, ctx)?,
            }
        }
        Ok(())
    })?;
    Ok(())
}

fn write_spacing<W: Write>(ppr: &mut Writer<W>, para: &Paragraph) -> io::Result<()> {
    let mut spacing = ppr.create_element("w:spacing");
    if let Some(before) = para.spacing_before {
        spacing = spacing.with_attribute(("w:before", before.to_string().as_str()));
    }
    if let Some(after) = para.spacing_after {
        spacing = spacing.with_attribute(("w:after", after.to_string().as_str()));
    }
    let exact = para
        .line_twips
        .or((para.spacing_before.is_some() && para.mark_size_half_pt.is_some()).then_some(0));
    if let Some((line, rule)) = exact
        .map(|line| (line, "exact"))
        .or_else(|| para.min_line_twips.map(|line| (line, "atLeast")))
    {
        spacing = spacing
            .with_attribute(("w:line", line.to_string().as_str()))
            .with_attribute(("w:lineRule", rule));
    }
    spacing.write_empty()?;
    Ok(())
}
