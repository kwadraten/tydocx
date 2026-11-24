//! The `wp:inline` + `wps:wsp` writer for bare shapes (`#rect`, `#square`,
//! `#ellipse`, `#circle`, `#polygon`, `#curve`, geometric `#line`): a
//! `wps:wsp` carrying a preset (`a:prstGeom`) or freeform (`a:custGeom`)
//! geometry, a solid or absent fill, an `a:ln` stroke, and a `wps:txbx`
//! when the shape holds text.

use std::io::{self, Write};

use quick_xml::Writer;

use super::WriteCtx;
use super::citation::write_bibliography_sdt;
use super::fields::write_toc_block;
use super::paragraph::write_paragraph;
use super::table::write_table;
use crate::document::{
    BlockElement, InlineShape, ShapeCap, ShapeJoin, ShapeOutline, ShapePathSeg, ShapeStroke,
};

/// Write a `wp:inline` drawing element holding one [`InlineShape`]: a
/// `w:drawing` inside a run, the extent the shape reserves in the flow,
/// and the `wps:wsp` payload. `n` is the shared `wp:docPr` id.
pub(super) fn write_inline_shape<W: Write>(
    writer: &mut Writer<W>,
    shape: &InlineShape,
    n: usize,
    ctx: &WriteCtx,
) -> io::Result<()> {
    let cx = shape.width_emu.to_string();
    let cy = shape.height_emu.to_string();
    let id_str = n.to_string();
    let name = format!("Shape{n}");
    writer.create_element("w:r").write_inner_content(|w| {
        w.create_element("w:drawing").write_inner_content(|dw| {
            dw.create_element("wp:inline")
                .with_attribute(("distT", "0"))
                .with_attribute(("distB", "0"))
                .with_attribute(("distL", "0"))
                .with_attribute(("distR", "0"))
                .write_inner_content(|inl| {
                    inl.create_element("wp:extent")
                        .with_attribute(("cx", cx.as_str()))
                        .with_attribute(("cy", cy.as_str()))
                        .write_empty()?;
                    inl.create_element("wp:effectExtent")
                        .with_attribute(("l", "0"))
                        .with_attribute(("t", "0"))
                        .with_attribute(("r", "0"))
                        .with_attribute(("b", "0"))
                        .write_empty()?;
                    inl.create_element("wp:docPr")
                        .with_attribute(("id", id_str.as_str()))
                        .with_attribute(("name", name.as_str()))
                        .write_empty()?;
                    inl.create_element("wp:cNvGraphicFramePr").write_empty()?;
                    inl.create_element("a:graphic")
                        .with_attribute((
                            "xmlns:a",
                            "http://schemas.openxmlformats.org/drawingml/2006/main",
                        ))
                        .write_inner_content(|gr| {
                            gr.create_element("a:graphicData")
                                .with_attribute((
                                    "uri",
                                    "http://schemas.microsoft.com/office/word/2010/wordprocessingShape",
                                ))
                                .write_inner_content(|gd| write_wsp(gd, shape, &cx, &cy, ctx))?;
                            Ok(())
                        })?;
                    Ok(())
                })?;
            Ok(())
        })?;
        Ok(())
    })?;
    Ok(())
}

fn write_wsp<W: Write>(
    gd: &mut Writer<W>,
    shape: &InlineShape,
    cx: &str,
    cy: &str,
    ctx: &WriteCtx,
) -> io::Result<()> {
    gd.create_element("wps:wsp")
        .with_attribute((
            "xmlns:wps",
            "http://schemas.microsoft.com/office/word/2010/wordprocessingShape",
        ))
        .write_inner_content(|wsp| {
            let c_nv = wsp.create_element("wps:cNvSpPr");
            if shape.blocks.is_empty() {
                c_nv.write_empty()?;
            } else {
                c_nv.with_attribute(("txBox", "1")).write_empty()?;
            }
            wsp.create_element("wps:spPr").write_inner_content(|sp| {
                write_xfrm(sp, shape, cx, cy)?;
                write_geometry(sp, shape)?;
                match &shape.fill_hex {
                    Some(hex) => {
                        sp.create_element("a:solidFill").write_inner_content(|sf| {
                            sf.create_element("a:srgbClr")
                                .with_attribute(("val", hex.as_str()))
                                .write_empty()?;
                            Ok(())
                        })?;
                    }
                    None => {
                        sp.create_element("a:noFill").write_empty()?;
                    }
                }
                write_stroke(sp, shape.stroke.as_ref())?;
                Ok(())
            })?;
            if shape.blocks.is_empty() {
                wsp.create_element("wps:bodyPr").write_empty()?;
            } else {
                write_text_body(wsp, shape, ctx)?;
            }
            Ok(())
        })?;
    Ok(())
}

/// `a:xfrm` — offset `(0,0)`, extent = the drawing size, plus the flip
/// flags a `line` preset needs to point the right way.
fn write_xfrm<W: Write>(
    sp: &mut Writer<W>,
    shape: &InlineShape,
    cx: &str,
    cy: &str,
) -> io::Result<()> {
    let (flip_h, flip_v) = match &shape.outline {
        ShapeOutline::Line { flip_h, flip_v } => (*flip_h, *flip_v),
        _ => (false, false),
    };
    let el = sp.create_element("a:xfrm");
    let el = if flip_h {
        el.with_attribute(("flipH", "1"))
    } else {
        el
    };
    let el = if flip_v {
        el.with_attribute(("flipV", "1"))
    } else {
        el
    };
    el.write_inner_content(|xf| {
        xf.create_element("a:off")
            .with_attribute(("x", "0"))
            .with_attribute(("y", "0"))
            .write_empty()?;
        xf.create_element("a:ext")
            .with_attribute(("cx", cx))
            .with_attribute(("cy", cy))
            .write_empty()?;
        Ok(())
    })?;
    Ok(())
}

fn write_geometry<W: Write>(sp: &mut Writer<W>, shape: &InlineShape) -> io::Result<()> {
    match &shape.outline {
        ShapeOutline::Rect => write_prst(sp, "rect", &[]),
        ShapeOutline::RoundRect { adj } => write_prst(sp, "roundRect", &[("adj", *adj)]),
        ShapeOutline::Round1Rect { adj } => write_prst(sp, "round1Rect", &[("adj", *adj)]),
        ShapeOutline::Round2SameRect { adj1, adj2 } => {
            write_prst(sp, "round2SameRect", &[("adj1", *adj1), ("adj2", *adj2)])
        }
        ShapeOutline::Round2DiagRect { adj1, adj2 } => {
            write_prst(sp, "round2DiagRect", &[("adj1", *adj1), ("adj2", *adj2)])
        }
        ShapeOutline::Ellipse => write_prst(sp, "ellipse", &[]),
        ShapeOutline::Line { .. } => write_prst(sp, "line", &[]),
        ShapeOutline::Custom {
            path_w_emu,
            path_h_emu,
            segments,
        } => write_cust_geom(sp, *path_w_emu, *path_h_emu, segments),
    }
}

fn write_prst<W: Write>(
    sp: &mut Writer<W>,
    prst: &'static str,
    adj: &[(&'static str, u32)],
) -> io::Result<()> {
    sp.create_element("a:prstGeom")
        .with_attribute(("prst", prst))
        .write_inner_content(|pg| {
            if adj.is_empty() {
                pg.create_element("a:avLst").write_empty()?;
                return Ok(());
            }
            pg.create_element("a:avLst").write_inner_content(|av| {
                for (name, val) in adj {
                    av.create_element("a:gd")
                        .with_attribute(("name", *name))
                        .with_attribute(("fmla", format!("val {val}").as_str()))
                        .write_empty()?;
                }
                Ok(())
            })?;
            Ok(())
        })?;
    Ok(())
}

/// `a:custGeom` — the freeform path `custGeom` shapes write: the path-space
/// `w`/`h` plus `a:pt`-based `moveTo`/`lnTo`/`cubicBezTo`/`close` segments.
fn write_cust_geom<W: Write>(
    sp: &mut Writer<W>,
    path_w_emu: i64,
    path_h_emu: i64,
    segments: &[ShapePathSeg],
) -> io::Result<()> {
    let w = path_w_emu.to_string();
    let h = path_h_emu.to_string();
    sp.create_element("a:custGeom").write_inner_content(|cg| {
        cg.create_element("a:avLst").write_empty()?;
        cg.create_element("a:gdLst").write_empty()?;
        cg.create_element("a:ahLst").write_empty()?;
        cg.create_element("a:cxnLst").write_empty()?;
        cg.create_element("a:rect")
            .with_attribute(("l", "l"))
            .with_attribute(("t", "t"))
            .with_attribute(("r", "r"))
            .with_attribute(("b", "b"))
            .write_empty()?;
        cg.create_element("a:pathLst").write_inner_content(|pl| {
            pl.create_element("a:path")
                .with_attribute(("w", w.as_str()))
                .with_attribute(("h", h.as_str()))
                .write_inner_content(|path| {
                    for seg in segments {
                        match seg {
                            ShapePathSeg::MoveTo { x_emu, y_emu } => {
                                path.create_element("a:moveTo")
                                    .write_inner_content(|el| write_pt(el, *x_emu, *y_emu))?;
                            }
                            ShapePathSeg::LineTo { x_emu, y_emu } => {
                                path.create_element("a:lnTo")
                                    .write_inner_content(|el| write_pt(el, *x_emu, *y_emu))?;
                            }
                            ShapePathSeg::CubicTo {
                                c1x_emu,
                                c1y_emu,
                                c2x_emu,
                                c2y_emu,
                                x_emu,
                                y_emu,
                            } => {
                                path.create_element("a:cubicBezTo")
                                    .write_inner_content(|el| {
                                        write_pt(el, *c1x_emu, *c1y_emu)?;
                                        write_pt(el, *c2x_emu, *c2y_emu)?;
                                        write_pt(el, *x_emu, *y_emu)
                                    })?;
                            }
                            ShapePathSeg::Close => {
                                path.create_element("a:close").write_empty()?;
                            }
                        }
                    }
                    Ok(())
                })?;
            Ok(())
        })?;
        Ok(())
    })?;
    Ok(())
}

fn write_pt<W: Write>(el: &mut Writer<W>, x_emu: i64, y_emu: i64) -> io::Result<()> {
    el.create_element("a:pt")
        .with_attribute(("x", x_emu.to_string().as_str()))
        .with_attribute(("y", y_emu.to_string().as_str()))
        .write_empty()?;
    Ok(())
}

/// `a:ln` — thickness, cap, solid colour, dash pairs, and join in schema
/// order (fill, dash, join). `None` stroke emits no `a:ln` at all.
fn write_stroke<W: Write>(sp: &mut Writer<W>, stroke: Option<&ShapeStroke>) -> io::Result<()> {
    let Some(stroke) = stroke else {
        return Ok(());
    };
    let cap = match stroke.cap {
        ShapeCap::Flat => "flat",
        ShapeCap::Round => "rnd",
        ShapeCap::Square => "sq",
    };
    let color = stroke.color_hex.as_deref().unwrap_or("000000");
    sp.create_element("a:ln")
        .with_attribute(("w", stroke.width_emu.to_string().as_str()))
        .with_attribute(("cap", cap))
        .write_inner_content(|ln| {
            ln.create_element("a:solidFill").write_inner_content(|sf| {
                sf.create_element("a:srgbClr")
                    .with_attribute(("val", color))
                    .write_empty()?;
                Ok(())
            })?;
            if let Some(dash) = &stroke.dash {
                ln.create_element("a:custDash").write_inner_content(|cd| {
                    for (d, gap) in dash {
                        cd.create_element("a:ds")
                            .with_attribute(("d", d.to_string().as_str()))
                            .with_attribute(("sp", gap.to_string().as_str()))
                            .write_empty()?;
                    }
                    Ok(())
                })?;
            }
            match stroke.join {
                ShapeJoin::Miter { lim } => {
                    ln.create_element("a:miter")
                        .with_attribute(("lim", lim.to_string().as_str()))
                        .write_empty()?;
                }
                ShapeJoin::Round => {
                    ln.create_element("a:round").write_empty()?;
                }
                ShapeJoin::Bevel => {
                    ln.create_element("a:bevel").write_empty()?;
                }
            }
            Ok(())
        })?;
    Ok(())
}

/// `wps:txbx` + `wps:bodyPr` for a shape body: the same block-content
/// dispatch the float text box uses, with `a:noAutofit` (the extent is
/// declared or measured — Word must not resize it) and overflow allowed,
/// matching Typst's fixed-frame bodies.
fn write_text_body<W: Write>(
    wsp: &mut Writer<W>,
    shape: &InlineShape,
    ctx: &WriteCtx,
) -> io::Result<()> {
    wsp.create_element("wps:txbx").write_inner_content(|tx| {
        tx.create_element("w:txbxContent")
            .write_inner_content(|c| {
                for element in &shape.blocks {
                    match element {
                        BlockElement::Paragraph(para) => write_paragraph(c, para, ctx)?,
                        BlockElement::Table(table) => write_table(c, table, ctx)?,
                        BlockElement::BibliographyBlock { paragraphs } => {
                            write_bibliography_sdt(c, paragraphs, ctx)?;
                        }
                        BlockElement::TocBlock {
                            max_depth,
                            caption_id,
                            paragraphs,
                        } => {
                            write_toc_block(c, *max_depth, caption_id.as_deref(), paragraphs, ctx)?;
                        }
                    }
                }
                Ok(())
            })?;
        Ok(())
    })?;
    wsp.create_element("wps:bodyPr")
        .with_attribute(("lIns", shape.inset_emu.left_emu.to_string().as_str()))
        .with_attribute(("tIns", shape.inset_emu.top_emu.to_string().as_str()))
        .with_attribute(("rIns", shape.inset_emu.right_emu.to_string().as_str()))
        .with_attribute(("bIns", shape.inset_emu.bottom_emu.to_string().as_str()))
        .with_attribute(("wrap", "square"))
        .with_attribute(("anchor", "t"))
        .with_attribute(("vertOverflow", "overflow"))
        .with_attribute(("horzOverflow", "overflow"))
        .write_inner_content(|bp| {
            bp.create_element("a:noAutofit").write_empty()?;
            Ok(())
        })?;
    Ok(())
}
