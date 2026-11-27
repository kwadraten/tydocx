//! CJK font handling and detection tests.

use crate::common::{
    fixture_doc_xml, fixture_document_with_world, fixture_package_from_document,
    fixture_styles_xml, paragraph_containing,
};

#[test]
fn issue_cjk_latin_font_mixing_content() {
    let xml = fixture_doc_xml("issue_cjk_latin_font_mixing");
    assert!(xml.contains("中文正文"), "CJK text should be present");
    assert!(xml.contains("English"), "Latin text should be present");
    assert!(xml.contains("2024"), "numbers should be present");
}

#[test]
fn issue_cjk_font_east_asia() {
    let (_, doc) = fixture_document_with_world("issue_cjk_font_east_asia");
    let package = fixture_package_from_document(&doc);
    let doc_xml = package.part_text("word/document.xml");
    let styles_xml = package.part_text("word/styles.xml");
    assert!(
        doc_xml.contains("eastAsia") || styles_xml.contains("eastAsia"),
        "should set w:rFonts eastAsia attribute for CJK (in document or styles)"
    );
    assert!(
        doc_xml.contains("\u{65E5}\u{672C}\u{8A9E}"),
        "Japanese text should be present"
    );
    assert!(
        doc_xml.contains("\u{4E2D}\u{6587}"),
        "Chinese text should be present"
    );
}

/// CJK fonts declared by English family name must reach Word as the localized
/// display name (`SimSun` → `宋体`, `SimHei` → `黑体`, `KaiTi` → `楷体`).
///
/// This translation reads each font's own name table, so it only fires when the
/// font is actually installed. CI installs no CJK fonts (see AGENTS.md), so the
/// test gates each assertion on availability: with the font present it asserts
/// the localized name; absent, it asserts the documented no-op (the English name
/// is kept) and notes the skip. Both branches assert — it never silently passes.
#[test]
fn issue_cjk_font_localized_name() {
    use typst::World as _;

    let (world, doc) = fixture_document_with_world("issue_cjk_font_localized_name");
    let installed = |family: &str| {
        world
            .book()
            .select_family(&family.to_lowercase())
            .next()
            .is_some()
    };
    let (simsun, simhei, kaiti) = (installed("SimSun"), installed("SimHei"), installed("KaiTi"));

    let package = fixture_package_from_document(&doc);
    let doc_xml = package.part_text("word/document.xml");
    let styles_xml = package.part_text("word/styles.xml");

    // Body default (styles.xml) — the bulk of the document inherits it.
    if simsun {
        assert!(
            styles_xml.contains(r#"w:eastAsia="宋体""#),
            "SimSun body default must localize to 宋体, got styles: {styles_xml}"
        );
        assert!(
            !styles_xml.contains(r#"w:eastAsia="SimSun""#),
            "no raw English SimSun should remain once localized"
        );
    } else {
        assert!(
            styles_xml.contains(r#"w:eastAsia="SimSun""#),
            "without SimSun installed the English name is kept (no-op); CI skip"
        );
        eprintln!("note: SimSun not installed — localized-name assertion skipped (expected on CI)");
    }

    for (text, expected) in [
        ("黑体标题内容", if simhei { "黑体" } else { "SimHei" }),
        ("楷体大字内容内容", if kaiti { "楷体" } else { "KaiTi" }),
    ] {
        let para = paragraph_containing(doc_xml, text);
        assert!(
            para.contains(&format!(r#"w:eastAsia="{expected}""#)),
            "{text}: declared East Asian font must reach Word as {expected}, got: {para}"
        );
    }
}

/// A CJK-only fallback font list `#set text(font: ("NSimSun", "Noto Serif SC"))`
/// must not let the never-rendered glyph-fallback (`Noto Serif SC`) become the
/// `w:eastAsia` body default. The positional "fonts[0]=Latin, fonts[1]=CJK" split
/// would pick `Noto Serif SC` (then localize it to the weight name
/// `Noto Serif SC Light`), clobbering the geometry-detected rendered CJK font
/// (`NSimSun`). The fix cross-checks the declared list against the detected fonts.
/// The cross-check needs NSimSun to actually render, so the strong assertion is
/// gated on it being installed (CI has no CJK fonts — see AGENTS.md, Testing).
#[test]
fn issue_cjk_fallback_list_font() {
    use typst::World as _;

    let (world, doc) = fixture_document_with_world("issue_cjk_fallback_list_font");
    let nsimsun_installed = world.book().select_family("nsimsun").next().is_some();

    let package = fixture_package_from_document(&doc);
    let styles_xml = package.part_text("word/styles.xml");

    // Font-INDEPENDENT: the original bug localized the never-rendered fallback
    // to its weight name (`eastAsia="Noto Serif SC Light"`). That must never
    // happen, whatever is installed.
    assert!(
        !styles_xml.contains("Noto Serif SC Light"),
        "a never-rendered fallback must not be localized into eastAsia, got styles: {styles_xml}"
    );

    // Font-DEPENDENT. With NSimSun installed, geometry sees it render, so the
    // cross-check keeps it (localized to 新宋体) and the fallback never reaches
    // eastAsia. Without NSimSun (CI installs no CJK fonts) geometry cannot tell
    // this list from a Latin/CJK pair, and `apply_body_font_split` documents
    // the positional fallback: the raw declared second entry.
    if nsimsun_installed {
        assert!(
            styles_xml.contains(r#"w:eastAsia="新宋体""#),
            "NSimSun body default must localize to 新宋体, got styles: {styles_xml}"
        );
        assert!(
            !styles_xml.contains("Noto Serif SC"),
            "rendered NSimSun must win over the never-rendered fallback, got styles: {styles_xml}"
        );
    } else {
        assert!(
            styles_xml.contains(r#"w:eastAsia="Noto Serif SC""#)
                || styles_xml.contains(r#"w:eastAsia="NSimSun""#),
            "without NSimSun installed eastAsia must be a raw declared name, got styles: {styles_xml}"
        );
        eprintln!("note: NSimSun not installed — positional-fallback branch (expected on CI)");
    }
}

fn run_containing<'a>(xml: &'a str, text: &str) -> &'a str {
    xml.split("<w:r>")
        .skip(1)
        .filter_map(|run| run.split("</w:r>").next())
        .find(|run| {
            run.split_once("<w:t")
                .and_then(|(_, tail)| tail.split_once('>'))
                .and_then(|(_, value)| value.split_once("</w:t>"))
                .is_some_and(|(value, _)| value.trim() == text)
        })
        .unwrap_or_else(|| panic!("no run containing {text:?} in {xml}"))
}

fn latin_font<'a>(xml: &'a str, slot: &str) -> &'a str {
    let (_, fonts) = xml.split_once("<w:rFonts ").expect("direct rFonts");
    let (tag, _) = fonts.split_once('>').expect("end of rFonts");
    let (_, value) = tag
        .split_once(&format!(r#"w:{slot}=""#))
        .unwrap_or_else(|| panic!("missing w:{slot} in {tag}"));
    value.split_once('"').expect("end of font name").0
}

#[test]
fn raw_block_tokens_use_the_ordered_code_font_not_the_body_font() {
    let fonts = std::path::PathBuf::from("tests/fonts");
    let doc = crate::common::fixture_document_with_font_dirs("raw_ordered_font_fallback", &[fonts]);
    let package = fixture_package_from_document(&doc);
    let (_, code_style) = package
        .part_text("word/styles.xml")
        .split_once(r#"w:styleId="CodeBlock""#)
        .expect("CodeBlock style");
    let code_style = code_style.split_once("</w:style>").expect("style end").0;
    // Typst's 0.8em raw size rounds to 18 half-points at the default 11pt text size.
    assert!(code_style.contains(r#"<w:sz w:val="18"/>"#), "{code_style}");
    let xml = package.part_text("word/document.xml");
    let inline = paragraph_containing(xml, "inlineToken");
    assert!(!inline.contains(r#"<w:pStyle w:val="CodeBlock"/>"#));
    for slot in ["ascii", "hAnsi"] {
        assert_eq!(latin_font(code_style, slot), "Zed Sans");
        assert_eq!(
            latin_font(run_containing(xml, "StyledProse"), slot),
            "New Computer Modern"
        );
        assert_eq!(
            latin_font(run_containing(inline, "inlineToken"), slot),
            "Zed Sans"
        );
    }
    for (token, color) in [
        ("def", "D73948"),
        ("blockToken", "4B69C6"),
        ("return", "D73948"),
    ] {
        let paragraph = paragraph_containing(xml, token);
        assert!(paragraph.contains(r#"<w:pStyle w:val="CodeBlock"/>"#));
        let run = run_containing(paragraph, token);
        // Direct formatting takes priority, but a size matching CodeBlock is inherited.
        if run.contains("<w:sz ") {
            assert!(run.contains(r#"<w:sz w:val="18"/>"#), "{run}");
        }
        for slot in ["ascii", "hAnsi"] {
            assert_eq!(latin_font(run, slot), latin_font(code_style, slot), "{run}");
        }
        assert!(
            run.contains(&format!(r#"<w:color w:val="{color}"/>"#)),
            "{run}"
        );
    }
}

#[test]
fn ordered_font_fallback_uses_available_face_before_missing_tail() {
    use typst::World as _;

    let path = crate::common::fixture_path("ordered_font_fallback");
    let fonts = std::path::PathBuf::from("tests/fonts");
    let world = tydocx::TydocxWorld::with_font_dirs(&path, &[fonts]).unwrap();
    let doc = tydocx::convert::convert(&world, &tydocx::ConvertOptions::default())
        .unwrap()
        .document;
    let package = fixture_package_from_document(&doc);
    let xml = package.part_text("word/document.xml");
    let run = run_containing(xml, "Ж");
    assert!(run.contains(r#"w:ascii="Libertinus Serif""#), "{run}");
    assert!(!xml.contains("TyDocx Missing Tail"), "{xml}");

    // This also tests the actual East Asian slot on systems with the thesis font.
    if world.book().contains_family("simsun") {
        let run = run_containing(xml, "感谢模板。");
        assert!(
            run.contains(r#"w:eastAsia="宋体""#) || run.contains(r#"w:eastAsia="SimSun""#),
            "{run}"
        );
        assert!(!xml.contains("TyDocx Missing CJK Tail"), "{xml}");
    }
}

#[test]
fn mixed_font_covers_splits_conflicting_ranges_of_one_text_element() {
    let xml = fixture_doc_xml("mixed_font_covers");
    for (text, expected) in [
        ("A", "Libertinus Serif"),
        ("—", "New Computer Modern"),
        ("B", "Libertinus Serif"),
    ] {
        let run = run_containing(&xml, text);
        assert!(run.contains(&format!(r#"w:ascii="{expected}""#)), "{run}");
        if text == "—" {
            assert!(
                run.contains(&format!(r#"w:eastAsia="{expected}""#)),
                "{run}"
            );
        }
    }
}

#[test]
fn issue_underline_font_size_change() {
    let xml = fixture_doc_xml("issue_underline_font_size_change");
    assert!(xml.contains("<w:u "), "should have underline formatting");
    assert!(
        xml.contains("w:strike"),
        "should have strikethrough formatting"
    );
    assert!(xml.contains("Normal size"), "underlined text present");
    assert!(xml.contains("Regular"), "strikethrough text present");
}

#[test]
fn no_math_fallback_font_on_plain_digit_or_whitespace() {
    // Regression: per-glyph math fallback must not leak a math font onto plain
    // text, and a whitespace-only run must not carry a stray size. Typst shapes
    // the isolated digit '7' in "[7]" with a math-table face; copying the paged
    // run style verbatim used to emit `w:rFonts w:ascii="...Math"` on that digit.
    // Detection now normalizes any FontFlags::MATH face (and any non-letter run
    // whose font differs from baseline) back to the baseline, and drops all
    // overrides on whitespace-only runs. See the `edge_math_fallback_digit` fixture.
    let xml = fixture_doc_xml("edge_math_fallback_digit");

    // (a) No run carries a *Math* face on its rFonts (the OpenType math family
    //     name marker), anywhere in the document.
    for rfonts in xml.split("<w:rFonts").skip(1) {
        let tag = rfonts.split('>').next().unwrap_or("");
        assert!(
            !tag.contains("Math"),
            "no run should carry a math fallback font; found rFonts: <w:rFonts{tag}>"
        );
    }

    // (b) The bare digit '7' must still survive as body text.
    assert!(
        xml.contains(">7<"),
        "expected the digit 7 to survive as a run"
    );

    // (c) A whitespace-only run must not carry a size override.
    for run in xml.split("<w:r>").skip(1) {
        let body = run.split("</w:r>").next().unwrap_or("");
        let Some(after_t) = body.split("<w:t").nth(1) else {
            continue;
        };
        let text = after_t
            .split_once('>')
            .and_then(|(_, rest)| rest.split("</w:t>").next())
            .unwrap_or("");
        if !text.is_empty() && text.chars().all(char::is_whitespace) {
            assert!(
                !body.contains("<w:sz "),
                "whitespace-only run must inherit size, got run: {body}"
            );
        }
    }
}

#[test]
fn variable_font_weight_bold_detection() {
    // Variable fonts (typst 0.15) report continuous weights; the paged-side bold
    // detection (weight >= 700, convert/page/run_style.rs) must see the instantiated
    // instance's weight, not the file default. Fonts vendored in tests/fonts.
    let fonts = std::path::PathBuf::from("tests/fonts");
    let doc_xml = crate::common::fixture_part_with_font_dirs(
        "variable_font_weight",
        "word/document.xml",
        &[fonts],
    );

    let heavy = paragraph_containing(&doc_xml, "Heavy weight paragraph.");
    assert!(
        heavy.contains("<w:b/>"),
        "weight-700 VF text must be bold:\n{heavy}"
    );
    let light = paragraph_containing(&doc_xml, "Light weight paragraph.");
    assert!(
        !light.contains("<w:b/>"),
        "weight-300 VF text must NOT be bold:\n{light}"
    );
    let regular = paragraph_containing(&doc_xml, "Regular weight paragraph.");
    assert!(
        !regular.contains("<w:b/>"),
        "default-weight VF text must NOT be bold:\n{regular}"
    );
}

#[test]
fn variable_font_style_italic_detection() {
    // Some variable fonts carry italics on an `ital` or `slnt` axis instead of
    // a separate italic face. `info().variant.style` (the file-default named
    // instance) stays Normal for such fonts even when `#text(style: "italic")`
    // resolves a nonzero axis coordinate at shape time — the paged-side italic
    // detection (convert/page/run_style.rs `effective_style`) must read the resolved
    // coordinate, not the file default. Font: tests/fonts/TyportSlantTest*,
    // a derivative of Karla with a synthetic `slnt` axis (no `ital` axis is
    // present in either font; `slnt` is what typst resolves for an italic
    // request when only `slnt` is available — see FontVariations::resolve).
    let fonts = std::path::PathBuf::from("tests/fonts");
    let doc_xml = crate::common::fixture_part_with_font_dirs(
        "variable_font_style",
        "word/document.xml",
        &[fonts],
    );

    let slanted = paragraph_containing(&doc_xml, "Slanted paragraph.");
    assert!(
        slanted.contains("<w:i/>"),
        "slnt-axis italic VF text must be italic:\n{slanted}"
    );
    let upright = paragraph_containing(&doc_xml, "Upright paragraph.");
    assert!(
        !upright.contains("<w:i/>"),
        "upright VF text must NOT be italic:\n{upright}"
    );
}

#[test]
fn deliberate_digit_run_font_override_is_kept() {
    // A digits-only run the author explicitly set in a non-body font must keep
    // its w:rFonts — only a true OpenType MATH-table fallback is normalized away.
    // See the `edge_digit_run_font` fixture.
    let xml = fixture_doc_xml("edge_digit_run_font");
    // The run "12345" must survive carrying its declared monospace face.
    let para = paragraph_containing(&xml, "12345");
    assert!(
        para.contains("DejaVu Sans Mono"),
        "deliberate per-run font on a digit run must be preserved:\n{para}"
    );
}

#[test]
fn issue_declared_lang_cjk_latin_only() {
    // Declared Chinese with Latin-only body text: the East-Asian hint and
    // language come from the declaration, not from rendered glyphs.
    let styles_xml = fixture_styles_xml("issue_declared_lang_cjk_latin_only");
    assert!(
        styles_xml.contains(r#"w:hint="eastAsia""#),
        "declared CJK language must emit the East-Asian hint:\n{styles_xml}"
    );
    assert!(
        styles_xml.contains(r#"w:eastAsia="zh-CN""#),
        "declared zh must drive the East-Asian language tag:\n{styles_xml}"
    );
}

#[test]
fn issue_declared_lang_en_latin_only() {
    // Declared English with Latin-only body text: no East-Asian hint — the
    // declaration, not the glyphs, decides the document script.
    let styles_xml = fixture_styles_xml("issue_declared_lang_en_latin_only");
    assert!(
        !styles_xml.contains(r#"w:hint="eastAsia""#),
        "declared Latin language must not emit the East-Asian hint:\n{styles_xml}"
    );
}

#[test]
fn issue_declared_lang_ja() {
    // Declared Japanese with Japanese body text: the East-Asian language
    // must be ja-JP, not the glyph fallback zh-CN.
    let styles_xml = fixture_styles_xml("issue_declared_lang_ja");
    assert!(
        styles_xml.contains(r#"w:hint="eastAsia""#),
        "declared CJK language must emit the East-Asian hint:\n{styles_xml}"
    );
    assert!(
        styles_xml.contains(r#"w:eastAsia="ja-JP""#),
        "declared ja must drive the East-Asian language tag:\n{styles_xml}"
    );
    assert!(
        !styles_xml.contains(r#"w:eastAsia="zh-CN""#),
        "the glyph fallback zh-CN must not leak into a declared-ja document:\n{styles_xml}"
    );
}

#[test]
fn issue_undeclared_lang_cjk_only() {
    // No declared language, Japanese-only body: the glyph fallback must
    // still detect CJK content — hint present, fallback language zh-CN.
    let styles_xml = fixture_styles_xml("issue_undeclared_lang_cjk_only");
    assert!(
        styles_xml.contains(r#"w:hint="eastAsia""#),
        "CJK-only body must emit the East-Asian hint even without Latin text:\n{styles_xml}"
    );
    assert!(
        styles_xml.contains(r#"w:eastAsia="zh-CN""#),
        "undeclared CJK body must fall back to zh-CN:\n{styles_xml}"
    );
}
