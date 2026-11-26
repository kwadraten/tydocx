use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use quick_xml::NsReader;
use quick_xml::events::Event;
use quick_xml::name::{QName, ResolveResult};
use tydocx_ooxml::document::{
    AnchorWrap, AnchoredContent, AnchoredFloat, AnchoredLine, BlockElement, Document,
    EquationNumber, HeaderFooter, ImageData, ImageFormat, InlineElement, InlineShape, PageSettings,
    Paragraph, RelFrom, Run, SectionBreak, SectionBreakType, ShapeCap, ShapeCompound, ShapeJoin,
    ShapeOutline, ShapeStroke, SrcRect,
};
use tydocx_ooxml::write_docx;
use zip::ZipArchive;

const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const PIC: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const WPS: &str = "http://schemas.microsoft.com/office/word/2010/wordprocessingShape";
const ASVG: &str = "http://schemas.microsoft.com/office/drawing/2016/SVG/main";
const OPC_RELS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const OPC_TYPES: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const PNG: &[u8] = include_bytes!("../../../tests/fixtures/test_image.png");
const SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"><rect width="1" height="1"/></svg>"#;

type Package = ZipArchive<Cursor<Vec<u8>>>;

fn package(doc: &Document) -> Package {
    let mut output = Cursor::new(Vec::new());
    write_docx(doc, &mut output).unwrap();
    ZipArchive::new(output).unwrap()
}

fn entry(package: &mut Package, path: &str) -> Vec<u8> {
    let mut result = Vec::new();
    package
        .by_name(path)
        .unwrap_or_else(|err| panic!("{path}: {err}"))
        .read_to_end(&mut result)
        .unwrap();
    result
}

#[derive(Default)]
struct Node {
    name: String,
    attrs: BTreeMap<String, String>,
    children: Vec<Node>,
    text: String,
}

impl Node {
    fn child(&self, name: &str) -> &Self {
        let mut matches = self.children.iter().filter(|node| node.name == name);
        let child = matches
            .next()
            .unwrap_or_else(|| panic!("missing {name} under {}", self.name));
        assert!(
            matches.next().is_none(),
            "duplicate {name} under {}",
            self.name
        );
        child
    }

    fn order(&self, expected: &[&str]) {
        let names: Vec<_> = self
            .children
            .iter()
            .map(|node| node.name.as_str())
            .collect();
        assert_eq!(names, expected, "children of {}", self.name);
    }

    fn attr(&self, name: &str) -> &str {
        &self.attrs[name]
    }

    fn descendants(&self, name: &str) -> Vec<&Self> {
        self.children
            .iter()
            .flat_map(|child| {
                (child.name == name)
                    .then_some(child)
                    .into_iter()
                    .chain(child.descendants(name))
            })
            .collect()
    }
}

fn check_qname(name: QName<'_>, resolved: ResolveResult<'_>, default_ns: Option<&str>) -> String {
    // A spelled prefix is not enough: check its resolved URI for elements and attributes.
    let name = std::str::from_utf8(name.as_ref()).unwrap();
    let expected = name.split_once(':').map_or(default_ns, |(prefix, _)| {
        Some(match prefix {
            "a" => A,
            "pic" => PIC,
            "r" => R,
            "wps" => WPS,
            "asvg" => ASVG,
            "mc" => "http://schemas.openxmlformats.org/markup-compatibility/2006",
            "w14" => "http://schemas.microsoft.com/office/word/2010/wordml",
            "w" => "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
            "m" => "http://schemas.openxmlformats.org/officeDocument/2006/math",
            "wp" => "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing",
            "xml" => "http://www.w3.org/XML/1998/namespace",
            _ => panic!("unexpected prefix on {name}"),
        })
    });
    let actual = match resolved {
        ResolveResult::Bound(ns) => Some(std::str::from_utf8(ns.0).unwrap()),
        ResolveResult::Unbound => None,
        ResolveResult::Unknown(prefix) => panic!("{name}: unbound {prefix:?}"),
    };
    assert_eq!(actual, expected, "namespace for {name}");
    name.to_owned()
}

fn tree(package: &mut Package, path: &str, default_ns: Option<&str>) -> Node {
    let xml = entry(package, path);
    let mut reader = NsReader::from_reader(xml.as_slice());
    let mut stack = vec![Node::default()];
    loop {
        let event = reader
            .read_event()
            .unwrap_or_else(|err| panic!("{path}: {err}"));
        match event {
            Event::Start(ref tag) | Event::Empty(ref tag) => {
                let mut node = Node {
                    name: check_qname(tag.name(), reader.resolve_element(tag.name()).0, default_ns),
                    ..Node::default()
                };
                for attr in tag.attributes() {
                    let attr = attr.unwrap();
                    if attr.key.as_ref() == b"xmlns" || attr.key.as_ref().starts_with(b"xmlns:") {
                        continue;
                    }
                    let name = check_qname(attr.key, reader.resolve_attribute(attr.key).0, None);
                    let value = attr
                        .decode_and_unescape_value(reader.decoder())
                        .unwrap()
                        .into_owned();
                    if name == "mc:Ignorable" && value == "w14" {
                        // Prefixes named in attribute values must be in scope on this element.
                        let name = QName(b"w14:ligatures");
                        check_qname(name, reader.resolve_element(name).0, None);
                    }
                    node.attrs.insert(name, value);
                }
                if matches!(event, Event::Start(_)) {
                    stack.push(node);
                } else {
                    stack.last_mut().unwrap().children.push(node);
                }
            }
            Event::End(_) => {
                let node = stack.pop().unwrap();
                stack.last_mut().unwrap().children.push(node);
            }
            Event::Text(text) => stack
                .last_mut()
                .unwrap()
                .text
                .push_str(&text.unescape().unwrap()),
            Event::Eof => {
                assert_eq!(stack.len(), 1);
                let mut top = stack.pop().unwrap();
                assert_eq!(top.children.len(), 1, "{path} must have one root");
                return top.children.pop().unwrap();
            }
            Event::Decl(_) => {}
            _ => panic!("{path}: unexpected XML event {event:?}"),
        }
    }
}

fn picture(wrapper: &Node) -> &Node {
    let data = wrapper.child("a:graphic").child("a:graphicData");
    assert_eq!(data.attr("uri"), PIC);
    data.order(&["pic:pic"]);
    let picture = data.child("pic:pic");
    picture.order(&["pic:nvPicPr", "pic:blipFill", "pic:spPr"]);
    picture
        .child("pic:nvPicPr")
        .order(&["pic:cNvPr", "pic:cNvPicPr"]);
    let props = picture.child("pic:spPr");
    props.order(&["a:xfrm", "a:prstGeom"]);
    let transform = props.child("a:xfrm");
    transform.order(&["a:off", "a:ext"]);
    for axis in ["cx", "cy"] {
        let display = wrapper.child("wp:extent").attr(axis);
        assert!(display.parse::<u64>().unwrap() > 0);
        assert_eq!(transform.child("a:ext").attr(axis), display);
    }
    picture.child("pic:blipFill")
}

fn check_media(package: &mut Package, source: &str, embed: &str, target: &str, bytes: &[u8]) {
    let rels = tree(
        package,
        &format!("word/_rels/{source}.rels"),
        Some(OPC_RELS),
    );
    assert_eq!(rels.name, "Relationships");
    let mut matched = rels.children.iter().filter(|rel| rel.attr("Id") == embed);
    let rel = matched.next().unwrap();
    assert!(matched.next().is_none(), "duplicate {embed} in {source}");
    assert_eq!(rel.name, "Relationship");
    assert_eq!(rel.attr("Type"), format!("{R}/image"));
    assert_eq!(
        rel.attrs
            .get("TargetMode")
            .map(String::as_str)
            .unwrap_or("Internal"),
        "Internal"
    );
    assert_eq!(rel.attr("Target"), target);
    assert_eq!(entry(package, &format!("word/{target}")), bytes);
}

#[test]
fn cropped_inline_png_and_header_svg_have_ordered_payloads_and_local_media() {
    let mut raster = ImageData {
        bytes: PNG.to_vec(),
        format: ImageFormat::Png,
        svg: None,
        width_emu: 25400,
        height_emu: 12700,
        src_rect: Some(SrcRect {
            l: 25000,
            t: 0,
            r: 0,
            b: 0,
        }),
        alt: Some("Image & description".into()),
    };
    let mut body = Paragraph::new();
    body.add_image(raster.clone());
    raster.svg = Some(SVG.as_bytes().to_vec());
    let anchor = AnchoredFloat {
        x_emu: -12700,
        y_emu: 25400,
        horz_rel: RelFrom::Margin,
        width_emu: raster.width_emu,
        height_emu: raster.height_emu,
        clearance_emu: 0,
        wrap: AnchorWrap::None,
        behind_doc: true,
        z_order: 3,
        content: AnchoredContent::Image(raster),
    };
    let mut header = Paragraph::new();
    header.inlines.push(InlineElement::AnchoredFloat(anchor));
    let mut doc = Document::new();
    doc.add_paragraph(body);
    doc.header = Some(HeaderFooter {
        paragraphs: vec![header],
    });
    let mut package = package(&doc);
    let body = tree(&mut package, "word/document.xml", None);
    let drawing = body
        .child("w:body")
        .child("w:p")
        .child("w:r")
        .child("w:drawing");
    drawing.order(&["wp:inline"]);
    let inline = drawing.child("wp:inline");
    inline.order(&["wp:extent", "wp:docPr", "a:graphic"]);
    assert_eq!(
        inline.child("wp:docPr").attr("descr"),
        "Image & description"
    );
    let blip_fill = picture(inline);
    blip_fill.order(&["a:blip", "a:srcRect", "a:stretch"]);
    assert_eq!(blip_fill.child("a:srcRect").attr("l"), "25000");
    blip_fill.child("a:stretch").order(&["a:fillRect"]);
    let blip = blip_fill.child("a:blip");
    blip.order(&[]);
    check_media(
        &mut package,
        "document.xml",
        blip.attr("r:embed"),
        "media/image1.png",
        PNG,
    );

    let header = tree(&mut package, "word/header1.xml", None);
    let drawing = header.child("w:p").child("w:r").child("w:drawing");
    drawing.order(&["wp:anchor"]);
    let anchor = drawing.child("wp:anchor");
    anchor.order(&[
        "wp:simplePos",
        "wp:positionH",
        "wp:positionV",
        "wp:extent",
        "wp:effectExtent",
        "wp:wrapNone",
        "wp:docPr",
        "wp:cNvGraphicFramePr",
        "a:graphic",
    ]);
    assert_eq!(anchor.attr("behindDoc"), "1");
    let position = anchor.child("wp:positionH");
    assert_eq!(position.attr("relativeFrom"), "margin");
    assert_eq!(position.child("wp:posOffset").text, "-12700");
    let blip_fill = picture(anchor);
    blip_fill.order(&["a:blip", "a:srcRect", "a:stretch"]);
    let blip = blip_fill.child("a:blip");
    blip.order(&["a:extLst"]);
    let ext = blip.child("a:extLst").child("a:ext");
    // Pin the writer's extension token. The base blip still resolves to a PNG on its own.
    assert_eq!(ext.attr("uri"), "{96DAC541-7B7A-43D3-8B79-37D633B846F1}");
    ext.order(&["asvg:svgBlip"]);
    let svg = ext.child("asvg:svgBlip");
    assert_ne!(blip.attr("r:embed"), svg.attr("r:embed"));
    check_media(
        &mut package,
        "header1.xml",
        blip.attr("r:embed"),
        "media/image2.png",
        PNG,
    );
    check_media(
        &mut package,
        "header1.xml",
        svg.attr("r:embed"),
        "media/image2.svg",
        SVG.as_bytes(),
    );
    let types = tree(&mut package, "[Content_Types].xml", Some(OPC_TYPES));
    for (extension, media) in [("png", "image/png"), ("svg", "image/svg+xml")] {
        assert!(types.children.iter().any(|node| {
            node.name == "Default"
                && node.attr("Extension") == extension
                && node.attr("ContentType") == media
        }));
    }
}

#[test]
fn header_shape_keeps_omml_in_its_text_box_paragraph() {
    let mut textbox_para = Paragraph::new();
    textbox_para.add_math("<m:oMath><m:r><m:t>x</m:t></m:r></m:oMath>".into());
    let mut para = Paragraph::new();
    para.inlines.push(InlineElement::Shape(InlineShape {
        width_emu: 127000,
        height_emu: 127000,
        outline: ShapeOutline::Rect,
        fill_hex: None,
        stroke: None,
        inset_emu: Default::default(),
        blocks: vec![BlockElement::Paragraph(textbox_para)],
    }));
    let mut doc = Document::new();
    doc.header = Some(HeaderFooter {
        paragraphs: vec![para],
    });
    let root = tree(&mut package(&doc), "word/header1.xml", None);
    let drawing = root.child("w:p").child("w:r").child("w:drawing");
    let data = drawing
        .child("wp:inline")
        .child("a:graphic")
        .child("a:graphicData");
    assert_eq!(data.attr("uri"), WPS);
    let shape = data.child("wps:wsp");
    shape.order(&["wps:cNvSpPr", "wps:spPr", "wps:txbx", "wps:bodyPr"]);
    assert_eq!(shape.child("wps:cNvSpPr").attr("txBox"), "1");
    let props = shape.child("wps:spPr");
    props.order(&["a:xfrm", "a:prstGeom", "a:noFill"]);
    props.child("a:xfrm").order(&["a:off", "a:ext"]);
    let text = shape.child("wps:txbx").child("w:txbxContent").child("w:p");
    text.order(&["m:oMath"]);
    assert_eq!(text.child("m:oMath").child("m:r").child("m:t").text, "x");
    shape.child("wps:bodyPr").order(&["a:noAutofit"]);
}

#[test]
fn drawingml_strokes_encode_compound_values_and_order_fill_dash_join() {
    let mut doc = Document::new();
    let cases = [
        (ShapeCompound::Single, None, ShapeJoin::Round, "a:round"),
        (
            ShapeCompound::Double,
            Some("dbl"),
            ShapeJoin::Miter { lim: 400_000 },
            "a:miter",
        ),
        (
            ShapeCompound::ThickThin,
            Some("thickThin"),
            ShapeJoin::Bevel,
            "a:bevel",
        ),
        (
            ShapeCompound::ThinThick,
            Some("thinThick"),
            ShapeJoin::Round,
            "a:round",
        ),
        (
            ShapeCompound::Triple,
            Some("tri"),
            ShapeJoin::Round,
            "a:round",
        ),
    ];
    for (compound, _, join, _) in cases {
        let mut para = Paragraph::new();
        para.inlines.push(InlineElement::Shape(InlineShape {
            width_emu: 127_000,
            height_emu: 0,
            outline: ShapeOutline::Line {
                flip_h: false,
                flip_v: false,
            },
            fill_hex: None,
            stroke: Some(ShapeStroke {
                width_emu: 12_700,
                compound,
                color_hex: Some("123456".into()),
                alpha: Some(50_000),
                cap: ShapeCap::Square,
                join,
                dash: Some(vec![(100_000, 200_000)]),
            }),
            inset_emu: Default::default(),
            blocks: vec![],
        }));
        doc.add_paragraph(para);
    }
    let root = tree(&mut package(&doc), "word/document.xml", None);
    let lines = root.descendants("a:ln");
    assert_eq!(lines.len(), cases.len());
    for (line, (_, compound, _, join)) in lines.into_iter().zip(cases) {
        // CT_LineProperties permits only one fill, dash and join, in that order.
        line.order(&["a:solidFill", "a:custDash", join]);
        assert_eq!(line.attr("w"), "12700");
        assert_eq!(line.attr("cap"), "sq");
        assert_eq!(line.attrs.get("cmpd").map(String::as_str), compound);
        let color = line.child("a:solidFill").child("a:srgbClr");
        assert_eq!(color.attr("val"), "123456");
        assert_eq!(color.child("a:alpha").attr("val"), "50000");
        let dash = line.child("a:custDash").child("a:ds");
        assert_eq!((dash.attr("d"), dash.attr("sp")), ("100000", "200000"));
        if join == "a:miter" {
            assert_eq!(line.child(join).attr("lim"), "400000");
        }
    }
}

#[test]
fn zero_width_shape_stroke_suppresses_paint_instead_of_emitting_a_hairline() {
    let mut para = Paragraph::new();
    para.inlines.push(InlineElement::Shape(InlineShape {
        width_emu: 127_000,
        height_emu: 0,
        outline: ShapeOutline::Line {
            flip_h: false,
            flip_v: false,
        },
        fill_hex: None,
        stroke: Some(ShapeStroke {
            width_emu: 0,
            compound: ShapeCompound::Single,
            color_hex: Some("123456".into()),
            alpha: None,
            cap: ShapeCap::Flat,
            join: ShapeJoin::Round,
            dash: None,
        }),
        inset_emu: Default::default(),
        blocks: vec![],
    }));
    let mut doc = Document::new();
    doc.add_paragraph(para);
    let root = tree(&mut package(&doc), "word/document.xml", None);
    let line = root.descendants("wps:spPr")[0].child("a:ln");
    // MS-OI29500 §2.1.1326: `w="0"` would be Word's minimum visible width.
    assert!(line.attrs.is_empty());
    line.order(&["a:noFill"]);
}

#[test]
fn numbered_math_and_its_bookmarked_field_share_the_paragraph_without_nesting() {
    let mut para = Paragraph::new();
    para.inlines.push(InlineElement::Math {
        omml_xml: "<m:oMathPara><m:oMath><m:r><m:t>x</m:t></m:r></m:oMath></m:oMathPara>".into(),
        equation_number: Some(EquationNumber::Bookmarked {
            prefix: "(".into(),
            display: "7".into(),
            instruction: Some(" SEQ equation ".into()),
            suffix: ")".into(),
            bookmarks: vec![(42, "eq_seven".into())],
        }),
    });
    let mut doc = Document::new();
    doc.add_paragraph(para);
    let root = tree(&mut package(&doc), "word/document.xml", None);
    let p = root.child("w:body").child("w:p");
    p.order(&[
        "w:pPr",
        "w:r",
        "m:oMath",
        "w:r",
        "w:r",
        "w:bookmarkStart",
        "w:r",
        "w:r",
        "w:r",
        "w:r",
        "w:r",
        "w:bookmarkEnd",
        "w:r",
    ]);
    let c = &p.children;
    c[1].order(&["w:tab"]);
    c[3].order(&["w:tab"]);
    // Only the field result's numeral is referenceable; parentheses stay outside the range.
    assert_eq!(c[4].child("w:t").text, "(");
    assert_eq!(c[12].child("w:t").text, ")");
    assert_eq!(c[5].attr("w:name"), "eq_seven");
    assert_eq!(c[5].attr("w:id"), c[11].attr("w:id"));
    for (index, kind) in [(6, "begin"), (8, "separate"), (10, "end")] {
        assert_eq!(c[index].child("w:fldChar").attr("w:fldCharType"), kind);
    }
    let instruction = c[7].child("w:instrText");
    assert_eq!(instruction.attr("xml:space"), "preserve");
    assert_eq!(instruction.text, " SEQ equation ");
    assert_eq!(c[9].child("w:t").text, "7");
}

#[test]
fn intermediate_and_final_sections_write_all_required_page_margin_attributes() {
    let mut doc = Document::new();
    for settings in [
        Some(PageSettings {
            header_twips: Some(0),
            ..PageSettings::default()
        }),
        None,
    ] {
        let mut para = Paragraph::new();
        para.section_break = Some(SectionBreak {
            break_type: SectionBreakType::NextPage,
            page_settings: settings,
            title_page: false,
            page_numbering: None,
            page_number_start: None,
        });
        doc.add_paragraph(para);
    }
    doc.page_settings.footer_twips = Some(390);

    let root = tree(&mut package(&doc), "word/document.xml", None);
    let margins = root.descendants("w:pgMar");
    assert_eq!(margins.len(), 3);
    // CT_PageMar requires the margins even without a header/footer part; an explicit zero is not a default.
    for (margin, (header, footer)) in
        margins
            .into_iter()
            .zip([("0", "720"), ("720", "720"), ("720", "390")])
    {
        assert_eq!(margin.attrs.len(), 7);
        assert_eq!(
            (
                margin.attr("w:header"),
                margin.attr("w:footer"),
                margin.attr("w:gutter")
            ),
            (header, footer, "0"),
        );
        for name in ["w:top", "w:bottom", "w:left", "w:right"] {
            assert!(margin.attrs.contains_key(name));
        }
    }
}

#[test]
fn opentype_run_properties_are_qualified_and_ignorable_in_body_and_footer() {
    let mut doc = Document::new();
    let mut para = Paragraph::new();
    let mut styled = Run::new("alternate glyphs");
    styled.ligatures = Some("contextual");
    styled.num_form = Some("oldStyle");
    styled.number_spacing = Some("tabular");
    para.inlines.push(InlineElement::Text(styled));
    let mut ordinary = Run::new("ordinary run");
    ordinary.bold = true;
    para.inlines.push(InlineElement::Text(ordinary));
    doc.add_paragraph(para.clone());
    doc.footer = Some(HeaderFooter {
        paragraphs: vec![para],
    });

    let mut package = package(&doc);
    for path in ["word/document.xml", "word/footer1.xml"] {
        let root = tree(&mut package, path, None);
        let props = root.descendants("w:rPr");
        assert_eq!(props.len(), 2);
        assert_eq!(props[0].attr("mc:Ignorable"), "w14");
        props[0].order(&["w14:ligatures", "w14:numForm", "w14:numSpacing"]);
        for (child, expected) in props[0]
            .children
            .iter()
            .zip(["contextual", "oldStyle", "tabular"])
        {
            assert_eq!(child.attrs.len(), 1);
            assert_eq!(child.attr("w14:val"), expected);
        }
        assert!(!props[1].attrs.contains_key("mc:Ignorable"));
        props[1].order(&["w:b"]);
    }
}

#[test]
fn drawing_ids_are_package_wide_even_when_image_relationships_restart_per_story() {
    let png = ImageData {
        bytes: PNG.to_vec(),
        format: ImageFormat::Png,
        svg: None,
        width_emu: 25400,
        height_emu: 12700,
        src_rect: None,
        alt: None,
    };
    let mut doc = Document::new();
    let mut note = Paragraph::new();
    note.add_image(png.clone());
    let note_id = doc.add_footnote(vec![note]);
    let mut body = Paragraph::new();
    body.add_image(png.clone());
    body.inlines.push(InlineElement::AnchoredLine(AnchoredLine {
        x_emu: 0,
        y_emu: 0,
        length_emu: 25400,
        thickness_emu: 6350,
        color: None,
    }));
    body.add_footnote_ref(note_id);
    doc.add_paragraph(body);
    let linked_part = || {
        let mut para = Paragraph::new();
        para.add_hyperlink_body(
            "https://example.test/".into(),
            vec![InlineElement::Image(png.clone())],
        );
        HeaderFooter {
            paragraphs: vec![para],
        }
    };
    doc.header = Some(linked_part());
    doc.footer = Some(linked_part());
    let mut output = package(&doc);
    let mut repeated_output = package(&doc);
    // Drawing IDs count each occurrence across story parts, unlike their local image relationships.
    for (source, expected_ids, media) in [
        ("document.xml", ["1", "2"].as_slice(), "media/image1.png"),
        ("footnotes.xml", ["3"].as_slice(), "media/image2.png"),
        ("header1.xml", ["4"].as_slice(), "media/image3.png"),
        ("footer1.xml", ["5"].as_slice(), "media/image4.png"),
    ] {
        let path = format!("word/{source}");
        let root = tree(&mut output, &path, None);
        let ids: Vec<_> = root
            .descendants("wp:docPr")
            .into_iter()
            .map(|node| node.attr("id"))
            .collect();
        assert_eq!(ids, expected_ids);
        assert_eq!(
            entry(&mut output, &path),
            entry(&mut repeated_output, &path)
        );
        let blip = root.descendants("a:blip")[0];
        let embed = blip.attr("r:embed");
        if source != "document.xml" {
            assert_eq!(embed, "rId1");
        }
        check_media(&mut output, source, embed, media, PNG);
    }
}
