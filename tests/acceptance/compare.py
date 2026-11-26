"""Compare text, package structure, and first-page ink; save visual evidence."""

import collections as c
import difflib
import json
import re
import unicodedata as ud
import xml.etree.ElementTree as et
import zipfile
from pathlib import Path

import numpy as np
import pymupdf as fitz
from PIL import Image, ImageOps

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
OUTPUT = ROOT / "output" / "render-trials"
PART = re.compile(r"word/(document|header\d+|footer\d+|footnotes|endnotes)\.xml$")
TEXT_TAGS = {
    "{http://schemas.openxmlformats.org/wordprocessingml/2006/main}t",
    "{http://schemas.openxmlformats.org/officeDocument/2006/math}t",
    "{http://schemas.openxmlformats.org/drawingml/2006/main}t",
}
PARA = "{http://schemas.openxmlformats.org/wordprocessingml/2006/main}p"
IGNORE = dict.fromkeys(map(ord, "\u00ad\u200b\u200c\u200d\u2060\ufeff"), None)
TOKEN = re.compile(r"[\u3400-\u9fff\u3040-\u30ff\uac00-\ud7af]|[^\W_]+", re.UNICODE)


def tokens(text):
    text = ud.normalize("NFKC", text).casefold().translate(IGNORE)
    text = re.sub(r"(?<=\w)[‐-]\s*\n\s*(?=\w)", "", text)
    # Match CJK characters independently so wrapping does not split a token;
    # strip them from the word branch so mixed-script runs behave consistently.
    text = re.sub(r"([\u3400-\u9fff\u3040-\u30ff\uac00-\ud7af])", r" \1 ", text)
    return TOKEN.findall(text)


def delta(a, b):
    aa, bb = c.Counter(tokens(a)), c.Counter(tokens(b))
    shared = (aa & bb).total()
    both_terms = len(aa.keys() & bb.keys())
    return {
        "recall_by_count": round(shared / max(aa.total(), 1), 3),
        "precision_by_count": round(shared / max(bb.total(), 1), 3),
        "reference_distinct_token_recall": round(both_terms / max(len(aa), 1), 3),
        "reference_token_count": aa.total(),
        "candidate_token_count": bb.total(),
        "missing_distinct_tokens": sorted(aa.keys() - bb.keys()),
        "extra_distinct_tokens": sorted(bb.keys() - aa.keys()),
    }


def structure(path):
    result = {}
    with zipfile.ZipFile(path) as package:
        text = []
        result["media"] = sum(name.startswith("word/media/") for name in package.namelist())
        tags = c.Counter()
        for name in package.namelist():
            if not PART.fullmatch(name):
                continue
            root = et.fromstring(package.read(name))
            tags.update(node.tag.rsplit("}", 1)[-1] for node in root.iter())
            text.append(f"\n--- {name} ---\n")
            for paragraph in root.iter(PARA):
                def extract(node):
                    if node.tag in TEXT_TAGS:
                        yield node.text or ""
                    elif node.tag != PARA:
                        for child in node:
                            yield from extract(child)
                text.append("".join(s for node in paragraph for s in extract(node)) + "\n")
        for tag in ("tbl", "oMath", "anchor", "inline", "txbx", "sectPr", "headerReference",
                    "footerReference", "footnoteReference", "fldSimple", "fldChar", "ins", "del"):
            result[tag] = tags[tag]
    return "".join(text), result


def pdf_output(path, prefix):
    with fitz.open(path) as doc:
        pages = []
        fonts, sizes, colors = c.Counter(), c.Counter(), c.Counter()
        for number, page in enumerate(doc, 1):
            pixmap = page.get_pixmap(dpi=85, alpha=False)
            pixmap.save(path.with_name(f"{prefix}-{number:02d}.png"))
            text_data = page.get_text("dict", flags=fitz.TEXTFLAGS_DICT & ~fitz.TEXT_PRESERVE_IMAGES)
            for block in text_data["blocks"]:
                for line in block.get("lines", []):
                    for span in line["spans"]:
                        weight = len(span["text"].strip())
                        if weight:
                            fonts[span["font"]] += weight
                            sizes[str(round(span["size"], 1))] += weight
                            colors[f"#{span['color']:06x}"] += weight
            pages.append({"size_pt": [round(page.rect.width, 2), round(page.rect.height, 2)],
                          "extractable_chars": len(page.get_text()),
                          "drawings": len(page.get_drawings()),
                          "images": len(page.get_image_info())})
        style = {"font_char_counts": dict(fonts.most_common()),
                 "size_pt_char_counts": dict(sizes.most_common()),
                 "color_char_counts": dict(colors.most_common())}
        return "\n\f\n".join(page.get_text(sort=True) for page in doc), pages, style


def preview(folder, number):
    files = [folder / f"{stem}-{number:02d}.png" for stem in ("typst", "wps")]
    if not all(path.exists() for path in files):
        return None
    results = []
    panel = Image.new("RGB", (816, 590), "#ddd")
    for i, path in enumerate(files):
        with Image.open(path) as page:
            cover = ImageOps.contain(page.convert("RGB"), (400, 574))
            panel.paste(cover, (i * 408 + 4, 8))
            img = np.asarray(page.convert("RGB").resize((272, 352)), dtype=np.float32) / 255.0
            results.append(img)
    panel.save(folder / f"page-{number:02d}-side-by-side.png")
    a, b = results
    mask_a, mask_b = a.min(axis=-1) < 0.90, b.min(axis=-1) < 0.90
    union = (mask_a | mask_b).sum()
    ys, xs = np.indices(mask_a.shape)

    def extent(mask):
        if not mask.any():
            return None
        qx, qy = np.quantile(xs[mask] / mask.shape[1], (0.025, 0.975)), np.quantile(ys[mask] / mask.shape[0], (0.025, 0.975))
        return [round(float(x), 3) for x in (qx[0], qy[0], qx[1], qy[1])]

    return {
        "page": number,
        "rgb_rmse_0_to_1": round(float(np.sqrt(((a - b) ** 2).mean())), 4),
        "ink_iou": round(float((mask_a & mask_b).sum() / max(union, 1)), 4),
        "typst_ink_fraction": round(float(mask_a.mean()), 4),
        "wps_ink_fraction": round(float(mask_b.mean()), 4),
        "typst_ink_95pct_box": extent(mask_a),
        "wps_ink_95pct_box": extent(mask_b),
    }


def main():
    manifest = json.loads((HERE / "manifest.json").read_bytes())
    rows = []
    for entry in manifest["entries"]:
        name = entry["id"]
        folder = OUTPUT / name
        ref, wps, docx = (folder / name for name in ("typst.pdf", "wps.pdf", "tydocx.docx"))
        row = {"id": name, "typst_pdf": ref.is_file(), "docx": docx.is_file(), "wps_pdf": wps.is_file()}
        if ref.is_file():
            ref_text, row["typst_pages"], row["typst_pdf_style"] = pdf_output(ref, "typst")
            (folder / "typst.txt").write_text(ref_text, encoding="utf8")
        if docx.is_file():
            docx_text, row["docx_structure"] = structure(docx)
            (folder / "docx-xml.txt").write_text(docx_text, encoding="utf8")
            if ref.is_file():
                row["typst_to_xml_text"] = delta(ref_text, docx_text)
        if wps.is_file():
            wps_text, row["wps_pages"], row["wps_pdf_style"] = pdf_output(wps, "wps")
            (folder / "wps.txt").write_text(wps_text, encoding="utf8")
            if ref.is_file():
                row["typst_to_wps_text"] = delta(ref_text, wps_text)
                changes = "".join(difflib.unified_diff(ref_text.splitlines(keepends=True),
                                                       wps_text.splitlines(keepends=True),
                                                       "typst.txt", "wps.txt"))
                (folder / "text.diff").write_text(changes, encoding="utf8")
                common_pages = min(len(row["typst_pages"]), len(row["wps_pages"]))
                row["ordinal_page_visual"] = {
                    "note": "Both pages normalized to common raster. Same index may be different content after reflow; page-count drift is not a failure.",
                    "pairs": [preview(folder, number) for number in range(1, common_pages + 1)],
                }
        rows.append(row)
        print(name, ref.is_file(), docx.is_file(), wps.is_file(),
              row.get("typst_to_wps_text", {}).get("reference_distinct_token_recall"),
              [p["ink_iou"] for p in row.get("ordinal_page_visual", {}).get("pairs", [])][:6], flush=True)
    OUTPUT.mkdir(parents=True, exist_ok=True)
    (OUTPUT / "comparison.json").write_text(json.dumps(rows, indent=2, ensure_ascii=False) + "\n", encoding="utf8")


if __name__ == "__main__":
    main()
