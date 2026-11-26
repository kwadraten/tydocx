"""Align every page and collect raster, PDF, typography, and OPC evidence."""

from argparse import ArgumentParser
from collections import Counter, defaultdict
from concurrent.futures import ThreadPoolExecutor
from difflib import SequenceMatcher
from pathlib import Path
import re
import subprocess
import unicodedata as ud

import numpy as np
import pymupdf as fitz
from PIL import Image, ImageDraw, ImageFont

from docx_audit import docx_audit
from produce import OUT, json_write

DPI = 108
GRAM = 11


def norm(string):
    return "".join(c for c in ud.normalize("NFKC", string).casefold()
                   if not c.isspace() and ud.category(c)[0] not in "PCZ")


def box(data):
    return [round(float(x), 2) for x in data]


def numrect(rect, scale, max_x, max_y, padding=0):
    x0, y0, x1, y1 = rect
    return (max(0, int(x0 * scale) - padding), max(0, int(y0 * scale) - padding),
            min(max_x, int(x1 * scale + .99) + padding), min(max_y, int(y1 * scale + .99) + padding))


def coverage_rect(ink, rect, scale):
    h, w = ink.shape
    x0, y0, x1, y1 = numrect(rect, scale, w, h)
    region = ink[y0:y1, x0:x1]
    return int(region.sum())


def raster(page, spans, objects, target):
    pix = page.get_pixmap(dpi=DPI, alpha=False, annots=False)
    pix.save(str(target))
    rgb = np.frombuffer(pix.samples, dtype=np.uint8).reshape(pix.height, pix.width, 3)
    dark = np.min(rgb, axis=2) < 239
    chroma = (np.max(rgb, axis=2) - np.min(rgb, axis=2)) >= 24
    scale = DPI / 72
    mask = np.zeros_like(dark)
    for span in spans:
        x0, y0, x1, y1 = numrect(span["bbox"], scale, pix.width, pix.height, padding=1)
        mask[y0:y1, x0:x1] = True
    deadtext = dark & ~mask
    y, x = np.where(dark)
    y2, x2 = np.where(deadtext)
    def extent(x, y):
        return None if len(x) == 0 else box((x.min()/scale, y.min()/scale, (x.max()+1)/scale, (y.max()+1)/scale))
    for obj in objects:
        obj["ink_px"] = coverage_rect(dark, obj["bbox"], scale)
        obj["ink_outside_text_bboxes_px"] = coverage_rect(deadtext, obj["bbox"], scale)
    return {"dpi": DPI, "raster_size": [pix.width, pix.height], "ink_px": int(dark.sum()),
            "ink_fraction": round(float(dark.mean()), 6), "chromatic_ink_px": int(chroma.sum()),
            "ink_bounds_pt": extent(x, y), "ink_outside_text_bboxes_px": int(deadtext.sum()),
            "ink_outside_text_bboxes_pt": extent(x2, y2),
            "vertical_bands": [{"total_px": int(chunk.sum()), "non_text_px": int(c.sum())}
                               for chunk, c in zip(np.array_split(dark, 10), np.array_split(deadtext, 10))]}


def extract(directory, side):
    pdf = fitz.open(directory / f"{side}.pdf")
    target = directory / "pages" / side
    target.mkdir(parents=True, exist_ok=True)
    pages, tokens = [], []
    extractor = subprocess.run(["pdftotext", "-layout", "-enc", "UTF-8", str(directory / f"{side}.pdf"),
                                str(directory / f"{side}.txt")], capture_output=True)
    (directory / f"{side}-pdftotext.log").write_bytes(extractor.stdout + extractor.stderr)
    fonts = subprocess.run(["pdffonts", str(directory / f"{side}.pdf")], capture_output=True)
    (directory / f"{side}-fonts.txt").write_bytes(fonts.stdout + fonts.stderr)
    for pg in pdf:
        pid = pg.number + 1
        blocks, spans, galleries = [], [], []
        for data in pg.get_text("dict", flags=fitz.TEXTFLAGS_DICT & ~fitz.TEXT_PRESERVE_IMAGES)["blocks"]:
            if data["type"] != 0:
                continue
            lines = []
            for line in data["lines"]:
                run_info = [
                    {"text": span["text"], "bbox": box(span["bbox"]), "origin": box(span["origin"]),
                     "size": round(span["size"], 2), "font": span["font"], "color": f"#{span['color']:06x}",
                     "flags": span["flags"]}
                    for span in line["spans"]
                ]
                spans.extend(run_info)
                lines.append({"text": "".join(s["text"] for s in run_info), "bbox": box(line["bbox"]),
                              "direction": box(line["dir"]), "spans": run_info})
            plain = "\n".join(line["text"] for line in lines)
            rect = box(data["bbox"])
            anchor = norm(plain)
            zone = "top" if rect[3] < pg.rect.height * .085 else "bottom" if rect[1] > pg.rect.height * .89 else "middle"
            item = {"id": f"{pid}.{len(blocks)+1}", "page": pid, "bbox": rect, "zone": zone,
                    "text": plain, "normalized_length": len(anchor), "lines": lines}
            blocks.append(item)
            tokens.append((item, anchor))
        for obj in pg.get_image_info(hashes=True):
            galleries.append({"type": "raster object", "bbox": box(obj["bbox"]),
                              "embedded_pixels": [obj["width"], obj["height"]], "xref_digest": obj["digest"].hex()})
        for draw in pg.get_drawings():
            galleries.append({"type": f"vector {draw['type']}", "bbox": box(draw["rect"]),
                              "stroke": draw["color"], "fill": draw["fill"], "width_pt": draw["width"],
                              "segments": len(draw["items"])})
        raster_file = target / f"{pid:03}.png"
        image = raster(pg, spans, galleries, raster_file)
        annotations = [{k: box(v) if isinstance(v, (fitz.Rect, fitz.Point)) else v
                        for k, v in link.items()} for link in pg.get_links()]
        sections = [{"text": span["text"], "bbox": span["bbox"]} for bl in blocks
                    for line in bl["lines"] for span in line["spans"] if bl["zone"] != "middle"]
        font_totals, size_totals, color_totals = Counter(), Counter(), Counter()
        for span in spans:
            weight = len(norm(span["text"]))
            font_totals[span["font"]] += weight
            size_totals[span["size"]] += weight
            color_totals[span["color"]] += weight
        pages.append({
            "page": pid, "size_pt": box((pg.rect.width, pg.rect.height)), "mark_count": len(blocks),
            "normalized_chars": sum(bl["normalized_length"] for bl in blocks),
            "image": image, "visual_objects": galleries, "links": annotations,
            "fonts_chars": font_totals, "size_chars": size_totals, "color_chars": color_totals,
            "margin_band_spans": sections, "blocks": blocks,
        })
    document = {"pages": pages, "metadata": pdf.metadata, "outlines": pdf.get_toc(simple=False)}
    if extractor.returncode != 0:
        (directory / f"{side}.txt").write_text("\n\f\n".join("\n\n".join(b["text"] for b in p["blocks"])
                                         for p in pages), encoding="utf-8")
    return document, tokens


def grams_index(tokens):
    index = defaultdict(list)
    for i, (_, normalized) in enumerate(tokens):
        for pos in range(len(normalized) - GRAM + 1):
            gram = normalized[pos:pos+GRAM]
            if len(index[gram]) < 64:
                index[gram].append((i, pos))
    return index


def styles(b):
    by_style = Counter()
    for l in b["lines"]:
        for s in l["spans"]:
            by_style[(s["font"], s["size"], s["color"])] += len(norm(s["text"]))
    return [{"font": f, "size": z, "color": c, "chars": chars}
            for (f, z, c), chars in by_style.most_common(5)]


def align(source, target, target_index, docx_words=None):
    quick = defaultdict(list)
    for i, (_, n) in enumerate(target):
        if 2 <= len(n) < GRAM:
            quick[n].append(i)
    aligned, page_histogram = [], defaultdict(Counter)
    totals = Counter()
    for b, n in source:
        weights = Counter()
        intervals = defaultdict(list)
        for pos in range(max(0, len(n) - GRAM + 1)):
            hits = target_index.get(n[pos:pos+GRAM], [])
            if not hits or len(hits) >= 64:
                continue
            for i, qpos in hits:
                weights[i] += 1 / len(hits)
                intervals[i].append([pos, qpos])
        covered = bytearray(len(n))
        picks = []
        for i, _ in weights.most_common(16):
            match_block, matched_norm = target[i]
            if len(n) > 1800 or len(matched_norm) > 3600:
                pairs = [[p, q, GRAM] for p, q in intervals[i]]
            else:
                pairs = [list(t) for t in SequenceMatcher(None, n, matched_norm, autojunk=False).get_matching_blocks()
                         if t.size >= max(5, GRAM // 2)]
            match_count = sum(length for p, q, length in pairs)
            new = sum(1 for p, q, length in pairs for j in range(p, p+length) if not covered[j])
            # Keep meaningful split or duplicate coverage without letting running
            # heads and tiny common words claim the whole paragraph.
            if new < min(10, len(n)) and (picks or match_count < min(10, len(n))):
                continue
            for p, q, length in pairs:
                covered[p:p+length] = b"\x01" * length
            picks.append((match_block, match_count, new, pairs[:6]))
        if 2 <= len(n) < GRAM and (short := quick.get(n)):
            # Ambiguous short items (especially page numbers) are not page votes.
            covered[:] = b"\x01" * len(n)
            for i in short[:8]:
                picks.append((target[i][0], len(n), len(n), [[0, 0, len(n)]]))
        bypage = Counter()
        for target_block, chars, new, pairs in picks:
            if len(n) >= GRAM:
                bypage[target_block["page"]] += new
                page_histogram[b["page"]][target_block["page"]] += new
        ratio = round(sum(covered) / len(n), 3) if n else None
        totals["chars"] += len(n)
        totals["covered"] += sum(covered)
        if len(n) >= GRAM:
            totals["long_chars"] += len(n)
            totals["long_covered"] += sum(covered)
        record = {"id": b["id"], "page": b["page"], "range_pt": b["bbox"], "zone": b["zone"],
                  "text": b["text"], "normalized_chars": len(n), "covered_fraction": ratio,
                  "styles": styles(b), "target_page_votes_chars": bypage,
                  "targets": [{"id": other["id"], "page": other["page"], "range_pt": other["bbox"],
                               "matched_chars": chars, "incremental_chars": new, "text": other["text"][:550],
                               "dx_left_pt": round(other["bbox"][0] - b["bbox"][0], 2),
                               "dy_top_pt": round(other["bbox"][1] - b["bbox"][1], 2),
                               "styles": styles(other), "sample_indices": pairs}
                              for other, chars, new, pairs in picks]}
        if docx_words is not None and len(n) >= GRAM and ratio is not None and ratio < .98:
            probes = sorted({n[p:p+GRAM] for p in range(0, len(n) - GRAM + 1, GRAM // 2)})
            record["docx_shingle_fraction"] = round(sum(x in docx_words for x in probes) / len(probes), 3)
        aligned.append(record)
    return {"blocks": aligned, "page_votes": page_histogram, "totals": totals}


def map_pages(pages, votes):
    mappings = []
    for p in pages:
        matches = votes.get(p["page"], {})
        strongest = sorted(matches.items(), key=lambda a: -a[1])
        top_n = max(matches.values(), default=0)
        threshold = max(11, top_n * .08)
        mappings.append({"page": p["page"], "candidates": [{"page": p, "votes": c} for p, c in strongest if c >= threshold],
                         "ink_px": p["image"]["ink_px"], "normalized_chars": p["normalized_chars"]})
    return mappings


def contact_sheets(folder, source_map, target_map):
    # Each side is complete and independently indexed. Labels give cross-page support.
    face = ImageFont.truetype("arial.ttf", 14)
    for side, rows in [("typst", source_map), ("wps", target_map)]:
        for start in range(0, len(rows), 24):
            group = rows[start:start+24]
            tile_x, tile_y, across = 207, 273, 6
            sheet = Image.new("RGB", (tile_x*across, tile_y*((len(group)+across-1)//across)), "#e9eaec")
            draw = ImageDraw.Draw(sheet)
            for j, row in enumerate(group):
                index = row["page"]
                thumb = Image.open(folder / "pages" / side / f"{index:03}.png")
                thumb.thumbnail((tile_x - 10, tile_y - 29))
                x, y = (j % across)*tile_x, (j//across)*tile_y
                sheet.paste(thumb, (x + (tile_x - thumb.width)//2, y + 2))
                related = ",".join(str(x["page"]) for x in row["candidates"][:7]) or "—"
                draw.text((x+4, y+tile_y-22), f"{side} {index} → {related}", fill="black", font=face)
            sheet.save(folder / f"contact-{side}-{start+1:03}.jpg", quality=90)


def per_template(folder):
    typ, t = extract(folder, "typst")
    wps, w = extract(folder, "wps")
    docx = docx_audit(folder / "tydocx.docx")
    corpus = "\n".join(p["text"] for p in docx["paragraphs"]) + "\n" + "\n".join(
        p for furniture in docx["furniture"] for p in furniture["paragraphs"])
    (folder / "docx.txt").write_text(corpus, encoding="utf-8")
    normalized_docx = norm(corpus)
    docx_words = {normalized_docx[i:i+GRAM] for i in range(len(normalized_docx)-GRAM+1)}
    fwd = align(t, w, grams_index(w), docx_words)
    rev = align(w, t, grams_index(t))
    source_map = map_pages(typ["pages"], fwd["page_votes"])
    target_map = map_pages(wps["pages"], rev["page_votes"])
    json_write(folder / "typst-pages.json", typ)
    json_write(folder / "wps-pages.json", wps)
    json_write(folder / "docx.json", docx)
    json_write(folder / "alignment.json", {
        "normalization": "per block NFKC, casefold; drop Unicode whitespace, punctuation, controls, separators",
        "method": f"{GRAM}-char candidate shingles; rare-anchor weighting; union of candidate substring matches; short exact blocks excluded from page votes",
        "limits": "separate note blocks, equations, glyph streams and non-text marks may not align. Coordinates are PDF pts from top left, not a common physical-page overlay.",
        "typst_to_wps": fwd, "wps_to_typst": rev, "source_map": source_map, "target_map": target_map,
    })
    contact_sheets(folder, source_map, target_map)
    summary = {}
    for label, pdf in [("typst", typ), ("wps", wps)]:
        pages = pdf["pages"]
        summary[label] = {"pages": len(pages), "normalized_chars": sum(p["normalized_chars"] for p in pages),
                          "links": sum(len(p["links"]) for p in pages),
                          "raster_objects": sum(obj["type"] == "raster object" for p in pages for obj in p["visual_objects"]),
                          "vector_objects": sum(obj["type"].startswith("vector") for p in pages for obj in p["visual_objects"]),
                          "blank_pages": [p["page"] for p in pages if p["image"]["ink_px"] <= 10],
                          "size_pt": sorted({tuple(p["size_pt"]) for p in pages})}
    summary["alignment"] = {side: x["totals"] for side, x in [("typst", fwd), ("wps", rev)]}
    summary["opc"] = {k: len(docx[k]) for k in ("zip_duplicate_names", "missing_essential",
                                               "xml_parse_errors", "untyped_parts", "relationship_issues",
                                               "field_or_bookmark_issues", "unresolved_navigation_targets")}
    summary["elements"] = {k: docx["document_element_counts"].get(k, 0) for k in
                           ("tbl", "oMath", "drawing", "hyperlink", "bookmarkStart", "sectPr", "pageBreakBefore")}
    json_write(folder / "summary.json", summary)
    print(folder.name, summary, flush=True)


def main():
    parser = ArgumentParser()
    parser.add_argument("ids", nargs="*")
    args = parser.parse_args()
    folders = [d for d in sorted(OUT.iterdir()) if all((d / n).is_file() for n in
               ("typst.pdf", "wps.pdf", "tydocx.docx")) and (not args.ids or d.name in args.ids)]
    with ThreadPoolExecutor(max_workers=3) as executor:
        list(executor.map(per_template, folders))


if __name__ == "__main__":
    main()
