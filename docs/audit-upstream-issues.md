# 上游 issue 逐项核查（本轮改造后）

日期：2026-11-16。基线：`feature/docx-backend` @ `9bf3fa4`（含步骤 1 已提交改动），Typst 0.15.1。
上游：dianzuan/typort 的 open issue，核查时仍是 17 个（无新增、无关闭）。

方法：逐项读相关代码；有疑问的跑单 fixture 转换看实际输出（CLI 为步骤 1 后的
`target/debug/typort`；fixture 放在 `/tmp/audit`，未提交进仓库）。
结论分三档：**已修复** / **部分修复** / **仍在**。

## 结论总表

| 上游 issue | 结论 | 证据（代码位置或输出片段） | 交集对照 |
| --- | --- | --- | --- |
| #19 统一四种 inline 收集模式 | 仍在 | `inline_walk.rs`：`collect_inlines` 仍按 `options.purpose` 在 Text / Tag / Frame / Element 四处分支；`handle_inline_html_element` 的 `"code"` 分支仍与 strong/emph 分支重复，`sup`/`sub` 仍只差一个字段 | — |
| #20 handle_emphasis_tag 绕过 InlineFmt | 仍在 | `inline_walk.rs::handle_emphasis_tag` 的 else 分支仍手写 `run.bold = true` / `run.italic = true`，未走 `InlineFmt::apply_to` | — |
| #21 preset 脚注格式字符串匹配 | 仍在 | `typort-presets/src/lib.rs:30`：`format: Option<String>`；`:53-55`：`match format.as_str() { "circled" => CircledNumber, _ => Decimal }`，与 issue 描述一字不差 | 脚注格式 |
| #22 API 整理 | 仍在 | `document.rs:9`：`ParagraphStyle::ooxml_value(&self) -> String`，而同文件 `:29`、`:104`、`:643` 返回 `&'static str`，且 `VerticalAlign::ooxml_value(self)` 按值取 self；`writer/package.rs:74`：`write_indentation` 仍是四个 positional `Option<&str>`，调用点 8 处（`styles.rs` 4 处、`writer/paragraph.rs` 4 处）；`typort-math/src/lib.rs:783`：`convert_group_character` 仍由函数内 `macro_rules!` 构成，仍用于 `:184` 的空 `else if` 分支；`#[deprecated]` 别名仍在：`document.rs:40`、`:119`（`as_ooxml_str`）、`:653`（`as_val`）、`typort-presets/src/lib.rs:129`（`load_builtin_preset`） | — |
| #23 空白 run 被标 smallcaps | 仍在 | `smallcaps.rs:21-27`：`trimmed` 为空串时 `t.contains("")` 恒真，空白 run 必被标；实测 `#smallcaps[A] #smallcaps[B]` 输出单个带 `<w:smallCaps/>` 的 run `"A B"`，空格也被盖上 smallcaps | — |
| #24 高亮色写死黄色 | 仍在 | `inline_walk.rs:508`：`"highlight" => run.highlight_color = Some("yellow".into())`；实测 `#highlight(fill: rgb("#ffcccc"))` 仍出 `yellow` | L5 |
| #25 东亚字体/语言按字形猜测 | 部分修复 | 声明语言已驱动 `w:lang`（`source.rs::apply_language_override`，`#set text(lang:)` 优先）；但仍按字形决策：eastAsia 字体槽（`run_style.rs:399`）、默认语言 `has_cjk → "zh-CN"`（`page/style.rs`）、字体脚本切分（`run_style.rs:194`、`page/style.rs:717`）。`text_norm.rs` 的 CJK 空格剥离也是字形判断，但那是排版规则而非语言决策，不在此列 | — |
| #26 等宽/数学字体按名字子串探测 | 仍在 | `page/style.rs:178-185`：`fl.contains("mono") \|\| fl.contains("courier") \|\| fl.contains("consol") \|\| fl.contains("fira code") \|\| fl.contains("source code")`；另有一处同类判断：`recovery/lines.rs:180` 的 `font_name.contains("Math")` | — |
| #27 三个 body-size 检测器直方图不一致 | 仍在 | 三处仍各用各的加权、取样与分桶：`page/style.rs`（前 3 页、glyph 数加权、half-pt 分桶）、`page/run_style.rs::detect_rendered_body_style`（全部页、`text.len()` 字节长加权、half-pt 分桶）、`recovery/lines.rs`（仅首页、`text.len()` 字节长加权、0.1pt 分桶）。CJK 文本按字节长算会得 3 倍权重，与 glyph 数加权结果可不一致 | — |
| #28 import 按项目根解析 | 部分修复 | `page/reachable.rs::collect_reachable_source_texts` 已按导入文件目录解析（`mod.rs:354` 的水平线路径在用）；但 `source.rs::gather_source_overrides`（`:99-106`）仍 `world.root().join(...)` 且只读主文件一层导入；实测：主文件 import `sub/a.typ`、a.typ import `./b.typ`、b.typ 含 `#set par(first-line-indent: 2em)`，输出无 `w:firstLine`，嵌套的 set 规则被漏掉 | — |
| #29 中文五级编号无条件写出 | 仍在 | `writer/numbering.rs:37-40`：abstractNum 3 无条件写出；精确化：`numbering.xml` 本身只在文档有列表时写出（`writer/mod.rs:127` 的 `RelKind::Numbering` 门控），"无条件"指的是只要写出就带 abstractNum 3；实测纯英文 `- one / - two` 列表仍含 `chineseCountingThousand`，且无任何 `w:num` 引用它 | numbering 常写 abstractNum 3 |
| #30 脚注与页眉页脚段落丢非文本 inline | 部分修复 | 脚注体：文本与公式存活——实测 `footnotes.xml` 含 `$x^2+y^2$` 的 OMML；链接退化为纯文本（href 丢），图片与 frame 丢（`collect_footnote_inline_tag` 只处理 equation；`writer/run.rs:10-22` 的 `write_inline` 只写 Text/Math，其余静默丢弃）。页眉页脚：仍在——writer 只写 Text run（`writer/header_footer.rs::write_header_footer_paragraph`），提取也是纯文本（L3 已实测） | L3（页眉页脚） |
| #31 表格内列表缺 numbering 引用 | 仍在 | `tables.rs::convert_cell_paragraphs` 用 Generic 目的收集，无任何列表处理；实测单元格内两项列表并成一段纯文本 `"first itemsecond item"`，无 `w:numPr`、无 numbering part，比 issue 描述的"无项目符号"更重——连项与项的分隔都丢了 | L1 |
| #32 边距 em 值假定 12pt | 仍在 | `page/units.rs:51`：`Unit::Em => value * 12.0`；实测 `#set text(size: 10.5pt)` + `#set page(margin: 2em)` 出 `pgMar 480` twips，应为 420 twips（2 × 10.5pt），误差约 14%，与 issue 所述一致 | — |
| #33 编译错误丢 span | 已修复 | 步骤 1：`convert` 返回 `Vec<SourceDiagnostic>`（`convert/mod.rs:134`），CLI 打印 `file:line:col`；实测：`a33.typ:2:2: unknown variable: unknownvar`，exit 1 | 步骤 1 |
| #34 写盘失败留半截文件 | 已修复 | 步骤 1：`main.rs::write_docx_atomically`（同目录 staging + rename，失败删 staging）；实测：输出路径为目录时 exit 1，报错 `failed to write .docx: Is a directory`，无残留 staging 文件、无半截输出 | 步骤 1 |
| #35 换行变新行、行断变段断 | 仍在 | 实测三则：普通段落换行正确（`"Hello world"` 一段）；`#block` 内换行变段落断裂——长块输出两个 `w:p`，短块 `#block[ab\ncd]` 输出三个 `w:p`（正确的 `"ab cd"` 之外另有 recovery 插入的 `"ab"`、`"cd"` 两段）；`#linebreak()` 正文路径正确出 `<w:br/>`，但在列表项内（Generic 路径）被丢弃，词粘连成 `"item onecontinued"`，全文 0 个 `w:br`。机制：HTML walk 的段落本身正确，问题在 recovery 行插入按可视行拆段、去重未能与已有段落合并 | — |

汇总：已修复 2 项（#33、#34），部分修复 3 项（#25、#28、#30），仍在 12 项。

## 逐项细节（总表之外需要展开的）

### #35 的机制：HTML walk 没错，recovery 行插入拆了段

短块 `#block[ab\ncd]`（只有一行可视行）输出三个段落：居中的正确段落 `"ab cd"`、
recovery 插入的 `"ab"`、`"cd"`。长块则只剩两行各一段，正确段落反被去重吞掉。
所以修法不在 HTML walk，而在 recovery 插入与去重的交界：插入前按文本比对已有段落，
或等步骤 4 拿掉 recovery 层后此类问题自然消失。Generic 路径丢 `<br>` 是另一处：
`handle_inline_html_element` 的 `"br"` 分支只在 Paragraph 目的路径存在，
Generic / FormattedRun / Footnote 目的没有对应处理。

### #28 的两处调用点

`collect_reachable_source_texts` 修的是"可达源码文本"（水平线恢复在用），
`gather_source_overrides` 修的是"set 规则收集"——issue 说的"set 规则被漏掉"正是后者。
修法是让后者复用前者（已按导入文件目录解析、带 visited 去重、`/…` 仍按根解析），
而不是在 source.rs 里再写一份解析。

### #27 的三处差异

| 检测器 | 取样 | 加权 | 分桶 |
| --- | --- | --- | --- |
| `page/style.rs` | 前 3 页 | glyph 数 | half-pt |
| `page/run_style.rs` | 全部页 | `text.len()`（字节长） | half-pt |
| `recovery/lines.rs` | 仅首页 | `text.len()`（字节长） | 0.1pt |

三处都在 `stats::dominant_key` 上汇合，但输入的权重、取样与分桶全不同，
同一文档可以得出三个 body size。统一时按 issue 建议取 glyph 数加权、同一取样，
常量收一处；输出会变，需带 fixture 并重生成受影响的 golden（审 diff）。

### #30 的脚注与页眉页脚不是同一状态

脚注体已过半：`collect_inlines` 的 Footnote 目的 + `write_inline` 让文本与 OMML
都活着，丢的是链接的 href、图片与 frame。页眉页脚则是两层都只认文本：
提取（`extract_margin_zone`）拼纯文本段落，writer（`write_header_footer_paragraph`）
再只写 Text run。修脚注体是补 writer 的 inline 分支；修页眉页脚要从提取开始保留
每片段的 run 样式（与 L3 同一项工作）。

## 修复分类与先修顺序

分类：**现在就能修**（当前架构允许）/ **需等步骤 2 的 styled IR** /
**需先定设计决策** / **不修**。

结论：17 项里没有一项严格需要等步骤 2——步骤 1 的拆分（compile/export、
span 诊断、原子写盘）已落地，剩下的缺口都在当前架构内可修。步骤 2/4 会让
#35 一类 recovery 问题自然消失，但不等它也能在去重层小修。

```mermaid
flowchart LR
    W1[第一波：数据丢失] --> W2[第二波：小而确定的 bug]
    W2 --> W3[第三波：检测器与启发式统一]
    W3 --> W4[第四波：重构类]
```

先修顺序（按"丢失程度 × 常见程度 ÷ 修复成本"排）：

1. **#31 表格内列表**——单元格列表丢结构又丢分隔，学术表格常见；单元格段落复用 body 的 `numPr` 处理。
2. **#23 空白 run smallcaps**——`smallcaps.rs` 里跳过空白 run，一行 guard。
3. **#32 边距 em**——边距 em 换算延后到 body size 已知时（leading/indent 已是这个做法）。
4. **#35 换行与行断**——recovery 插入前与已有段落比对去重；Generic 路径补 `<br>` 处理。
5. **#30 脚注与页眉页脚**——`write_inline` 补链接/图片分支；页眉页脚提取保留 run 样式（与 L3 合并）。
6. **#24 高亮颜色**——把声明的 fill 透传（L5）。
7. **#20 emphasis 旁路**——经 `apply_to` 或加 `InlineFmt::or_into`；issue 称输出 byte-identical，低风险。
8. **#21 preset 脚注格式**——`FootnotePreset.format` 改 serde enum，加载时拒收未知值。
9. **#28 import 解析**——`gather_source_overrides` 复用 `collect_reachable_source_texts`。
10. **#29 中文五级编号**——需先定设计：删掉，还是只在需要时写出；两种都会改 golden。
11. **#25 东亚语言**——声明语言优先、字形作回退（把声明语言传入检测路径）。
12. **#26 字体探测**——经 font book 读 OS/2 `isFixedPitch`/panose 与 math 表（`language.rs` 已有重解析 Face 的先例）。
13. **#27 检测器统一**——一个检测器、一处常量；输出会变，需 fixture + golden 重生成。
14. **#19 inline 收集统一**——行为中性部分先行，行为变化另配 fixture。
15. **#22 API 整理**——receiver 与 `write_indentation` 结构体化、`convert_group_character` 查表化现在就能修；`#[deprecated]` 别名移除等 0.3.0 版本决策。

不修：无。L6（文内引用只出字面标记、不发 CITATION 域）是已定的设计决策，
不是上游 issue，不在本次核查内。

## 与 docs/conformance-intersection.md 的对照出入

- **#30**：交集文档写"部分核实：页眉页脚实测证实；脚注体仍待核"。本次已核完：
  脚注体文本与公式存活，链接退化纯文本，图片与 frame 丢；页眉页脚仍在（writer
  与提取两层）。结论从"待核"变为"部分修复（脚注体）/ 仍在（页眉页脚）"。
- **#31**：交集文档写"相关，待核对"。本次实测确认仍在，且比 issue 描述更重：
  列表项被并成一段纯文本，分隔也丢了。
- **#33 / #34**：交集文档写"已修复（独立验收通过）"。本次用 CLI 实测复核，
  仍成立（span 输出与原子写盘失败无残留）。
- **#24 / #29**：交集文档写"实测证实，仍在"。本次用新 fixture 复核，仍成立；
  另补一个精确化：#29 的 `numbering.xml` 本身只在有列表时写出
  （`RelKind::Numbering` 门控），"无条件"指的是只要写出就带 abstractNum 3。
- **#35**：交集文档未覆盖。本次确认仍在，并定位机制：HTML walk 的段落本身正确，
  问题在 recovery 行插入 + 去重失效；另发现 `#linebreak()` 在列表项内被丢弃。
- **#28**：交集文档未覆盖。本次发现是部分修复（`reachable.rs` 已修，
  `gather_source_overrides` 未修），不是"未核对"。
- **#25**：交集文档未覆盖。本次确认部分修复（声明语言已驱动 `w:lang`，
  其余字形决策仍在）。

## 本次实测说明

- 转换用的 CLI 是步骤 1 后的树编出的 `target/debug/typort`；每项只跑单 fixture，
  未全量跑测试。
- 复核用 fixture（`/tmp/audit`，未提交）：`a35`（block 换行 + linebreak）、
  `c35`（短块机制定位）、`b35`（普通段落换行 + 列表项内 linebreak）、`a23`、
  `a24`、`a30`（脚注内公式与链接）、`a31`（单元格内列表）、`a32`（10.5pt 正文 + 2em 边距）、
  `a33`（编译错误 span）、`a29`/`b29`（numbering 有无）、`main28`（嵌套 import 的 set 规则）。
- 行文未写测试数量。图用 Mermaid。
