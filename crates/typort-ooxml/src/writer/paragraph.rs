use std::io::{self, Write};

use quick_xml::Writer;

use super::WriteCtx;
use super::document::write_section_break;
use super::math::{strip_math_para, write_equation_number, write_math_inline};
use super::package::{Indentation, two_em_hanging_twips, write_indentation};
use super::run::{write_inline, write_tab};
use crate::document::{Alignment, HangingIndent, InlineElement, Paragraph};

#[allow(
    clippy::too_many_lines,
    reason = "writes the schema-ordered paragraph XML"
)]
pub(super) fn write_paragraph<W: Write>(
    writer: &mut Writer<W>,
    para: &Paragraph,
    ctx: &WriteCtx,
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
        let has_tab_stops = !para.tab_stops.is_empty();
        let has_spacing = para.spacing_before.is_some() || para.spacing_after.is_some();
        if has_style
            || has_list
            || has_num
            || has_alignment
            || suppress_indent
            || has_eq_number
            || has_hanging
            || has_left_indent
            || has_code_block
            || has_section_break
            || has_hrule
            || has_tab_stops
            || has_spacing
            || has_keep
        {
            w.create_element("w:pPr").write_inner_content(|ppr| {
                // Horizontal rule: emit bottom border
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
                        for &pos in &para.tab_stops {
                            let pos_str = pos.to_string();
                            tabs.create_element("w:tab")
                                .with_attribute(("w:val", "right"))
                                .with_attribute(("w:pos", pos_str.as_str()))
                                .write_empty()?;
                        }
                        Ok(())
                    })?;
                }
                // Emit indent: left indent (blockquote), hanging (bibliography), list, or suppress first-line
                // When the Normal style carries an East-Asian char-based
                // first-line indent (firstLineChars), a per-paragraph override
                // that zeroes firstLine must also zero firstLineChars, else
                // Word's char-based value wins and the zeroing is ignored. When
                // None, emit exactly the historical attribute set (byte-identical).
                let zero_chars = ctx.doc_style.first_line_indent_chars.is_some();
                if para.left_indent.is_some() || para.right_indent.is_some() {
                    let left_str = para.left_indent.map(|v| v.to_string());
                    let right_str = para.right_indent.map(|v| v.to_string());
                    write_indentation(
                        ppr,
                        Indentation {
                            left: left_str.as_deref(),
                            right: right_str.as_deref(),
                            first_line_chars: zero_chars.then_some("0"),
                            first_line: Some("0"),
                            ..Default::default()
                        },
                    )?;
                } else if has_hanging {
                    // Bibliographies use the historical 2em default; source-authored
                    // paragraph rules carry their exact converted width.
                    let bib_indent = match para.hanging_indent.unwrap_or(HangingIndent::Default) {
                        HangingIndent::Default => {
                            two_em_hanging_twips(ctx.doc_style.body_size_half_pt)
                        }
                        HangingIndent::Twips(twips) => twips,
                    };
                    let bib_indent_str = bib_indent.to_string();
                    write_indentation(
                        ppr,
                        Indentation {
                            left: Some(bib_indent_str.as_str()),
                            hanging: Some(bib_indent_str.as_str()),
                            first_line_chars: zero_chars.then_some("0"),
                            first_line: Some("0"),
                            ..Default::default()
                        },
                    )?;
                } else if has_list {
                    // List indent: left = 2em, hanging = 1em, computed from body
                    // font size; Typst `list(indent:, body-indent:)` overrides.
                    let list_left = para
                        .list_left_twips
                        .unwrap_or_else(|| two_em_hanging_twips(ctx.doc_style.body_size_half_pt));
                    let list_hanging = para.list_hanging_twips.unwrap_or(list_left / 2);
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
                } else if suppress_indent || has_eq_number {
                    write_indentation(
                        ppr,
                        Indentation {
                            first_line_chars: zero_chars.then_some("0"),
                            first_line: Some("0"),
                            ..Default::default()
                        },
                    )?;
                }
                // Emit spacing override
                if let Some(before) = para.spacing_before {
                    let before_str = before.to_string();
                    let after_str = para.spacing_after.map(|v| v.to_string());
                    let el = ppr
                        .create_element("w:spacing")
                        .with_attribute(("w:before", before_str.as_str()));
                    if let Some(after) = after_str.as_deref() {
                        el.with_attribute(("w:after", after)).write_empty()?;
                    } else {
                        el.write_empty()?;
                    }
                } else if let Some(after) = para.spacing_after {
                    let after_str = after.to_string();
                    ppr.create_element("w:spacing")
                        .with_attribute(("w:after", after_str.as_str()))
                        .write_empty()?;
                }
                // Emit alignment
                if let Some(alignment) = &para.alignment {
                    ppr.create_element("w:jc")
                        .with_attribute(("w:val", alignment.ooxml_value()))
                        .write_empty()?;
                }
                // Emit section break (w:sectPr inside w:pPr)
                if let Some(section) = &para.section_break {
                    write_section_break(ppr, section)?;
                }
                Ok(())
            })?;
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
            write_inline(w, inline, ctx)?;
        }
        Ok(())
    })?;
    Ok(())
}
