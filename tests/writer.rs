mod tests {
    use quick_xml::Reader;
    use quick_xml::events::Event;
    use std::io::Cursor;
    use tydocx::ooxml::*;

    /// Helper: build a docx from a Document and return the ZIP archive bytes.
    fn build_docx(doc: &Document) -> Vec<u8> {
        let mut buf = Vec::new();
        write_docx(doc, Cursor::new(&mut buf)).unwrap();
        buf
    }

    /// Helper: extract a named file from a docx ZIP archive as a String.
    fn read_zip_entry(buf: &[u8], name: &str) -> String {
        let mut archive = zip::ZipArchive::new(Cursor::new(buf)).unwrap();
        std::io::read_to_string(archive.by_name(name).unwrap()).unwrap()
    }

    /// Helper: check whether a named file exists in the ZIP archive.
    fn zip_has_entry(buf: &[u8], name: &str) -> bool {
        let archive = zip::ZipArchive::new(Cursor::new(buf)).unwrap();
        archive.file_names().any(|n| n == name)
    }

    fn child_names(xml: &str, parent: &str) -> Vec<String> {
        let mut reader = Reader::from_str(xml);
        loop {
            match reader.read_event().unwrap() {
                Event::Start(e) if e.name().as_ref() == parent.as_bytes() => break,
                Event::Eof => panic!("missing {parent}: {xml}"),
                _ => {}
            }
        }
        let mut names = Vec::new();
        let mut depth = 0;
        loop {
            match reader.read_event().unwrap() {
                Event::Start(e) => {
                    if depth == 0 {
                        names.push(String::from_utf8(e.name().as_ref().to_vec()).unwrap());
                    }
                    depth += 1;
                }
                Event::Empty(e) if depth == 0 => {
                    names.push(String::from_utf8(e.name().as_ref().to_vec()).unwrap());
                }
                Event::End(_) if depth == 0 => return names,
                Event::End(_) => depth -= 1,
                Event::Eof => panic!("unclosed {parent}: {xml}"),
                _ => {}
            }
        }
    }

    // ── Existing tests ──────────────────────────────────────────────────

    #[test]
    fn minimal_docx_is_valid_zip_with_content_types() {
        let doc = Document::new();
        let mut buf = Vec::new();
        write_docx(&doc, Cursor::new(&mut buf)).unwrap();

        let reader = zip::ZipArchive::new(Cursor::new(&buf)).unwrap();
        let names: Vec<&str> = reader.file_names().collect();
        assert!(names.contains(&"[Content_Types].xml"));
        assert!(names.contains(&"word/document.xml"));
        assert!(names.contains(&"_rels/.rels"));
        assert!(names.contains(&"word/_rels/document.xml.rels"));
    }

    #[test]
    fn docx_contains_paragraph_text() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        para.add_run("你好世界");
        doc.add_paragraph(para);

        let mut buf = Vec::new();
        write_docx(&doc, Cursor::new(&mut buf)).unwrap();

        let mut reader = zip::ZipArchive::new(Cursor::new(&buf)).unwrap();
        let doc_xml =
            std::io::read_to_string(reader.by_name("word/document.xml").unwrap()).unwrap();
        assert!(doc_xml.contains("你好世界"));
    }

    // ── 1. Run formatting ───────────────────────────────────────────────

    #[test]
    fn run_bold_produces_w_b() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        let mut run = document::Run::new("bold text");
        run.bold = true;
        para.push_run(run);
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert!(xml.contains("<w:b/>"), "expected <w:b/> in: {xml}");
        assert!(xml.contains("bold text"));
    }

    #[test]
    fn run_italic_produces_w_i() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        let mut run = document::Run::new("italic text");
        run.italic = true;
        para.push_run(run);
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert!(xml.contains("<w:i/>"), "expected <w:i/> in: {xml}");
    }

    #[test]
    fn run_superscript_produces_vert_align_superscript() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        let mut run = document::Run::new("sup");
        run.superscript = true;
        para.push_run(run);
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert!(
            xml.contains(r#"<w:vertAlign w:val="superscript"/>"#),
            "expected vertAlign superscript in: {xml}"
        );
    }

    #[test]
    fn run_subscript_produces_vert_align_subscript() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        let mut run = document::Run::new("sub");
        run.subscript = true;
        para.push_run(run);
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert!(
            xml.contains(r#"<w:vertAlign w:val="subscript"/>"#),
            "expected vertAlign subscript in: {xml}"
        );
    }

    #[test]
    fn superscript_run_emits_vertalign_alone_not_a_shrunk_size() {
        // A super/subscript run must NOT also emit an explicit (reduced) <w:sz>:
        // w:vertAlign already shrinks the glyph and raises it by a fraction of the
        // *effective* em, so a pre-shrunk size collapses the raise and the mark sits
        // mid-line instead of up top (ECMA-376 §17.3.2.42). vertAlign alone, at the
        // inherited body size, is the spec-intended representation.
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        let mut run = document::Run::new("1");
        run.superscript = true;
        run.size_half_pt = Some(13); // a detected, already-reduced superscript size
        para.push_run(run);
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert!(
            xml.contains(r#"<w:vertAlign w:val="superscript"/>"#),
            "expected vertAlign superscript in: {xml}"
        );
        assert!(
            !xml.contains("<w:sz "),
            "a superscript run must use vertAlign alone, not a shrunk <w:sz>: {xml}"
        );
    }

    #[test]
    fn run_monospace_produces_courier_new_font() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        let mut run = document::Run::new("code");
        run.monospace = true;
        para.push_run(run);
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert!(
            xml.contains(r#"w:ascii="Courier New"#),
            "expected Courier New font in: {xml}"
        );
    }

    // ── 2. Paragraph styles ─────────────────────────────────────────────

    #[test]
    fn heading_1_produces_pstyle_heading1() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        para.style = Some(document::ParagraphStyle::Heading(1));
        para.add_run("Title");
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert!(
            xml.contains(r#"<w:pStyle w:val="Heading1"/>"#),
            "expected pStyle Heading1 in: {xml}"
        );
    }

    #[test]
    fn normal_style_produces_pstyle_normal() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        para.style = Some(document::ParagraphStyle::Normal);
        para.add_run("Body text");
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert!(
            xml.contains(r#"<w:pStyle w:val="Normal"/>"#),
            "expected pStyle Normal in: {xml}"
        );
    }

    #[test]
    fn paragraph_properties_follow_schema_and_keep_after_and_exact_line_spacing() {
        // A bottom border and fill belong after style/keep/numbering, and
        // CT_PPr puts spacing before indent. Setting only after-spacing
        // must not discard the separately authored exact line height.
        let mut doc = Document::new();
        let mut para = document::Paragraph {
            style: Some(document::ParagraphStyle::Normal),
            keep_next: true,
            keep_lines: true,
            list_info: Some(document::ListInfo { id: 1, level: 0 }),
            horizontal_rule: true,
            shading: Some("ABCDEF".into()),
            tab_stops: vec![750],
            spacing_after: Some(96),
            line_twips: Some(240),
            left_indent: Some(120),
            alignment: Some(document::Alignment::Right),
            ..document::Paragraph::default()
        };
        para.add_run("paragraph");
        doc.add_paragraph(para);

        let xml = read_zip_entry(&build_docx(&doc), "word/document.xml");
        assert_eq!(
            child_names(&xml, "w:pPr"),
            [
                "w:pStyle",
                "w:keepNext",
                "w:keepLines",
                "w:numPr",
                "w:pBdr",
                "w:shd",
                "w:tabs",
                "w:spacing",
                "w:ind",
                "w:jc",
            ]
        );
        assert!(xml.contains(r#"<w:spacing w:after="96" w:line="240" w:lineRule="exact"/>"#));
    }

    // ── 3. Alignment ────────────────────────────────────────────────────

    #[test]
    fn alignment_center_produces_jc_center() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        para.alignment = Some(document::Alignment::Center);
        para.add_run("centered");
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert!(
            xml.contains(r#"<w:jc w:val="center"/>"#),
            "expected jc center in: {xml}"
        );
    }

    #[test]
    fn alignment_right_produces_jc_right() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        para.alignment = Some(document::Alignment::Right);
        para.add_run("right");
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert!(
            xml.contains(r#"<w:jc w:val="right"/>"#),
            "expected jc right in: {xml}"
        );
    }

    #[test]
    fn alignment_justify_produces_jc_both() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        para.alignment = Some(document::Alignment::Justify);
        para.add_run("justified");
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert!(
            xml.contains(r#"<w:jc w:val="both"/>"#),
            "expected jc both in: {xml}"
        );
    }

    // ── 4. Indent suppression ───────────────────────────────────────────

    #[test]
    fn suppress_indent_produces_first_line_zero() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        para.suppress_indent = true;
        para.add_run("no indent");
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert!(
            xml.contains(r#"w:firstLine="0""#),
            "expected firstLine=0 in: {xml}"
        );
    }

    // ── 5. Hanging indent ───────────────────────────────────────────────

    #[test]
    fn hanging_indent_produces_hanging_and_left() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        para.hanging_indent = Some(document::HangingIndent::Default);
        para.add_run("bibliography entry");
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert!(
            xml.contains(r#"w:hanging="440""#),
            "expected hanging=440 in: {xml}"
        );
        assert!(
            xml.contains(r#"w:left="440""#),
            "expected left=440 in: {xml}"
        );
    }

    // ── 6. Left indent ──────────────────────────────────────────────────

    #[test]
    fn left_indent_produces_w_left() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        para.left_indent = Some(720);
        para.add_run("indented block");
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert!(
            xml.contains(r#"w:left="720""#),
            "expected left=720 in: {xml}"
        );
    }

    // ── 7. Code block ───────────────────────────────────────────────────

    #[test]
    fn code_block_produces_pstyle_codeblock() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        para.code_block = true;
        para.add_run("fn main() {}");
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert!(
            xml.contains(r#"<w:pStyle w:val="CodeBlock"/>"#),
            "expected pStyle CodeBlock in: {xml}"
        );
    }

    // ── 8. List numbering ───────────────────────────────────────────────

    #[test]
    fn list_item_produces_num_pr_with_ilvl_and_num_id() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        para.list_info = Some(document::ListInfo { id: 1, level: 0 });
        para.add_run("list item");
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert!(xml.contains("<w:numPr>"), "expected w:numPr in: {xml}");
        assert!(
            xml.contains(r#"<w:ilvl w:val="0"/>"#),
            "expected ilvl 0 in: {xml}"
        );
        assert!(
            xml.contains(r#"<w:numId w:val="1"/>"#),
            "expected numId 1 in: {xml}"
        );
    }

    // ── 9. Footnote reference (decimal) ─────────────────────────────────

    #[test]
    fn footnote_ref_decimal_emits_footnote_reference() {
        let mut doc = Document::new();
        let fn_id = doc.add_footnote(vec![document::Paragraph {
            inlines: vec![document::InlineElement::Text(document::Run::new(
                "note text",
            ))],
            ..document::Paragraph::default()
        }]);
        let mut para = document::Paragraph::new();
        para.add_run("See");
        para.add_footnote_ref(fn_id);
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        let expected = format!(r#"<w:footnoteReference w:id="{fn_id}"/>"#);
        assert!(xml.contains(&expected), "expected {expected} in: {xml}");
        // Decimal mode should NOT have customMarkFollows
        assert!(
            !xml.contains("customMarkFollows"),
            "decimal mode should not have customMarkFollows"
        );
    }

    // ── 10. Footnote reference (circled) ────────────────────────────────

    #[test]
    fn footnote_ref_circled_is_word_generated() {
        // Circled footnote numbering used to be emitted as a literal ①
        // character with `customMarkFollows`, freezing the global sequence.
        // Word now generates the mark from `decimalEnclosedCircle`: the
        // reference is a plain `w:footnoteReference`, and the footnote body
        // starts with a `w:footnoteRef` mark run — no literal mark anywhere.
        let mut doc = Document::new();
        doc.style.footnote_format = FootnoteFormat::CircledNumber;
        let fn_id = doc.add_footnote(vec![document::Paragraph {
            inlines: vec![document::InlineElement::Text(document::Run::new(
                "circled note",
            ))],
            ..document::Paragraph::default()
        }]);
        let mut para = document::Paragraph::new();
        para.add_footnote_ref(fn_id);
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        let expected = format!(r#"<w:footnoteReference w:id="{fn_id}"/>"#);
        assert!(xml.contains(&expected), "expected {expected} in: {xml}");
        assert!(
            !xml.contains("customMarkFollows"),
            "circled mode should not have customMarkFollows"
        );
        // fn_id=2, seq_num=1 => circled char U+2460 = \u{2460}
        assert!(
            !xml.contains('\u{2460}'),
            "no literal circled digit should be emitted in the body: {xml}"
        );
        let fn_xml = read_zip_entry(&buf, "word/footnotes.xml");
        assert!(
            fn_xml.contains("w:footnoteRef"),
            "footnote body should start with a generated mark run: {fn_xml}"
        );
        assert!(
            !fn_xml.contains('\u{2460}'),
            "no literal circled digit should be emitted in footnote bodies: {fn_xml}"
        );
    }

    // ── 11. Footnote content ────────────────────────────────────────────

    #[test]
    fn footnotes_xml_contains_footnote_text_and_ref() {
        let mut doc = Document::new();
        doc.add_footnote(vec![document::Paragraph {
            inlines: vec![document::InlineElement::Text(document::Run::new(
                "This is a footnote.",
            ))],
            ..document::Paragraph::default()
        }]);
        // Need at least one paragraph referencing it so the doc is valid,
        // but footnotes.xml is generated from doc.footnotes regardless.
        let mut para = document::Paragraph::new();
        para.add_footnote_ref(2);
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/footnotes.xml");
        assert!(
            xml.contains("This is a footnote."),
            "expected footnote text in: {xml}"
        );
        assert!(
            xml.contains("<w:footnoteRef/>"),
            "expected w:footnoteRef in footnotes.xml: {xml}"
        );
        assert!(
            xml.contains(r#"<w:pStyle w:val="FootnoteText"/>"#),
            "expected FootnoteText style in: {xml}"
        );
    }

    #[test]
    fn footnote_linked_image_gets_footnote_local_rel_id() {
        let image = |seed: u8| document::ImageData {
            svg: None,
            bytes: vec![0x89, 0x50, 0x4E, 0x47, seed],
            format: document::ImageFormat::Png,
            width_emu: 914_400,
            height_emu: 457_200,
            src_rect: None,
            alt: None,
        };
        let mut fn_para = document::Paragraph::new();
        fn_para.add_hyperlink_body(
            "https://example.com/脚注?q=\"image\"&keep=1".to_string(),
            vec![document::InlineElement::Image(image(1))],
        );
        fn_para.add_image(image(2));
        let mut doc = Document::new();
        doc.add_footnote(vec![fn_para]);
        let mut para = document::Paragraph::new();
        para.add_footnote_ref(2);
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let fn_xml = read_zip_entry(&buf, "word/footnotes.xml");
        let rels = read_zip_entry(&buf, "word/_rels/footnotes.xml.rels");
        let (_, link) = fn_xml
            .split_once(r#"<w:hyperlink r:id="rIdHyperlink1">"#)
            .unwrap();
        assert!(
            link.split_once("</w:hyperlink>")
                .unwrap()
                .0
                .contains(r#"r:embed="rId1""#)
        );
        assert!(rels.contains(concat!(
            r#"<Relationship Id="rIdHyperlink1" "#,
            r#"Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" "#,
            r#"Target="https://example.com/%E8%84%9A%E6%B3%A8?q=%22image%22&amp;keep=1" TargetMode="External"/>"#,
        )));
        let embeds: Vec<&str> = fn_xml
            .split("r:embed=\"")
            .skip(1)
            .map(|rest| rest.split('"').next().unwrap())
            .collect();
        assert_eq!(embeds.len(), 2, "both images embed in footnotes: {fn_xml}");
        for embed in &embeds {
            assert!(
                rels.contains(&format!("Id=\"{embed}\"")),
                "{embed} must resolve in footnotes.xml.rels: {rels}"
            );
        }
        let targets: Vec<&str> = embeds
            .iter()
            .map(|embed| {
                rels.split(&format!("Id=\"{embed}\""))
                    .nth(1)
                    .and_then(|rest| rest.split("Target=\"media/").nth(1))
                    .and_then(|rest| rest.split('"').next())
                    .unwrap()
            })
            .collect();
        assert_ne!(
            targets[0], targets[1],
            "linked and plain images must point at their own media: {rels}"
        );
        let doc_xml = read_zip_entry(&buf, "word/document.xml");
        let doc_rels = read_zip_entry(&buf, "word/_rels/document.xml.rels");
        assert!(!doc_rels.contains("rIdHyperlink"));
        assert!(
            !doc_xml.contains("r:embed"),
            "this document embeds images only inside its footnote: {doc_xml}"
        );
    }

    // ── 12. Table borders ───────────────────────────────────────────────

    fn auto_table(rows: &[&[u32]]) -> document::Table {
        let cell = |colspan| document::TableCell {
            paragraphs: vec![],
            content: Vec::new(),
            colspan,
            vmerge: document::VMerge::None,
            width_pct: None,
            vertical_align: None,
            shading: None,
            width_twips: None,
            borders: None,
            margins: None,
        };
        document::Table {
            rows: rows
                .iter()
                .map(|spans| document::TableRow {
                    cells: spans.iter().copied().map(cell).collect(),
                    height_twips: None,
                    is_header: false,
                })
                .collect(),
            width_pct: None,
            border_size: None,
            borders: None,
            cell_margins: None,
            width_twips: None,
            col_widths_twips: None,
            alignment: None,
            float: None,
        }
    }

    #[test]
    fn table_produces_tbl_borders() {
        let mut doc = Document::new();
        doc.add_table(auto_table(&[&[1, 1]]));

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert_eq!(child_names(&xml, "w:tbl"), ["w:tblPr", "w:tblGrid", "w:tr"]);
        assert_eq!(child_names(&xml, "w:tblGrid"), ["w:gridCol", "w:gridCol"]);
        assert_eq!(xml.matches("<w:gridCol/>").count(), 2);
        assert!(!xml.contains("w:tblLayout"));
        assert!(
            xml.contains("<w:tblBorders>"),
            "expected w:tblBorders in: {xml}"
        );
        assert!(
            xml.contains(r#"<w:top w:val="single""#),
            "expected top border in: {xml}"
        );
        assert!(
            xml.contains(r#"<w:bottom w:val="single""#),
            "expected bottom border in: {xml}"
        );
    }

    #[test]
    fn auto_table_grid_counts_spanned_columns_across_rows() {
        // Neither cell count nor the first row's width is the grid size.
        let mut doc = Document::new();
        doc.add_table(auto_table(&[&[], &[1, 1], &[3, 1]]));
        let xml = read_zip_entry(&build_docx(&doc), "word/document.xml");
        assert_eq!(xml.matches("<w:gridCol/>").count(), 4);
        assert!(!xml.contains("w:tblLayout"));
        assert!(xml.contains(r#"<w:gridSpan w:val="3"/>"#));
        assert!(xml.contains(r#"<w:tcW w:w="3750" w:type="pct"/>"#));
    }

    #[test]
    fn auto_table_grid_is_bounded_for_overflowing_public_spans() {
        for (spans, columns) in [([1, 1], 2), ([u32::MAX, 1], 63)] {
            let mut doc = Document::new();
            doc.add_table(auto_table(&[&spans]));
            let xml = read_zip_entry(&build_docx(&doc), "word/document.xml");
            assert_eq!(xml.matches("<w:gridCol/>").count(), columns);
            assert_eq!(child_names(&xml, "w:tr"), ["w:tc", "w:tc"]);
            assert!(xml.len() < 4096, "unbounded grid: {} bytes", xml.len());
            if columns == 63 {
                // A giant gridSpan would let the reader expand the grid again,
                // even if the writer emitted only 63 explicit grid columns.
                assert!(xml.contains(r#"<w:gridSpan w:val="62"/>"#));
                assert!(!xml.contains(&u32::MAX.to_string()));
            } else {
                assert_eq!(
                    xml.matches(r#"<w:tcW w:w="2500" w:type="pct"/>"#).count(),
                    2
                );
            }
        }
    }

    #[test]
    fn explicit_table_grid_widths_are_not_auto_capped() {
        let mut doc = Document::new();
        let mut table = auto_table(&[&[1, 1]]);
        table.col_widths_twips = Some(vec![240; 64]);
        doc.add_table(table);
        let xml = read_zip_entry(&build_docx(&doc), "word/document.xml");
        assert_eq!(xml.matches(r#"<w:gridCol w:w="240"/>"#).count(), 64);
        assert!(xml.contains(r#"<w:tblLayout w:type="fixed"/>"#));
    }

    #[test]
    fn empty_auto_tables_have_a_grid_without_inventing_columns() {
        for rows in [&[][..], &[&[][..]][..]] {
            let mut doc = Document::new();
            doc.add_table(auto_table(rows));
            let xml = read_zip_entry(&build_docx(&doc), "word/document.xml");
            assert!(xml.contains("<w:tblGrid/>"));
            assert!(!xml.contains("w:gridCol"));
            assert!(!xml.contains("w:tblLayout"));
        }
    }

    #[test]
    fn floating_fixed_table_keeps_schema_order_without_forbidding_overlap() {
        // A TableFloat may represent an intentional absolute overlay. Do not
        // set an overlap policy without that information in the model.
        let mut doc = Document::new();
        doc.add_table(document::Table {
            rows: vec![document::TableRow {
                cells: vec![document::TableCell {
                    paragraphs: vec![],
                    content: vec![],
                    colspan: 2,
                    vmerge: document::VMerge::None,
                    width_pct: None,
                    vertical_align: None,
                    shading: None,
                    width_twips: Some(1600),
                    borders: None,
                    margins: None,
                }],
                height_twips: None,
                is_header: false,
            }],
            width_pct: None,
            border_size: None,
            borders: None,
            cell_margins: Some(document::TableCellMargins {
                top: 30,
                left: 60,
                bottom: 90,
                right: 120,
            }),
            width_twips: Some(1600),
            col_widths_twips: Some(vec![600, 1000]),
            alignment: None,
            float: Some(document::TableFloat {
                x_twips: -40,
                y_twips: 480,
                distance_twips: 90,
            }),
        });
        let xml = read_zip_entry(&build_docx(&doc), "word/document.xml");
        assert_eq!(child_names(&xml, "w:tbl"), ["w:tblPr", "w:tblGrid", "w:tr"]);
        assert_eq!(
            child_names(&xml, "w:tblPr"),
            [
                "w:tblpPr",
                "w:tblW",
                "w:tblBorders",
                "w:tblLayout",
                "w:tblCellMar"
            ]
        );
        let (position, _) = xml
            .split_once("<w:tblpPr ")
            .unwrap()
            .1
            .split_once("/>")
            .unwrap();
        assert!(!position.contains("w:overlap"));
        assert!(position.contains(r#"w:tblpX="-40" w:tblpY="480""#));
        for side in ["left", "right", "top", "bottom"] {
            assert!(position.contains(&format!(r#"w:{side}FromText="90""#)));
        }
        assert!(!xml.contains("w:tblOverlap"));
        assert!(xml.contains(r#"<w:tblLayout w:type="fixed"/>"#));
        assert!(xml.contains(r#"<w:gridCol w:w="600"/>"#));
        assert!(xml.contains(r#"<w:gridCol w:w="1000"/>"#));
    }

    // ── 13. Settings.xml ────────────────────────────────────────────────

    #[test]
    fn settings_xml_contains_footnote_pr_compat_theme_font() {
        let doc = Document::new();
        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/settings.xml");
        // CT_Settings places math defaults before theme font language.
        assert_eq!(
            child_names(&xml, "w:settings"),
            ["w:footnotePr", "w:compat", "m:mathPr", "w:themeFontLang"]
        );
        assert!(
            xml.contains(r#"w:eastAsia="zh-CN""#),
            "expected zh-CN in themeFontLang: {xml}"
        );
    }

    #[test]
    fn settings_disable_automatic_hyphenation_using_document_property() {
        let mut doc = Document::new();
        doc.style.suppress_auto_hyphens = true;
        doc.mirror_margins = true;
        doc.even_and_odd_headers = true;
        doc.update_fields = true;
        let xml = read_zip_entry(&build_docx(&doc), "word/settings.xml");

        // w:suppressAutoHyphens is paragraph-only. In CT_Settings the
        // document-wide switch belongs before even/odd headers.
        assert!(xml.contains(r#"<w:autoHyphenation w:val="0"/>"#));
        assert!(!xml.contains("w:suppressAutoHyphens"));
        assert_eq!(
            child_names(&xml, "w:settings"),
            [
                "w:mirrorMargins",
                "w:autoHyphenation",
                "w:evenAndOddHeaders",
                "w:updateFields",
                "w:footnotePr",
                "w:compat",
                "m:mathPr",
                "w:themeFontLang",
            ]
        );
    }

    // ── 14. Styles.xml ──────────────────────────────────────────────────

    #[test]
    fn styles_xml_normal_has_jc_left_and_first_line_indent() {
        let doc = Document::new();
        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/styles.xml");
        // Normal style uses left alignment (Typst default is justify: false)
        assert!(
            xml.contains(r#"<w:jc w:val="left"/>"#),
            "expected jc left in Normal style: {xml}"
        );
        // Typst default first-line indent is 0 (no indent)
        assert!(
            xml.contains(r#"w:firstLine="0""#),
            "expected firstLine=0 in Normal style: {xml}"
        );
    }

    #[test]
    fn styles_xml_heading_has_bold_and_outline_level() {
        let doc = Document::new();
        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/styles.xml");
        // Heading styles should have w:b for bold
        assert!(
            xml.contains(r#"w:styleId="Heading1""#),
            "expected Heading1 style definition: {xml}"
        );
        assert!(
            xml.contains("<w:b/>"),
            "expected bold in heading style: {xml}"
        );
        // Outline level 0 for Heading1
        assert!(
            xml.contains(r#"<w:outlineLvl w:val="0"/>"#),
            "expected outlineLvl 0 for Heading1: {xml}"
        );
    }

    #[test]
    fn style_properties_keep_schema_order_for_inherited_formatting() {
        let xml = read_zip_entry(&build_docx(&Document::new()), "word/styles.xml");
        let normal = normal_style(&xml);
        assert_eq!(child_names(normal, "w:style"), ["w:name", "w:pPr", "w:rPr"]);
        assert_eq!(
            child_names(normal, "w:pPr"),
            ["w:widowControl", "w:spacing", "w:ind", "w:jc"]
        );
        assert_eq!(
            child_names(heading1_style(&xml), "w:pPr"),
            ["w:widowControl", "w:spacing", "w:ind", "w:outlineLvl"]
        );
        assert_eq!(
            child_names(style_by_id(&xml, "CodeBlock"), "w:pPr"),
            ["w:shd", "w:spacing", "w:ind"]
        );
        assert_eq!(
            child_names(&xml, "w:pPr"),
            [
                "w:kinsoku",
                "w:wordWrap",
                "w:overflowPunct",
                "w:topLinePunct",
                "w:autoSpaceDE",
                "w:autoSpaceDN"
            ]
        );
    }

    #[test]
    fn deep_headings_reference_defined_styles_with_their_own_outline_levels() {
        let mut doc = Document::new();
        doc.style.heading_sizes = [60, 50, 40, 30, 26];
        for level in 6..=9 {
            let mut para = document::Paragraph {
                style: Some(document::ParagraphStyle::Heading(level)),
                ..document::Paragraph::default()
            };
            para.add_run("deep heading");
            doc.add_paragraph(para);
        }
        let buf = build_docx(&doc);
        let styles = read_zip_entry(&buf, "word/styles.xml");
        let body = read_zip_entry(&buf, "word/document.xml");
        // These headings inherit level 5's font size, not its outline level.
        // A missing style silently turns the paragraph into body text.
        let h5 = style_by_id(&styles, "Heading5");
        for level in 6..=9 {
            let id = format!("Heading{level}");
            assert!(body.contains(&format!(r#"<w:pStyle w:val="{id}"/>"#)));
            let style = style_by_id(&styles, &id);
            assert!(style.contains(&format!(r#"<w:outlineLvl w:val="{}"/>"#, level - 1)));
            assert_eq!(
                style.split_once("<w:rPr>").unwrap().1,
                h5.split_once("<w:rPr>").unwrap().1
            );
            assert!(style.contains(r#"<w:sz w:val="26"/>"#));
        }
    }

    #[test]
    fn styles_xml_has_footnote_styles_when_footnotes_present() {
        let mut doc = Document::new();
        doc.add_footnote(vec![document::Paragraph {
            inlines: vec![document::InlineElement::Text(document::Run::new("fn"))],
            ..document::Paragraph::default()
        }]);
        let mut para = document::Paragraph::new();
        para.add_footnote_ref(2);
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/styles.xml");
        assert!(
            xml.contains(r#"w:styleId="FootnoteReference""#),
            "expected FootnoteReference style: {xml}"
        );
        assert!(
            xml.contains(r#"w:styleId="FootnoteText""#),
            "expected FootnoteText style: {xml}"
        );
    }

    // ── 15. Empty document ──────────────────────────────────────────────

    #[test]
    fn empty_document_no_crash_valid_zip() {
        let doc = Document::new();
        let buf = build_docx(&doc);
        let archive = zip::ZipArchive::new(Cursor::new(&buf)).unwrap();
        assert!(archive.len() > 0, "ZIP should have at least one entry");
    }

    // ── 16. Document without footnotes ──────────────────────────────────

    #[test]
    fn document_without_footnotes_has_no_footnotes_xml() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        para.add_run("no footnotes here");
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        assert!(
            !zip_has_entry(&buf, "word/footnotes.xml"),
            "expected no footnotes.xml when there are no footnotes"
        );
    }

    // ── 17. Document without lists ──────────────────────────────────────

    #[test]
    fn document_without_lists_has_no_numbering_xml() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        para.add_run("no lists here");
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        assert!(
            !zip_has_entry(&buf, "word/numbering.xml"),
            "expected no numbering.xml when there are no lists"
        );
    }

    // ── 18. Math inline element ─────────────────────────────────────────

    #[test]
    fn math_inline_passes_through_omml_xml() {
        let omml = r#"<m:oMath><m:r><m:t>x</m:t></m:r></m:oMath>"#;
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        para.add_math(omml.to_string());
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert!(xml.contains(omml), "expected OMML passthrough in: {xml}");
    }

    // ── 19. Line spacing: Pandoc-aligned (no frozen line height) ─────────

    #[test]
    fn line_spacing_is_pandoc_aligned_no_docgrid_no_line_rule() {
        // Pandoc emits no w:docGrid and no w:line/w:lineRule — it sets only
        // paragraph before/after spacing and lets Word default to single line
        // spacing. tydocx follows suit: freezing the rendered line pitch as
        // w:lineRule="atLeast" surfaced in Word as 行距=最小值, and a docGrid
        // linePitch re-imposed a geometric grid. Neither is emitted now.
        let doc = Document::new();
        let buf = build_docx(&doc);
        let body = read_zip_entry(&buf, "word/document.xml");
        let styles = read_zip_entry(&buf, "word/styles.xml");
        assert!(
            !body.contains("w:docGrid"),
            "should NOT emit a docGrid (Pandoc-aligned): {body}"
        );
        // The "最小值" rule (atLeast) must be gone everywhere; a benign
        // auto single-spacing on e.g. CodeBlock is fine and Pandoc-like.
        assert!(
            !styles.contains(r#"w:lineRule="atLeast""#)
                && !body.contains(r#"w:lineRule="atLeast""#),
            "no style should pin lineRule=atLeast (shows as 行距=最小值): {styles}"
        );
        // The Normal (body) style must not pin a line height at all.
        let normal = normal_style(&styles);
        assert!(
            !normal.contains("w:line=") && !normal.contains("w:lineRule"),
            "Normal style should set no line spacing (Word defaults to single): {normal}"
        );
    }

    fn normal_style(xml: &str) -> &str {
        style_by_id(xml, "Normal")
    }

    fn style_by_id<'a>(xml: &'a str, id: &str) -> &'a str {
        let start = xml
            .find(&format!(r#"w:styleId="{id}""#))
            .unwrap_or_else(|| panic!("missing style {id}"));
        let begin = xml[..start].rfind("<w:style").expect("style open tag");
        let end =
            xml[begin..].find("</w:style>").expect("style close tag") + begin + "</w:style>".len();
        &xml[begin..end]
    }

    // ── 20. CJK properties in docDefaults ──────────────────────────────

    #[test]
    fn cjk_properties_in_doc_defaults() {
        let doc = Document::new();
        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/styles.xml");
        assert!(
            xml.contains("<w:kinsoku/>"),
            "should have kinsoku in docDefaults: {xml}"
        );
        assert!(
            xml.contains("<w:overflowPunct/>"),
            "should have overflowPunct in docDefaults: {xml}"
        );
        assert!(
            xml.contains("<w:autoSpaceDE/>"),
            "should have autoSpaceDE in docDefaults: {xml}"
        );
        assert!(
            xml.contains("<w:autoSpaceDN/>"),
            "should have autoSpaceDN in docDefaults: {xml}"
        );
        assert!(
            xml.contains("<w:wordWrap/>"),
            "should have wordWrap in docDefaults: {xml}"
        );
        assert!(
            xml.contains("<w:topLinePunct/>"),
            "should have topLinePunct in docDefaults: {xml}"
        );
    }

    // ── 21. Heading paragraph control properties ───────────────────────

    #[test]
    fn heading_styles_flow_freely_without_keep_next() {
        let doc = Document::new();
        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/styles.xml");
        // Typst headings are "sticky" (keep-with-next), but replicating that as
        // a blanket w:keepNext makes Word bump a heading — together with its
        // following content — onto the next page even when it would fit, leaving
        // a gap at the bottom of the page. We let Word flow headings freely so
        // available space gets filled. keepNext is emitted nowhere else, so it
        // must not appear in styles.xml at all.
        assert!(
            !xml.contains("<w:keepNext/>"),
            "heading styles must not pin headings to the next paragraph: {xml}"
        );
        // Widow/orphan control stays — it guides Word's line breaking without
        // freezing where a page breaks.
        let h1 = heading1_style(&xml);
        assert!(
            h1.contains("<w:widowControl/>"),
            "Heading1 should still have widowControl: {h1}"
        );
    }

    /// Extract the `<w:style ... w:styleId="Heading1"> … </w:style>` block.
    fn heading1_style(xml: &str) -> &str {
        style_by_id(xml, "Heading1")
    }

    // ── 22. Numbered equation ──────────────────────────────────────────

    #[test]
    fn numbered_equation_produces_right_tab_and_number_text() {
        let omml = r#"<m:oMath><m:r><m:t>E=mc^2</m:t></m:r></m:oMath>"#;
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        para.add_numbered_math(omml.to_string(), "(1)".to_string());
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        // Right tab stop at 8306
        assert!(
            xml.contains(r#"<w:tab w:val="right" w:pos="8306"/>"#),
            "expected right tab stop in: {xml}"
        );
        // Tab character in a run
        assert!(xml.contains("<w:tab/>"), "expected tab element in: {xml}");
        // Equation number text
        assert!(
            xml.contains("(1)"),
            "expected equation number (1) in: {xml}"
        );
        // Suppress first-line indent for equation paragraphs
        assert!(
            xml.contains(r#"w:firstLine="0""#),
            "expected firstLine=0 for equation paragraph: {xml}"
        );
    }

    // ── 23. Image embedding ───────────────────────────────────────────────

    #[test]
    fn image_inline_produces_drawing_xml() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        para.add_image(document::ImageData {
            svg: None,
            bytes: vec![0x89, 0x50, 0x4E, 0x47], // PNG magic bytes
            format: document::ImageFormat::Png,
            width_emu: 914_400,  // 1 inch
            height_emu: 457_200, // 0.5 inch
            src_rect: None,
            alt: None,
        });
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert!(xml.contains("w:drawing"), "expected w:drawing in: {xml}");
        assert!(xml.contains("wp:inline"), "expected wp:inline in: {xml}");
        assert!(xml.contains("a:blip"), "expected a:blip in: {xml}");
        assert!(
            xml.contains(r#"cx="914400""#),
            "expected cx=914400 in: {xml}"
        );
        assert!(
            xml.contains(r#"cy="457200""#),
            "expected cy=457200 in: {xml}"
        );
    }

    #[test]
    fn image_writes_to_zip_media() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        let png_data = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]; // PNG header
        para.add_image(document::ImageData {
            svg: None,
            bytes: png_data.clone(),
            format: document::ImageFormat::Png,
            width_emu: 914_400,
            height_emu: 914_400,
            src_rect: None,
            alt: None,
        });
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        assert!(
            zip_has_entry(&buf, "word/media/image1.png"),
            "ZIP should contain word/media/image1.png"
        );

        // Verify the image bytes are correct
        let mut archive = zip::ZipArchive::new(Cursor::new(&buf)).unwrap();
        let mut img_entry = archive.by_name("word/media/image1.png").unwrap();
        let mut img_bytes = Vec::new();
        std::io::Read::read_to_end(&mut img_entry, &mut img_bytes).unwrap();
        assert_eq!(img_bytes, png_data, "image bytes in ZIP should match input");
    }

    #[test]
    fn image_content_types_include_png() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        para.add_image(document::ImageData {
            svg: None,
            bytes: vec![0x89],
            format: document::ImageFormat::Png,
            width_emu: 100,
            height_emu: 100,
            src_rect: None,
            alt: None,
        });
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let ct = read_zip_entry(&buf, "[Content_Types].xml");
        assert!(
            ct.contains("image/png"),
            "content types should include image/png: {ct}"
        );
    }

    #[test]
    fn image_content_types_include_jpeg() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        para.add_image(document::ImageData {
            svg: None,
            bytes: vec![0xFF, 0xD8],
            format: document::ImageFormat::Jpeg,
            width_emu: 100,
            height_emu: 100,
            src_rect: None,
            alt: None,
        });
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let ct = read_zip_entry(&buf, "[Content_Types].xml");
        assert!(
            ct.contains("image/jpeg"),
            "content types should include image/jpeg: {ct}"
        );
        assert!(
            zip_has_entry(&buf, "word/media/image1.jpg"),
            "ZIP should contain word/media/image1.jpg"
        );
    }

    #[test]
    fn image_rels_include_image_relationship() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        para.add_image(document::ImageData {
            svg: None,
            bytes: vec![0x89],
            format: document::ImageFormat::Png,
            width_emu: 100,
            height_emu: 100,
            src_rect: None,
            alt: None,
        });
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let rels = read_zip_entry(&buf, "word/_rels/document.xml.rels");
        assert!(
            rels.contains("relationships/image"),
            "rels should include image relationship: {rels}"
        );
        assert!(
            rels.contains("media/image1.png"),
            "rels should reference media/image1.png: {rels}"
        );
    }

    #[test]
    fn document_xml_has_image_namespaces() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        para.add_image(document::ImageData {
            svg: None,
            bytes: vec![0x89],
            format: document::ImageFormat::Png,
            width_emu: 100,
            height_emu: 100,
            src_rect: None,
            alt: None,
        });
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert!(
            xml.contains("xmlns:wp="),
            "document should have wp namespace: {xml}"
        );
        assert!(
            xml.contains("xmlns:a="),
            "document should have a namespace: {xml}"
        );
        assert!(
            xml.contains("xmlns:pic="),
            "document should have pic namespace: {xml}"
        );
    }

    #[test]
    fn document_without_images_has_no_image_namespaces() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        para.add_run("no images here");
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert!(
            !xml.contains("xmlns:wp="),
            "document without images should not have wp namespace"
        );
    }

    fn anchored_line() -> document::AnchoredLine {
        document::AnchoredLine {
            x_emu: 1_306_527,
            y_emu: 18_999,
            length_emu: 789_864,
            thickness_emu: 5_588,
            color: None,
        }
    }

    /// Every `prefix:` used by element/attribute names must be declared via
    /// `xmlns:prefix` somewhere in the part — an unbound prefix makes it
    /// unparseable.
    fn assert_bound_prefixes(xml: &str) {
        use std::collections::BTreeSet;
        let mut used = BTreeSet::new();
        let mut declared = BTreeSet::new();
        let mut reader = quick_xml::Reader::from_str(xml);
        let mut buf = Vec::new();
        loop {
            let event = reader
                .read_event_into(&mut buf)
                .unwrap_or_else(|e| panic!("{xml} is not well-formed XML: {e}"));
            match event {
                quick_xml::events::Event::Start(e) | quick_xml::events::Event::Empty(e) => {
                    collect_prefix(e.name().as_ref(), &mut used);
                    for attr in e.attributes().flatten() {
                        let key = attr.key.as_ref();
                        if let Some(rest) = key.strip_prefix(b"xmlns:") {
                            declared.insert(rest.to_vec());
                        } else {
                            collect_prefix(key, &mut used);
                        }
                    }
                }
                quick_xml::events::Event::End(e) => {
                    collect_prefix(e.name().as_ref(), &mut used);
                }
                quick_xml::events::Event::Eof => break,
                _ => {}
            }
            buf.clear();
        }
        // `xml` is reserved by the XML spec; it never needs a declaration.
        used.remove(&b"xml".to_vec());
        let unbound: Vec<_> = used.difference(&declared).collect();
        assert!(
            unbound.is_empty(),
            "unbound prefixes {unbound:?} used but never declared: {xml}"
        );
    }

    fn collect_prefix(name: &[u8], prefixes: &mut std::collections::BTreeSet<Vec<u8>>) {
        if let Some(colon) = name.iter().position(|b| *b == b':') {
            prefixes.insert(name[..colon].to_vec());
        }
    }

    #[test]
    fn anchored_line_declares_drawing_namespaces() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        para.inlines
            .push(document::InlineElement::AnchoredLine(anchored_line()));
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert_bound_prefixes(&xml);
        assert!(xml.contains("xmlns:wp="), "anchored line needs wp: {xml}");
        assert!(xml.contains("xmlns:a="), "anchored line needs a: {xml}");
    }

    #[test]
    fn anchored_line_strokes_black_by_default() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        para.inlines
            .push(document::InlineElement::AnchoredLine(anchored_line()));
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        let ln_pos = xml.find("<a:ln").expect("the line needs an a:ln stroke");
        let ln_end = xml[ln_pos..]
            .find("</a:ln>")
            .map(|i| ln_pos + i)
            .unwrap_or(xml.len());
        assert!(
            xml[ln_pos..ln_end].contains(r#"<a:srgbClr val="000000""#),
            "a colorless anchored line must fill its stroke black: {xml}"
        );
    }

    #[test]
    fn footnote_anchored_line_declares_drawing_namespaces() {
        let mut doc = Document::new();
        doc.add_footnote(vec![document::Paragraph {
            inlines: vec![
                document::InlineElement::Text(document::Run::new("note ")),
                document::InlineElement::AnchoredLine(anchored_line()),
            ],
            ..document::Paragraph::default()
        }]);
        let mut para = document::Paragraph::new();
        para.add_footnote_ref(2);
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/footnotes.xml");
        assert_bound_prefixes(&xml);
        assert!(xml.contains("xmlns:wp="), "anchored line needs wp: {xml}");
    }

    // ── 25. Bookmark start/end ──────────────────────────────────────────

    #[test]
    fn bookmark_produces_bookmark_start_and_end() {
        let mut doc = Document::new();
        let bk_id = doc.next_bookmark_id();
        let mut para = document::Paragraph::new();
        para.add_bookmark(bk_id, "fig-demo".to_string());
        para.add_run("Figure 1: Demo");
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert!(
            xml.contains(r#"<w:bookmarkStart w:id="0" w:name="fig-demo"/>"#),
            "expected bookmarkStart in: {xml}"
        );
        assert!(
            xml.contains(r#"<w:bookmarkEnd w:id="0"/>"#),
            "expected bookmarkEnd in: {xml}"
        );
    }

    // ── 26. Cross-reference field (REF) ─────────────────────────────────

    #[test]
    fn field_ref_produces_fld_char_and_instr_text() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        para.add_field_ref("fig-demo".to_string(), "Figure 1".to_string());
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert!(
            xml.contains(r#"w:fldCharType="begin"#),
            "expected fldChar begin in: {xml}"
        );
        assert!(
            xml.contains("REF fig-demo"),
            "expected REF instrText in: {xml}"
        );
        assert!(
            xml.contains(r#"w:fldCharType="separate"#),
            "expected fldChar separate in: {xml}"
        );
        assert!(xml.contains("Figure 1"), "expected display text in: {xml}");
        assert!(
            xml.contains(r#"w:fldCharType="end"#),
            "expected fldChar end in: {xml}"
        );
    }

    // ── 27. Hyperlink ───────────────────────────────────────────────────

    #[test]
    fn hyperlink_produces_fld_simple_with_url() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        para.add_hyperlink(
            "https://example.com/?a=1&b=%22".to_string(),
            vec![document::Run::new("click here")],
        );
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        // XML escapes the field delimiters once. WPS must see actual quotes
        // in the field code, not the literal text `&quot;` in the address.
        assert!(xml.contains(
            r#"<w:fldSimple w:instr="HYPERLINK &quot;https://example.com/?a=1&amp;b=%22&quot;">"#
        ));
        assert!(!xml.contains("&amp;quot;"));
        assert!(
            xml.contains("click here"),
            "expected display text in: {xml}"
        );
        assert!(
            xml.contains(r#"<w:color w:val="0563C1"/>"#),
            "expected blue color in: {xml}"
        );
        assert!(
            xml.contains(r#"<w:u w:val="single"/>"#),
            "expected underline in: {xml}"
        );
    }

    #[test]
    fn external_hyperlink_relationship_encodes_url_and_escapes_xml_separately() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        para.add_hyperlink(
            "https://example.com/路径?q=\"café\"&keep=x%2Fy#frag".into(),
            vec![document::Run::new("special target")],
        );
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        let rels = read_zip_entry(&buf, "word/_rels/document.xml.rels");
        let (_, link) = xml
            .split_once(r#"<w:hyperlink r:id="rIdHyperlink1">"#)
            .unwrap();
        let (body, _) = link.split_once("</w:hyperlink>").unwrap();
        assert!(body.contains("special target"));
        assert!(body.contains(r#"<w:color w:val="0563C1"/>"#));
        assert!(body.contains(r#"<w:u w:val="single"/>"#));
        assert!(!xml.contains("w:fldSimple"));
        // URL percent-encoding must preserve existing escapes and reserved
        // delimiters. The XML layer then escapes only the literal ampersand.
        assert!(rels.contains(concat!(
            r#"<Relationship Id="rIdHyperlink1" "#,
            r#"Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" "#,
            r#"Target="https://example.com/%E8%B7%AF%E5%BE%84?q=%22caf%C3%A9%22&amp;keep=x%2Fy#frag" TargetMode="External"/>"#,
        )));
    }

    #[test]
    fn malformed_percent_and_non_host_brackets_use_encoded_relationship_targets() {
        // Existing escapes stay byte-for-byte intact, and brackets in IPv6
        // literals do not make path, query, fragment, or userinfo brackets safe.
        let cases = [
            (
                "https://example.com/100%?bad=%Q1&short=%2&good=x%2Fy",
                "https://example.com/100%25?bad=%25Q1&amp;short=%252&amp;good=x%2Fy",
            ),
            (
                "https://example.com/path[1]?q=[a]#[f]",
                "https://example.com/path%5B1%5D?q=%5Ba%5D#%5Bf%5D",
            ),
            (
                "https://user[1]@[2001:db8::1]:8443/path[2]?q=[a]",
                "https://user%5B1%5D@[2001:db8::1]:8443/path%5B2%5D?q=%5Ba%5D",
            ),
            ("//[::ffff:192.0.2.1]/test%", "//[::ffff:192.0.2.1]/test%25"),
            (
                "https://example.com//[::1]",
                "https://example.com//%5B::1%5D",
            ),
        ];
        for (url, expected) in cases {
            let mut doc = Document::new();
            let mut para = document::Paragraph::new();
            para.add_hyperlink(url.into(), vec![document::Run::new("linked")]);
            doc.add_paragraph(para);
            let buf = build_docx(&doc);
            let xml = read_zip_entry(&buf, "word/document.xml");
            let rels = read_zip_entry(&buf, "word/_rels/document.xml.rels");
            assert!(
                xml.contains(r#"<w:hyperlink r:id="rIdHyperlink1">"#),
                "{xml}"
            );
            assert!(!xml.contains("w:fldSimple"));
            assert!(rels.contains(&format!(
                concat!(
                    r#"<Relationship Id="rIdHyperlink1" "#,
                    r#"Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" "#,
                    r#"Target="{}" TargetMode="External"/>"#,
                ),
                expected,
            )), "{rels}");
        }
    }

    #[test]
    fn valid_ipv6_uri_keeps_the_simple_hyperlink_field() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        para.add_hyperlink(
            "https://[2001:db8::1]:8080/x%2F".into(),
            vec![document::Run::new("IPv6")],
        );
        doc.add_paragraph(para);
        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert!(xml.contains(
            r#"<w:fldSimple w:instr="HYPERLINK &quot;https://[2001:db8::1]:8080/x%2F&quot;">"#
        ));
        let rels = read_zip_entry(&buf, "word/_rels/document.xml.rels");
        assert!(!rels.contains("rIdHyperlink"));
    }

    #[test]
    fn header_and_footer_keep_top_level_hyperlinks_and_own_relationships() {
        let mut header = document::Paragraph::new();
        header.add_hyperlink_body(
            "https://example.com/header[1]".into(),
            vec![
                document::InlineElement::Text(document::Run::new("header link")),
                document::InlineElement::Image(document::ImageData {
                    svg: None,
                    bytes: vec![0x89, 0x50, 0x4E, 0x47],
                    format: document::ImageFormat::Png,
                    width_emu: 914_400,
                    height_emu: 457_200,
                    src_rect: None,
                    alt: None,
                }),
            ],
        );
        let mut footer = document::Paragraph::new();
        footer.add_hyperlink(
            "https://example.com/footer%".into(),
            vec![document::Run::new("footer link")],
        );
        let mut doc = Document::new();
        doc.header = Some(document::HeaderFooter {
            paragraphs: vec![header],
        });
        doc.footer = Some(document::HeaderFooter {
            paragraphs: vec![footer],
        });
        let buf = build_docx(&doc);
        for (part, text, target, other) in [
            ("header1", "header link", "header%5B1%5D", "footer%25"),
            ("footer1", "footer link", "footer%25", "header%5B1%5D"),
        ] {
            let xml = read_zip_entry(&buf, &format!("word/{part}.xml"));
            let rels = read_zip_entry(&buf, &format!("word/_rels/{part}.xml.rels"));
            assert_bound_prefixes(&xml);
            let (_, link) = xml
                .split_once(r#"<w:hyperlink r:id="rIdHyperlink1">"#)
                .unwrap();
            let (body, _) = link.split_once("</w:hyperlink>").unwrap();
            assert!(body.contains(&format!(r#"<w:t xml:space="preserve">{text}</w:t>"#)));
            if part == "header1" {
                assert!(body.contains(r#"r:embed="rId1""#));
                assert!(rels.contains(r#"relationships/image" Target="media/image1.png"/>"#));
            }
            assert!(rels.contains(&format!(
                concat!(
                    r#"<Relationship Id="rIdHyperlink1" "#,
                    r#"Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" "#,
                    r#"Target="https://example.com/{}" TargetMode="External"/>"#,
                ),
                target,
            )), "{rels}");
            assert!(!rels.contains(other));
        }
    }

    #[test]
    fn footnote_with_only_an_external_relationship_writes_its_own_rels_part() {
        let mut doc = Document::new();
        let mut body = document::Paragraph::new();
        body.add_hyperlink(
            "https://example.com/α".into(),
            vec![document::Run::new("body")],
        );
        let mut note = document::Paragraph::new();
        note.add_hyperlink(
            "https://example.com/β".into(),
            vec![document::Run::new("note")],
        );
        let id = doc.add_footnote(vec![note]);
        body.add_footnote_ref(id);
        doc.add_paragraph(body);

        let buf = build_docx(&doc);
        for (part, target, other) in [
            ("document", "%CE%B1", "%CE%B2"),
            ("footnotes", "%CE%B2", "%CE%B1"),
        ] {
            let xml = read_zip_entry(&buf, &format!("word/{part}.xml"));
            assert!(xml.contains(r#"<w:hyperlink r:id="rIdHyperlink1">"#));
            let rels = read_zip_entry(&buf, &format!("word/_rels/{part}.xml.rels"));
            assert!(rels.contains(&format!(
                r#"Target="https://example.com/{target}" TargetMode="External""#
            )));
            assert!(!rels.contains(other));
        }
    }

    // ── 28. Page break ──────────────────────────────────────────────────

    #[test]
    fn page_break_produces_br_type_page() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        para.add_page_break();
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert!(
            xml.contains(r#"<w:br w:type="page"/>"#),
            "expected page break in: {xml}"
        );
    }

    // ── 29. Bookmark counter increments ─────────────────────────────────

    #[test]
    fn bookmark_counter_increments() {
        let mut doc = Document::new();
        let id1 = doc.next_bookmark_id();
        let id2 = doc.next_bookmark_id();
        assert_eq!(id1, 0);
        assert_eq!(id2, 1);
    }

    // ── 30. Run underline ─────────────────────────────────────────────

    #[test]
    fn run_underline_produces_w_u_single() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        let mut run = document::Run::new("underlined");
        run.underline = true;
        para.push_run(run);
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert!(
            xml.contains(r#"<w:u w:val="single"/>"#),
            "expected <w:u w:val=\"single\"/> in: {xml}"
        );
        assert!(xml.contains("underlined"));
    }

    // ── 31. Run strikethrough ──────────────────────────────────────────

    #[test]
    fn run_strikethrough_produces_w_strike() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        let mut run = document::Run::new("deleted");
        run.strikethrough = true;
        para.push_run(run);
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert!(
            xml.contains("<w:strike/>"),
            "expected <w:strike/> in: {xml}"
        );
        assert!(xml.contains("deleted"));
    }

    // ── 32. Run highlight ──────────────────────────────────────────────

    #[test]
    fn run_highlight_produces_w_highlight_yellow() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        let mut run = document::Run::new("highlighted");
        run.highlight_color = Some("yellow".into());
        para.push_run(run);
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert!(
            xml.contains(r#"<w:highlight w:val="yellow"/>"#),
            "expected <w:highlight w:val=\"yellow\"/> in: {xml}"
        );
        assert!(xml.contains("highlighted"));
    }

    // ── 33. Run smallcaps ──────────────────────────────────────────────

    #[test]
    fn run_smallcaps_produces_w_smallcaps() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        let mut run = document::Run::new("Small Caps");
        run.smallcaps = true;
        para.push_run(run);
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert!(
            xml.contains("<w:smallCaps/>"),
            "expected <w:smallCaps/> in: {xml}"
        );
        assert!(xml.contains("Small Caps"));
    }

    // ── 34. Horizontal rule ────────────────────────────────────────────

    #[test]
    fn horizontal_rule_produces_pbdr_bottom() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        para.horizontal_rule = true;
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert!(xml.contains("<w:pBdr>"), "expected w:pBdr in: {xml}");
        assert!(
            xml.contains(r#"<w:bottom w:val="single" w:sz="6" w:space="1" w:color="auto"/>"#),
            "expected bottom border attributes in: {xml}"
        );
    }

    // ── 35. Combined cross-reference round-trip ─────────────────────────

    #[test]
    fn bookmark_and_ref_round_trip() {
        let mut doc = Document::new();

        // Target paragraph with bookmark
        let bk_id = doc.next_bookmark_id();
        let mut target_para = document::Paragraph::new();
        target_para.style = Some(document::ParagraphStyle::Heading(1));
        target_para.add_bookmark(bk_id, "intro".to_string());
        target_para.add_run("Introduction");
        doc.add_paragraph(target_para);

        // Paragraph with cross-reference
        let mut ref_para = document::Paragraph::new();
        ref_para.add_run("See ");
        ref_para.add_field_ref("intro".to_string(), "Introduction".to_string());
        doc.add_paragraph(ref_para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");

        // Both bookmark and REF field should be present
        assert!(xml.contains(r#"w:name="intro"#), "bookmark should exist");
        assert!(
            xml.contains("REF intro"),
            "REF field should reference bookmark"
        );
    }

    // ── 36. Page numbering — decimal ───────────────────────────────────

    #[test]
    fn page_numbering_decimal_generates_footer_with_page_field() {
        let mut doc = Document::new();
        doc.page_numbering = Some(PageNumberFormat::Decimal);
        // The format alone does not conjure a footer; the generated footer
        // is requested separately (a bare number was detected in the margin).
        doc.page_number_footers.default = true;
        let mut para = document::Paragraph::new();
        para.add_run("body text");
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        // Footer file should exist
        assert!(
            zip_has_entry(&buf, "word/footer1.xml"),
            "ZIP should contain word/footer1.xml when page_numbering is set"
        );
        let footer = read_zip_entry(&buf, "word/footer1.xml");
        // Should contain PAGE field code
        assert!(
            footer.contains(" PAGE "),
            "footer should contain PAGE instrText: {footer}"
        );
        assert!(
            footer.contains(r#"w:fldCharType="begin"#),
            "footer should contain fldChar begin: {footer}"
        );
        assert!(
            footer.contains(r#"w:fldCharType="separate"#),
            "footer should contain fldChar separate: {footer}"
        );
        assert!(
            footer.contains(r#"w:fldCharType="end"#),
            "footer should contain fldChar end: {footer}"
        );
        // Should be center-aligned
        assert!(
            footer.contains(r#"<w:jc w:val="center"/>"#),
            "footer PAGE field should be centered: {footer}"
        );
    }

    // ── 37. Page numbering — footer NOT static text ────────────────────

    #[test]
    fn page_numbering_footer_does_not_contain_static_page_number_body() {
        let mut doc = Document::new();
        doc.page_numbering = Some(PageNumberFormat::Decimal);
        doc.page_number_footers.default = true;
        let mut para = document::Paragraph::new();
        para.add_run("body text");
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let doc_xml = read_zip_entry(&buf, "word/document.xml");
        // The body should NOT contain the page number as a static run
        // (it's in the footer via PAGE field, not in the body)
        assert!(
            !doc_xml.contains(r#"<w:t xml:space="preserve">1</w:t>"#) || doc_xml.contains("PAGE"),
            "body should not contain static page number text"
        );
    }

    // ── 38. Page numbering — sectPr references footer ──────────────────

    #[test]
    fn page_numbering_sect_pr_references_footer() {
        let mut doc = Document::new();
        doc.page_numbering = Some(PageNumberFormat::Decimal);
        doc.page_number_footers.default = true;
        let mut para = document::Paragraph::new();
        para.add_run("text");
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let doc_xml = read_zip_entry(&buf, "word/document.xml");
        assert!(
            doc_xml.contains("w:footerReference"),
            "sectPr should reference footer1.xml: {doc_xml}"
        );
    }

    // ── 39. Page numbering — pgNumType with decimal ────────────────────

    #[test]
    fn page_numbering_decimal_emits_pg_num_type() {
        let mut doc = Document::new();
        doc.page_numbering = Some(PageNumberFormat::Decimal);
        let mut para = document::Paragraph::new();
        para.add_run("text");
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let doc_xml = read_zip_entry(&buf, "word/document.xml");
        assert!(
            doc_xml.contains(r#"<w:pgNumType w:fmt="decimal"/>"#),
            "sectPr should contain pgNumType decimal: {doc_xml}"
        );
    }

    // ── 40. Page numbering — lower roman ───────────────────────────────

    #[test]
    fn page_numbering_lower_roman_emits_pg_num_type() {
        let mut doc = Document::new();
        doc.page_numbering = Some(PageNumberFormat::LowerRoman);
        let mut para = document::Paragraph::new();
        para.add_run("text");
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let doc_xml = read_zip_entry(&buf, "word/document.xml");
        assert!(
            doc_xml.contains(r#"<w:pgNumType w:fmt="lowerRoman"/>"#),
            "sectPr should contain pgNumType lowerRoman: {doc_xml}"
        );
    }

    // ── 41. Page numbering — content types and rels ────────────────────

    #[test]
    fn page_numbering_generates_content_types_and_rels() {
        let mut doc = Document::new();
        doc.page_numbering = Some(PageNumberFormat::Decimal);
        doc.page_number_footers.default = true;
        let mut para = document::Paragraph::new();
        para.add_run("text");
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let ct = read_zip_entry(&buf, "[Content_Types].xml");
        assert!(
            ct.contains("footer"),
            "content types should reference footer: {ct}"
        );
        let rels = read_zip_entry(&buf, "word/_rels/document.xml.rels");
        assert!(
            rels.contains("footer"),
            "document rels should reference footer: {rels}"
        );
    }

    // ── 42. No page numbering — no footer generated ────────────────────

    #[test]
    fn no_page_numbering_no_footer() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        para.add_run("no page numbers");
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        assert!(
            !zip_has_entry(&buf, "word/footer1.xml"),
            "ZIP should NOT contain footer1.xml when no page numbering and no footer"
        );
    }

    // ── 42b. Numbering format alone does not conjure a footer ──────────

    #[test]
    fn page_numbering_format_without_footer_flag_generates_no_footer() {
        // `page_numbering` is only the `pgNumType` format; the generated
        // footer needs `page_number_footer`. A declared `#set
        // page(numbering:)` with no rendered footer must not conjure one.
        let mut doc = Document::new();
        doc.page_numbering = Some(PageNumberFormat::LowerRoman);
        let mut para = document::Paragraph::new();
        para.add_run("text");
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        assert!(
            !zip_has_entry(&buf, "word/footer1.xml"),
            "ZIP should NOT contain footer1.xml when only the format is set"
        );
        let doc_xml = read_zip_entry(&buf, "word/document.xml");
        assert!(
            !doc_xml.contains("w:footerReference"),
            "sectPr should not reference a footer: {doc_xml}"
        );
        assert!(
            doc_xml.contains(r#"<w:pgNumType w:fmt="lowerRoman"/>"#),
            "sectPr should still carry the declared format: {doc_xml}"
        );
    }

    // ── 43. Citation source on document ────────────────────────────────

    #[test]
    fn citation_source_on_document() {
        use document::{CitationSource, PersonName, SourceType};
        let mut doc = Document::new();
        doc.citation_sources.push(CitationSource {
            tag: "Smi20".into(),
            source_type: SourceType::JournalArticle,
            authors: vec![PersonName {
                last: "Smith".into(),
                first: Some("John".into()),
                middle: Some("A.".into()),
            }],
            title: Some("Example Article".into()),
            year: Some("2020".into()),
            journal_name: Some("Journal of Examples".into()),
            volume: Some("42".into()),
            issue: Some("3".into()),
            pages: Some("100-115".into()),
            doi: Some("10.1234/example".into()),
            url: None,
            publisher: None,
            city: None,
            edition: None,
            book_title: None,
        });
        assert_eq!(doc.citation_sources.len(), 1);
        assert_eq!(
            doc.citation_sources[0].source_type.ooxml_value(),
            "JournalArticle"
        );
    }

    // ── 44. Paragraph add citation ─────────────────────────────────────

    #[test]
    fn paragraph_add_citation() {
        let mut para = document::Paragraph::new();
        para.add_citation(vec!["Smi20".into()], "(Smith, 2020)".into());
        assert_eq!(para.inlines.len(), 1);
        assert!(matches!(
            &para.inlines[0],
            document::InlineElement::Citation {
                keys, display_text, ..
            }
                if keys == &["Smi20"] && display_text == "(Smith, 2020)"
        ));
    }

    // ── 45. Citation SDT with field code ──────────────────────────────

    #[test]
    fn citation_produces_sdt_with_field_code() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        para.add_run("See ");
        para.add_citation(vec!["Smi20".into()], "(Smith, 2020)".into());
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert!(xml.contains("w:sdt"), "expected SDT wrapper in: {xml}");
        assert!(
            xml.contains("w:citation"),
            "expected w:citation marker in: {xml}"
        );
        assert!(
            xml.contains("CITATION Smi20"),
            "expected CITATION field code in: {xml}"
        );
        assert!(
            xml.contains("(Smith, 2020)"),
            "expected display text in: {xml}"
        );
    }

    #[test]
    fn merged_citation_uses_backslash_m() {
        let mut doc = Document::new();
        let mut para = document::Paragraph::new();
        para.add_citation(
            vec!["Smi20".into(), "Jon21".into()],
            "(Smith, 2020; Jones, 2021)".into(),
        );
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert!(
            xml.contains(r"CITATION Smi20 \l 2052 \m Jon21"),
            "expected merged citation field with zh-CN locale in: {xml}"
        );
    }

    // ── 47. Custom XML bibliography data source ───────────────────────

    #[test]
    fn custom_xml_sources_contains_bibliography_entry() {
        use document::{CitationSource, PersonName, SourceType};
        let mut doc = Document::new();
        doc.citation_sources.push(CitationSource {
            tag: "Smi20".into(),
            source_type: SourceType::JournalArticle,
            authors: vec![PersonName {
                last: "Smith".into(),
                first: Some("John".into()),
                middle: Some("A.".into()),
            }],
            title: Some("Example Article".into()),
            year: Some("2020".into()),
            journal_name: Some("Journal of Examples".into()),
            volume: Some("42".into()),
            issue: Some("3".into()),
            pages: Some("100-115".into()),
            doi: Some("10.1234/example".into()),
            url: None,
            publisher: None,
            city: None,
            edition: None,
            book_title: None,
        });
        let mut para = document::Paragraph::new();
        para.add_citation(vec!["Smi20".into()], "[1]".into());
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        assert!(
            zip_has_entry(&buf, "customXml/item1.xml"),
            "expected customXml/item1.xml"
        );
        let xml = read_zip_entry(&buf, "customXml/item1.xml");
        assert!(xml.contains("b:Sources"), "expected b:Sources root: {xml}");
        assert!(
            xml.contains("<b:Tag>Smi20</b:Tag>"),
            "expected b:Tag: {xml}"
        );
        assert!(
            xml.contains("<b:SourceType>JournalArticle</b:SourceType>"),
            "expected source type: {xml}"
        );
        assert!(
            xml.contains("<b:Last>Smith</b:Last>"),
            "expected author last: {xml}"
        );
        assert!(
            xml.contains("<b:First>John</b:First>"),
            "expected author first: {xml}"
        );
        assert!(
            xml.contains("<b:Title>Example Article</b:Title>"),
            "expected title: {xml}"
        );
        assert!(
            xml.contains("<b:Year>2020</b:Year>"),
            "expected year: {xml}"
        );
        assert!(
            xml.contains("<b:JournalName>Journal of Examples</b:JournalName>"),
            "expected journal: {xml}"
        );
    }

    #[test]
    fn bibliography_zip_has_custom_xml_parts() {
        use document::{CitationSource, SourceType};
        let mut doc = Document::new();
        doc.citation_sources.push(CitationSource {
            tag: "Test1".into(),
            source_type: SourceType::Misc,
            authors: vec![],
            title: Some("Test".into()),
            year: None,
            journal_name: None,
            volume: None,
            issue: None,
            pages: None,
            doi: None,
            url: None,
            publisher: None,
            city: None,
            edition: None,
            book_title: None,
        });
        let mut para = document::Paragraph::new();
        para.add_citation(vec!["Test1".into()], "[1]".into());
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        assert!(zip_has_entry(&buf, "customXml/item1.xml"));
        assert!(zip_has_entry(&buf, "customXml/itemProps1.xml"));
        assert!(zip_has_entry(&buf, "customXml/_rels/item1.xml.rels"));

        let props = read_zip_entry(&buf, "customXml/itemProps1.xml");
        assert!(
            props.contains("ds:datastoreItem"),
            "expected datastoreItem: {props}"
        );

        let rels = read_zip_entry(&buf, "customXml/_rels/item1.xml.rels");
        assert!(
            rels.contains("customXmlProps"),
            "expected customXmlProps rel: {rels}"
        );

        let ct = read_zip_entry(&buf, "[Content_Types].xml");
        assert!(
            ct.contains("customXmlProperties"),
            "expected content type override: {ct}"
        );

        let doc_rels = read_zip_entry(&buf, "word/_rels/document.xml.rels");
        assert!(
            doc_rels.contains("customXml"),
            "expected customXml relationship: {doc_rels}"
        );
    }

    #[test]
    fn no_citations_means_no_custom_xml() {
        let doc = Document::new();
        let buf = build_docx(&doc);
        assert!(!zip_has_entry(&buf, "customXml/item1.xml"));
    }

    // ── 49. Bibliography block SDT ────────────────────────────────────

    #[test]
    fn bibliography_block_produces_sdt_with_field() {
        let mut doc = Document::new();
        let mut bib_para = document::Paragraph::new();
        bib_para.add_run("Smith, J. (2020). Example. Journal, 42(3), 100-115.");
        bib_para.hanging_indent = Some(document::HangingIndent::Default);
        doc.body
            .elements
            .push(document::BlockElement::BibliographyBlock {
                paragraphs: vec![bib_para],
            });

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert!(
            xml.contains("w:bibliography"),
            "expected w:bibliography SDT marker: {xml}"
        );
        assert!(
            xml.contains("BIBLIOGRAPHY"),
            "expected BIBLIOGRAPHY field code: {xml}"
        );
        assert!(
            xml.contains("Smith, J."),
            "expected cached bibliography text: {xml}"
        );
    }

    // ── OOXML spec compliance tests ────────────────────────────────────

    #[test]
    fn source_type_thesis_serializes_as_report() {
        assert_eq!(document::SourceType::Thesis.ooxml_value(), "Report");
    }

    #[test]
    fn custom_xml_sources_has_guid() {
        use document::{CitationSource, SourceType};
        let mut doc = Document::new();
        doc.citation_sources.push(CitationSource {
            tag: "Test1".into(),
            source_type: SourceType::Misc,
            authors: vec![],
            title: Some("Test".into()),
            year: None,
            journal_name: None,
            volume: None,
            issue: None,
            pages: None,
            doi: None,
            url: None,
            publisher: None,
            city: None,
            edition: None,
            book_title: None,
        });
        let mut para = document::Paragraph::new();
        para.add_citation(vec!["Test1".into()], "[1]".into());
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "customXml/item1.xml");
        assert!(xml.contains("<b:Guid>"), "expected b:Guid element: {xml}");
        assert!(
            xml.contains("<b:Guid>{"),
            "expected GUID format with braces: {xml}"
        );
    }

    #[test]
    fn bibliography_sdt_has_unique_id() {
        let mut doc = Document::new();
        let mut p1 = document::Paragraph::new();
        p1.add_citation(vec!["Smi20".into()], "[1]".into());
        doc.add_paragraph(p1);
        let mut bib_para = document::Paragraph::new();
        bib_para.add_run("Smith (2020).");
        doc.body
            .elements
            .push(document::BlockElement::BibliographyBlock {
                paragraphs: vec![bib_para],
            });

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert!(
            !xml.contains("111145805"),
            "SDT ID should not be hardcoded: {xml}"
        );
    }

    #[test]
    fn bibliography_sdt_has_doc_part_gallery() {
        let mut doc = Document::new();
        let mut bib_para = document::Paragraph::new();
        bib_para.add_run("Reference entry.");
        doc.body
            .elements
            .push(document::BlockElement::BibliographyBlock {
                paragraphs: vec![bib_para],
            });

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "word/document.xml");
        assert!(
            xml.contains("w:docPartGallery"),
            "expected docPartGallery: {xml}"
        );
        assert!(
            xml.contains("Bibliographies"),
            "expected Bibliographies value: {xml}"
        );
        assert!(
            xml.contains("w:docPartUnique"),
            "expected docPartUnique: {xml}"
        );
    }

    #[test]
    fn custom_xml_item_props_has_unique_guid() {
        use document::{CitationSource, SourceType};
        let mut doc = Document::new();
        doc.citation_sources.push(CitationSource {
            tag: "A".into(),
            source_type: SourceType::Misc,
            authors: vec![],
            title: None,
            year: None,
            journal_name: None,
            volume: None,
            issue: None,
            pages: None,
            doi: None,
            url: None,
            publisher: None,
            city: None,
            edition: None,
            book_title: None,
        });
        let mut para = document::Paragraph::new();
        para.add_citation(vec!["A".into()], "[1]".into());
        doc.add_paragraph(para);

        let buf = build_docx(&doc);
        let xml = read_zip_entry(&buf, "customXml/itemProps1.xml");
        assert!(
            !xml.contains("C5A3B2D1"),
            "itemProps should not use hardcoded UUID: {xml}"
        );
        assert!(xml.contains("ds:itemID=\"{"), "expected GUID format: {xml}");
    }
}
