# Typst ↔ OOXML 交集调查

日期：2026-11-16。基线：`feature/docx-backend` @ `b5c5345`，Typst 0.15.1。
方法：逐项对代码核实（`crates/typort-core/src/convert/`、`crates/typort-ooxml/src/`），
辅以 golden 快照（`tests/snapshots/*.document.xml`，即已固定的真实输出）与现有集成测试断言。
实测转换因本机正在构建 CLI 而尚未跑通（构建仍在后台进行，见末节）；下表凡标“快照证实”的，
均有快照文件或测试断言作证据，其余为代码级核实。

证据分级：**代码** = 实现位置可查；**快照** = golden 快照或集成测试断言证实；
**待实测** = 代码与快照互相矛盾，需跑一次转换才能定论。

## 回退层级与优先级

```mermaid
flowchart LR
    A[A 原生语义对象] --> B[B 原生样式与布局组合]
    B --> C[C OOXML 图形对象]
    C --> D[D 不可编辑矢量图]
    D --> E[E 位图回退]
```

A 级（原生语义对象）最优先：标题、脚注、OMML、表格、编号、交叉引用、TOC。
缺口按“常用程度 × 修复成本”排序，见末节“先补顺序”。

## 已支持（原生，A 级为主）

| Typst 功能 | OOXML 输出 | 证据 |
| --- | --- | --- |
| 字体、字号、颜色、粗斜体 | `w:rPr`（`rFonts`/`sz`/`color`/`b`/`i`），取自 Paged 渲染 | 代码：`convert/page/run_style.rs`（`PagedRunStyle`：font_family、size_pt、color_hex、is_bold、is_italic） |
| 下划线、删除线、上标、下标、等宽、小型大写 | `w:rPr`（`u`/`strike`/`vertAlign`/`rStyle`/`smallCaps`） | 代码：`convert/inline_walk.rs::apply_inline_format` |
| 段落缩进、首行缩进、悬挂缩进 | `w:ind`（left/hanging/firstLine），首行缩进取自源码 AST | 代码：`convert/page/source_ast.rs`（`first_line_indent_*`）、`writer/paragraph.rs`（`write_indentation`） |
| 行距、段间距、对齐 | `w:spacing`（line/after）、`w:jc`；leading 取自源码 AST | 代码：`convert/source.rs`（leading → line_spacing，par.spacing → after） |
| 标题与标题样式 | `w:pStyle` Heading1–9；带 label 的标题加书签 | 快照证实：`aligned_equations`、`edge_theorem_proof` 含 Heading1/2；代码：`convert/headings.rs`、`writer/styles.rs`（Heading 样式，字号表只到 h5，h6–h9 与 h5 同号） |
| 脚注 | `footnotes.xml`，`footnoteRef`/`footnoteReference`，每页重编号；圈号格式（①②③）可检测 | 快照证实：`formatted_footnote` 含 `footnoteReference`；代码：`convert/footnote.rs`（`detect_footnote_format`）、`writer/footnotes.rs`（`numRestart eachPage`） |
| 表格、合并单元格 | `w:tbl`、`w:gridSpan`、`w:vMerge`、嵌套表、列宽百分比、`w:vAlign`、单元格段落对齐 | 快照证实：`edge_complex_table`；代码：`convert/tables.rs`、`convert/table_width.rs`、`convert/table_align.rs` |
| 表格边框 | `w:tblBorders`（top/left/bottom/right/insideH/insideV）、三线表头底线 | 代码：`writer/table.rs`（`tblBorders`、`header_sep`） |
| 编号列表（顶层） | `w:numPr` + `numbering.xml`；有序 decimal、起始值 override；无序 bullet | 快照证实：`edge_deep_nested_list` 含 `w:numPr`；代码：`convert/lists.rs`（只读 `start`）、`writer/numbering.rs` |
| 页眉、页脚 | 首页面边区域文本提取为 header/footer part；页码检测为 PAGE 域（Decimal/LowerRoman/UpperRoman/LowerLetter/UpperLetter 五种格式） | 代码：`convert/page/margin.rs`（`extract_header`/`extract_footer`/`detect_page_numbering`）、`writer/header_footer.rs`、`writer/fields.rs`（PAGE 域） |
| 页面大小、边距 | `w:sectPr`（`pgSz`/`pgMar`），取自 Paged | 代码：`convert/page/margin.rs`、`writer/document.rs` |
| 分栏 | `w:cols`（列数取自源码 AST `#set page(columns:)`，间距默认 720 twips）；只在列数 > 1 时写出 | 代码：`convert/page/source_ast.rs`（`columns`）、`convert/mod.rs`（“列数只来自源码 AST，无几何回退”）、`writer/document.rs` |
| 分节（页尺寸变化） | 段落内 `w:sectPr`，`nextPage` | 代码：`convert/page/sections.rs`（`detect_section_breaks`，仅看页尺寸） |
| 数学公式 | OMML（`m:f`、`m:sSub`/`m:sSup`/`m:sSubSup`、`m:rad`、`m:nary`、`m:d`、`m:m`、`m:eqArr`、`m:acc`、`m:bar`、`m:groupChr`、`m:func`、`m:limLow`/`m:limUpp`、`m:phant`、`m:box` 等），取自 Content 树而非 MathML | 快照证实：`aligned_equations`、`inline_math_in_text`、`edge_theorem_proof` 含 `m:oMath`；硬限制：公式内无颜色、无可伸缩箭头、Word 强制 Cambria Math（`docs/ARCHITECTURE.md`） |
| 图像 | PNG/JPEG 直接嵌入；`wp:inline`，显示尺寸按内容哈希取自 Paged | 代码：`convert/image.rs`（`collect_image_sizes`、`image_data_from_src`）、`writer/image.rs`（仅 `wp:inline`） |
| 交叉引用 | REF 域（` REF name \h `）+ 书签（标题、label 元素）；书目引用走内部超链接 | 代码：`convert/inline_walk.rs`（`handle_inline_ref_tag`，非书目目标 → `add_field_ref`）、`writer/fields.rs` |
| 外部链接 | `HYPERLINK` 域（`fldSimple`） | 代码：`writer/fields.rs` |
| 自动目录 | TOC 域（` TOC \o "1-N" \h \z \u `），深度默认 3，取自 `#outline()` | 代码：`convert/block.rs`（`handle_outline`）、`writer/fields.rs` |
| 文献与书目 | BIBLIOGRAPHY SDT + `b:Sources`（customXml，条目类型映射 article/book/thesis/web 等）；条目悬挂缩进、无项目符号 | 快照证实：`bibliography_basic` 含 BIBLIOGRAPHY SDT；代码：`convert/bibliography.rs`、`writer/citation.rs`；测试：`bibliography.rs` |
| 分页符、分栏符、换行 | `w:br`（type page/column/无 type）；显式 `#pagebreak()`/`#colbreak()` 取自源码 AST | 代码：`convert/breaks.rs` |
| 代码块 | CodeBlock 样式 + 等宽 run，按行分段 | 代码：`convert/block.rs`（`convert_code_block`）、`writer/styles.rs`（CodeBlock） |
| 制表位 | `w:tabs`（编号公式、网格恢复用） | 代码：`writer/paragraph.rs`（`tab_stops`） |

## 缺失

| 编号 | Typst 功能 | 现状 | 证据 | Fixture |
| --- | --- | --- | --- | --- |
| M1 | enum 编号样式（`"a)"`、`"i."`、`"1.1"` 等） | 丢弃。只读 `<ol start>`，其余一律 decimal `"%1."` | 代码：`convert/lists.rs`（只读 `start`）；`writer/numbering.rs`（abstractNum 1 只有 decimal） | `conf-enum-numbering.typ` |
| M2 | 单元格背景（`table(fill:)` / `table.cell(fill:)`） | 缺失。`TableCell` 无 shading 字段；`w:shd` 只在 CodeBlock 样式里写出，单元格不写 | 代码：`typort-ooxml/src/document.rs`（`TableCell` 字段）；`w:shd` 只见于 CodeBlock 样式（`typort-ooxml/src/styles.rs`） | `conf-cell-fill.typ` |
| M3 | 浮动图形（`#place(..., float: true)`） | 缺失。writer 只有 `wp:inline`，无 `wp:anchor`；内容只靠 recovery 以纯文本存活 | 代码：`writer/image.rs`（仅 `wp:inline`）；`convert/` 全仓无 `PlaceElem` 处理 | `conf-place-float.typ` |
| M4 | 网格布局（`#grid`） | 缺失（降级为 B 级 tab 恢复）。不转表；文本以制表位拼回 | 代码：`convert/` 全仓无 `GridElem` 处理；测试已确认现状：`recovery.rs`（`grid_content_recovered_in_output`、`grid_multi_column_has_tab_stops`）只断言文本与 tab 存在 | `conf-grid.typ` |
| M5 | 图形与简单形状（`rect`/`line`/`circle` 等） | 缺失。无 DrawingML shapes；裸形状直接丢弃，只有放进 figure 的画布才栅格化为 PNG | 代码：`convert/` 全仓无 `RectElem`/`CircleElem`/`LineElem` 处理；`convert/image.rs`（`extract_figure_rasters`，只处理 figure 画布） | `conf-shapes.typ` |
| M6 | 分节（栏数/边距变化） | 缺失。只检测页尺寸变化；栏数变化不产生 section；`SectionBreakType::Continuous`/`EvenPage`/`OddPage` 在 IR 里存在但 convert 从不构造 | 代码：`convert/page/sections.rs`（只比较宽高，`columns: None`）；`SectionBreakType::Continuous` 全仓零构造 | `conf-section-columns.typ` |
| M7 | 首页不同 / 奇偶页不同页眉页脚 | 缺失。IR 只有一对 header/footer；只读第一页边区；无 `w:titlePg` | 代码：`typort-ooxml/src/document.rs`（`header`/`footer` 各一个 `Option`）；`convert/page/margin.rs`（只读 `pages()[0]`）；writer 无 `titlePg` | `conf-first-page-header.typ` |
| M8 | 页码交叉引用（PAGEREF） | 缺失。`counter(page).at(label)` 无对应域；fields 只有 REF/HYPERLINK/TOC/PAGE | 代码：`writer/fields.rs`（无 PAGEREF） | `conf-pageref.typ` |
| M9 | 上划线（`#overline`） | 缺失且静默错配。映射为 underline（Word 的 `w:rPr` 本无 overline） | 代码：`convert/inline_walk.rs::apply_inline_format`（`"underline" \| "overline" => run.underline = true`） | `conf-overline.typ` |

## 有损

| 编号 | Typst 功能 | 现状 | 证据 | Fixture |
| --- | --- | --- | --- | --- |
| L1 | 嵌套列表（2 级及以下） | 有损。段落写出 `w:ilvl` 1–3，但 `numbering.xml` 里有序/无序 abstractNum 只定义了 ilvl 0，嵌套级的标记格式未定义，Word 按自身默认处理 | 代码：`writer/numbering.rs`（abstractNum 1/2 只有 level 0）；快照证实：`edge_deep_nested_list` 含 `w:ilvl` 0/1/2/3 | `conf-nested-list.typ` |
| L2 | 标题编号（`#set heading(numbering:)`） | 有损。编号以字面文本 run 前置（“一、”“1.1”等原样写出），不是 Word 的链接编号；移动标题后编号不更新。中文五级 abstractNum 3 已定义但“未自动链接到标题样式” | 代码：`convert/headings.rs`（`heading.numbers` 前置为文本）；`writer/numbering.rs`（abstractNum 3 注释） | `conf-heading-numbering.typ` |
| L3 | 页眉页脚格式 | 有损。边区提取把全部片段拼成一个纯文本段落：字体、字号、颜色、粗体全丢；多行压成一段；对齐只按 x 位置启发式判断 | 代码：`convert/page/margin.rs::extract_margin_zone`（`Run::new(&item.text)`，单段落） | `conf-header-footer-format.typ` |
| L4 | 矢量图（SVG、GIF/WebP、绘图画布） | 有损（D 级）。SVG 按原生像素尺寸栅格化为 PNG；GIF/WebP 重编码为 PNG；CeTZ / `#place` 曲线画布整体栅格化；一律 `wp:inline` | 代码：`convert/image.rs`（SVG→`resvg` 1x 栅格化；GIF/WebP→PNG；`rasterize_frame`） | `conf-svg-image.typ` |
| L5 | 高亮颜色 | 有损。颜色写死 yellow，声明的 fill 被忽略（上游 issue #24） | 代码：`convert/inline_walk.rs`（`"highlight" => Some("yellow")`） | `conf-highlight-color.typ` |
| L6 | 文内引用（`@key`） | 有损，待实测。文内只出字面数字标记 `[1]`；CITATION SDT 的 writer 存在，但 pinned 快照里没有 CITATION 域——代码与快照互相矛盾，须跑一次转换定论 | 代码：`writer/paragraph.rs`（`InlineElement::Citation` → `write_citation_sdt`）；快照：`bibliography_basic` 只有 BIBLIOGRAPHY SDT、无 CITATION、无 `smith2020` 字串；测试只断言标记存在，不区分两者 | `conf-citation-field.typ` |

## 顺带发现的文档与代码漂移（非交集缺口，记下来）

- `docs/ARCHITECTURE.md` 称 `ParagraphStyle` 为 “`Heading(n)`、`Normal`、`Quote`、`Code`、…”的 typed enum，实际只有 `Normal`、`Heading(u8)` 两个变体（`typort-ooxml/src/document.rs`）。Quote 以左缩进段落实现，Code 以 `code_block` 标志 + CodeBlock 样式实现。
- `writer/numbering.rs` 的中文五级 abstractNum 3 无论是否使用都会写出（上游 issue #29 的“无条件写出”正在此列）。
- `heading_sizes` 只有 5 项（h1–h5），h6–h9 与 h5 同号（`typort-ooxml/src/document.rs`）。

## 与上游 issue 的对照（供改造后逐项确认用）

用户已要求：本轮改造完成后，对照上游仓库 issue 逐项确认是否随架构改进得到修复；
未修复的，在架构允许范围内继续修。下面是本次调查中已核实的对应关系，其余待逐项核对：

| 上游 issue | 对应 | 本次核实 |
| --- | --- | --- |
| #24 高亮颜色写死 yellow | L5 | 已核实，代码级 |
| #29 中文五级编号无条件写出 | numbering.rs 常写 abstractNum 3 | 已核实，代码级 |
| #30 脚注与页眉页脚段落丢非文本 inline | L3（页眉页脚已核实；脚注体走 inline 收集，需另核） | 部分核实 |
| #31 表格内列表缺少 numbering part 引用 | L1（numbering 完整性） | 相关，待核对 |
| #33 编译错误丢 span 信息 | 步骤 1（实现代理进行中） | 进行中 |
| #34 写失败留半截文件 | 步骤 1（实现代理进行中，原子写盘） | 进行中 |
| #19 / #20 / #21 / #22 / #23 / #25 / #26 / #27 / #28 / #32 / #35 | 行内收集模式统一、emphasis 旁路、preset 脚注字符串匹配、API 整理、空白 run 误标 smallcaps、东亚字体猜测、等宽/数学字体名子串匹配、三个 body-size 检测器直方图不一致、import 相对项目根解析、margin em 值假定 12pt、换行与段落换行语义 | 未逐项核对，留待改造后按 issue 清单确认 |

## 先补顺序（建议）

按 A 级优先、常用程度 × 修复成本排序。M3/M4/M5 与 L3 的结构性部分在步骤 2
（spike 已推荐 B 路：自有 Output + 一次 realize + owned IR）落地后自然变好做，
先不动，等 IR 切过去再补。

1. **L1 嵌套列表级定义**——在 abstractNum 1/2 里补 ilvl 1–8 的 `w:lvl` 定义。
   小改动，嵌套列表常见。
2. **M1 enum 编号样式**——从 HTML/AST 读 `EnumElem` 的 numbering，按列表生成
   对应的 `numFmt`/`lvlText`（每列表一个 abstractNum 或 `lvlOverride`）。
3. **M2 单元格底色**——`TableCell` 加 shading 字段，写 `w:shd`。学术表格常用。
4. **L5 高亮颜色**——把声明的 fill 透传，不再写死 yellow（#24）。
5. **M9 上划线**——不要静默改写成 underline；改为保留文本 + 降级报告（E 级处理），
   直到有明确的 OOXML 表达。
6. **L2 标题编号**——把标题编号接到 Word 编号（abstractNum 3 已存在，只差链接），
   或明确立为“字面文本”并记入文档。需先定设计。
7. **M8 PAGEREF**——给 `counter(page).at(label)` 加 PAGEREF 域。
8. **L3 页眉页脚格式**——边区提取时保留每片段的 run 样式（字体/字号/颜色/粗体），
   多行分段。
9. **M6 分节（栏数/边距变化）**——`detect_section_breaks` 增加栏数与边距变化检测，
   产出 Continuous/NextPage。
10. **M7 首页/奇偶页眉页脚**——IR 与 writer 增加 `titlePg` 与奇偶 part。
11. **L4 矢量图**——简单形状转 DrawingML shapes；SVG 保留矢量（内嵌 SVG part 或
    DrawingML 回退）。较大。
12. **M3 / M4 / M5**——`wp:anchor` 浮动、grid 转表、形状转 DrawingML。等步骤 2 的
    styled IR 落地后再做——Content 树里位置信息天然存在，现在做只能继续靠几何猜。

## 本次实测说明

- 构建：`CARGO_BUILD_JOBS=1 cargo build -p typort --bin typort` 跑过一次（约 12 分钟，
  依赖编译通过），终因步骤 1 实现代理正在改动的诊断重构而失败：`typort` 二进制 crate
  4 个编译错误（`TyportWorld` 未实现 `std::error::Error`、缺失 import；E0432/E0433/E0599），
  涉及文件均为该代理在改的文件（`main.rs`、`lib.rs`、`world.rs`、`convert/mod.rs`、新增 `diag.rs`）。
  本次调查未动生产代码，失败与本次调查无关。树恢复可编译后，我再跑
  `tests/fixtures/conformance/` 逐项核对输出（尤其 L6 的 CITATION 矛盾），并更新本文档。
- Fixture：`tests/fixtures/conformance/` 下新增一批 `.typ`（每项缺口一个复现，
  另加一个 TOC 阳性对照）及支持文件（`conf-image.svg`、`conf-citation.bib`，
  由已有 fixture 复制而来）。未接入测试套件，未改现有测试文件与生产代码。
- 行文未写测试数量。图用 Mermaid。
