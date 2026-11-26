"""Print ranked triage leads and materialize hand-selected source/WPS PDF crops."""

from argparse import ArgumentParser
import json
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont
import numpy as np
import pymupdf as fitz

from analyze import norm
from produce import OUT, json_write


def read(path):
    return json.loads(path.read_bytes())


def leads(directory):
    al = read(directory / "alignment.json")
    audit = read(directory / "docx.json")
    sumry = read(directory / "summary.json")
    print(directory.name, sumry)
    for side in ("typst", "wps"):
        pages = read(directory / f"{side}-pages.json")["pages"]
        for p in pages:
            vectors = [o for o in p["visual_objects"] if o["type"].startswith("vector")]
            rasters = [o for o in p["visual_objects"] if o["type"] == "raster object"]
            marks = [b["text"].replace("\n", " ")[:115] for b in p["blocks"] if b["normalized_length"] >= 3]
            num = p["page"]
            mapping = al["source_map" if side == "typst" else "target_map"][num-1]
            margin = [f'{m["text"].strip()}@{m["bbox"][:2]}' for m in p["margin_band_spans"]
                      if m["text"].strip()]
            print(f"{side} p{num:2d} → {[x['page'] for x in mapping['candidates'][:8]]} "
                  f"{p['normalized_chars']:>5} chars {p['image']['ink_px']:>7} ink "
                  f"{len(rasters)} ras/{len(vectors)} vec/{len(p['links'])} links  "
                  f"{' | '.join(marks[:3])}  margin: {', '.join(margin)[:180]}")
    for name, arrange in (
        ("SOURCE POORLY ALIGNED", lambda b: b["normalized_chars"] * (1 - b["covered_fraction"])),
        ("SOURCE ABSENT FROM DOCX CANDIDATES", lambda b: b["normalized_chars"] * (1 - b.get("docx_shingle_fraction", 1))),
    ):
        candidates = [b for b in al["typst_to_wps"]["blocks"] if b["normalized_chars"] > 15 and b["covered_fraction"] < .85]
        print(name)
        for b in sorted(candidates, key=arrange, reverse=True)[:22]:
            if not arrange(b):
                continue
            print(f'[{b["id"]}] {b["range_pt"]} WPS={b["covered_fraction"]} XML={b.get("docx_shingle_fraction")} '
                  f'{b["text"].replace(chr(10), " ")[:225]} targets {list(b["target_page_votes_chars"])}')
    print("FIRST STRUCTURE LEADS")
    for issue in audit["unresolved_navigation_targets"][:10]:
        print(*issue)
    print("PAGE FURNITURE", [(x["part"], x["paragraphs"][:4], x["fields"]) for x in audit["furniture"] if "notes.xml" not in x["part"]])
    print("SECTION LEADS", audit["sections"][:2])
    print("DRAWS", audit["drawings"][:4])


def template_crops(directory, specs):
    sides = {side: fitz.open(directory / f"{side}.pdf") for side in ("typst", "wps")}
    out = directory / "crops"
    out.mkdir(exist_ok=True)
    face = ImageFont.truetype("arial.ttf", 18)
    report = []
    for incident in specs:
        images, panels = [], []
        for side in ("typst", "wps"):
            for section in incident.get(side, []):
                pid, *coords = section
                pdf_page = sides[side][pid-1]
                rect = fitz.Rect(*coords) & pdf_page.rect if coords else pdf_page.rect
                pix = pdf_page.get_pixmap(dpi=144, clip=rect, annots=False)
                im = Image.frombytes("RGB", [pix.width, pix.height], pix.samples)
                a = np.asarray(im)
                darkness = a.min(axis=2) < 239
                chroma = (a.max(axis=2) - a.min(axis=2)) > 23
                nonwhite = np.unique(a[darkness], axis=0) if darkness.any() else np.zeros((0, 3))
                pane = {"side": side, "page": pid, "bounds_pt": list(rect), "selected_pixels": int(darkness.sum()),
                        "color_pixels": int(chroma.sum()), "unique_nonwhite_colors": len(nonwhite)}
                images.append((im, f"{side} p{pid}  {list(map(round, list(rect)))}  ink {pane['selected_pixels']} px"))
                panels.append(pane)
        if images:
            title = incident["id"]
            height = max(image.height for image, _ in images) + 47
            sheet = Image.new("RGB", (sum(max(i.width, 300) + 12 for i, _ in images) + 12, height), "#e8eef2")
            draw = ImageDraw.Draw(sheet)
            x = 12
            for image, caption in images:
                draw.text((x, 10), caption, fill="#14263d", font=face)
                sheet.paste(image, (x, 35))
                x += max(image.width, 300) + 12
            sheet.save(out / f"{title}.png")
        report.append(dict(incident, panels=panels))
    json_write(out / "measurements.json", report)


def main():
    parser = ArgumentParser()
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--leads", nargs="+")
    group.add_argument("--crops", type=Path)
    args = parser.parse_args()
    if args.leads:
        for identifier in args.leads:
            leads(OUT / identifier)
    else:
        for identifier, specs in read(args.crops).items():
            template_crops(OUT / identifier, specs)


if __name__ == "__main__":
    main()
