"""Assemble one report per complete template, including an all-page alignment table."""

import argparse
from pathlib import Path
from evidence import read
from produce import OUT, json_write


def cell(value):
    return str(value).replace("|", "\\|").replace("\n", " ")


def row(*values):
    return "| " + " | ".join(map(cell, values)) + " |"


def anchors(page):
    if not page["normalized_chars"]:
        return "纯图形 / 空白" if page["image"]["ink_px"] else "空白"
    heads = [b["text"].strip() for b in page["blocks"] if b["normalized_length"] > 5 and b["zone"] == "middle"][:2]
    if not heads:
        heads = [b["text"].strip() for b in page["blocks"] if b["text"].strip()][:2]
    return " / ".join(h.replace("\n", " ")[:74] for h in heads)


def pct(stat, long=False):
    c, p = ("long_covered", "long_chars") if long else ("covered", "chars")
    return f"{100 * stat[c] / stat[p]:.1f}%" if stat[p] else "n/a"


def report(identifier, labels, details):
    directory = OUT / identifier
    source = read(directory / "source.json")
    data = read(directory / "summary.json")
    docx = read(directory / "docx.json")
    mapping = read(directory / "alignment.json")
    manual = read(OUT / "manual-alignment.json").get(identifier, [])
    if manual:
        mapping["reviewed_content_ranges"] = manual
        json_write(directory / "alignment.json", mapping)
    ex = next(status[identifier] for path in sorted(OUT.glob("status-wps*.json"))
              if (status := read(path)).get(identifier, {}).get("wrote"))
    t, w = [read(directory / f"{side}-pages.json")["pages"] for side in ("typst", "wps")]
    doc = [
        f"# {identifier} {source['version']} — 全文对照", "",
        f"{labels.get(identifier, '')}",
        f"- 源：{source_link(source)} · `{source['license']}` · "
        f"模板 `{source['index_template']['path']}/{source['index_template']['entrypoint']}` · 包入口 `{source['package_entrypoint']}`。",
        f"- 官方压缩包 SHA-256：`{source['archive_sha256']}`；[原版与依赖信息](source.json)。"
        f" 原版 {data['typst']['pages']} 页 → WPS {data['wps']['pages']} 页；导出 `{ex['method']}`。",
        f"- 原版/WPS 归一化字符 {data['typst']['normalized_chars']:,} / {data['wps']['normalized_chars']:,}；"
        f"长块可定位比例 {pct(data['alignment']['typst'], True)} / {pct(data['alignment']['wps'], True)}（双向，不是内容保真或视觉评分）。",
        f"- PDF 链接注释 {data['typst']['links']} → {data['wps']['links']}；PDF raster 对象 "
        f"{data['typst']['raster_objects']} → {data['wps']['raster_objects']}，PDF vector 对象 "
        f"{data['typst']['vector_objects']} → {data['wps']['vector_objects']}（各渲染器会拆并/栅格化，不能单独判缺失）。",
        f"- 输出 Word 原生表/OMML/绘图元素 {data['elements']['tbl']}/{data['elements']['oMath']}/{data['elements']['drawing']}；"
        f"根 XML/关系问题 {data['opc']['xml_parse_errors']}/{data['opc']['relationship_issues']}，"
        f"书签/字段结构告警 {data['opc']['field_or_bookmark_issues']}、悬空目标 {data['opc']['unresolved_navigation_targets']}。",
        f"- 纸张（pt）`{data['typst']['size_pt']}` → `{data['wps']['size_pt']}`；真正无墨迹页："
        f"原版 `{data['typst']['blank_pages']}` / WPS `{data['wps']['blank_pages']}`。无文字页不等于空白。",
        "",
        "[Typst PDF](typst.pdf) · [Word](tydocx.docx) · [WPS PDF](wps.pdf) · [Typst 文本](typst.txt) · "
        "[WPS 文本](wps.txt) · [DOCX 文本](docx.txt) · [降级](degradations.json) · [OOXML](docx.json) · "
        "[对齐](alignment.json) · [源逐页](typst-pages.json) · [WPS 逐页](wps-pages.json) · "
        "[源字体](typst-fonts.txt) · [WPS 字体](wps-fonts.txt)。",
        "",
        "联系表： " + " · ".join(f"[{p.stem}]({p.name})" for p in sorted(directory.glob("contact-*.jpg"))) + "；"
        "`pages/` 为双方全页 108 DPI，`crops/` 为选定差异 144 DPI。",
        "", "## 差异与分流", "", details or "> 待人工判读；自动对齐只能定位候选。",
        "", "## 锚点复核修正", "",
        *([row("原版物理页", "WPS 物理页", "跨页复核"), row("---:", "---:", "---")]
          + [row(x["typst"], x["wps"], x["meaning"]) for x in manual] if manual else ["无人工改写自动表。"]),
        "", "## 原版 → WPS 自动内容对齐", "",
        "括号是对齐投票数（归一化字符），不是页数或质量。`—` 包含源留白页、图形页、页码、低锚点页和真实遗漏；"
        "见逐页墨迹、对象、链接与 PDF，不能单凭空项判定遗漏。",
        "", row("原版物理页", "WPS 内容候选（票）", "原版字数 / 墨迹像素", "原版正文锚点"),
        row("---:", "---", "---:", "---"),
    ]
    for p, link in zip(t, mapping["source_map"]):
        related = ", ".join(f"{r['page']} ({r['votes']})" for r in link["candidates"]) or "—"
        doc.append(row(p["page"], related, f"{p['normalized_chars']} / {p['image']['ink_px']}", anchors(p)))
    doc.extend(("", "## WPS → 原版内容对齐", "",
                row("WPS 物理页", "原版内容候选（票）", "WPS 字数 / 墨迹像素", "WPS 正文锚点"),
                row("---:", "---", "---:", "---")))
    for p, link in zip(w, mapping["target_map"]):
        related = ", ".join(f"{r['page']} ({r['votes']})" for r in link["candidates"]) or "—"
        doc.append(row(p["page"], related, f"{p['normalized_chars']} / {p['image']['ink_px']}", anchors(p)))
    (directory / "report.md").write_text("\n".join(doc) + "\n", encoding="utf-8")


def source_link(source):
    repository = source.get("repository")
    if repository:
        return f"[{repository}]({repository})"
    url = f"https://typst.app/universe/package/{source.get('package_name', source['id'])}/{source['version']}/"
    return f"[Typst Universe]({url})（官方索引未给 `repository`）"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("selection", type=Path)
    args = parser.parse_args()
    ids = read(args.selection)
    for identifier, meta in ids.items():
        details_file = OUT / identifier / "findings.md"
        report(identifier, {identifier: meta}, details_file.read_text(encoding="utf-8").strip()
               if details_file.exists() else "")


if __name__ == "__main__":
    main()
