"""Check the DOCX and WPS PDF built by the modern-nju-thesis Cargo test."""

from collections import Counter
from math import ceil, floor
from pathlib import Path
import argparse
import re
import subprocess
import xml.etree.ElementTree as ET
from zipfile import ZipFile


W = "{http://schemas.openxmlformats.org/wordprocessingml/2006/main}"
R = "{http://schemas.openxmlformats.org/officeDocument/2006/relationships}"
PAGES = ["1"] * 7 + ["2"] * 2 + ["3"] * 3 + ["5", "7"] + ["9"] * 3


def read_cache(path):
    with ZipFile(path) as package:
        root = ET.fromstring(package.read("word/document.xml"))
        rels = ET.fromstring(package.read("word/_rels/document.xml.rels"))
    external = {r.get("Id") for r in rels if r.get("TargetMode") == "External"}
    starts = list(root.iter(W + "bookmarkStart"))
    names = Counter(b.get(W + "name") for b in starts)
    start_ids = Counter(b.get(W + "id") for b in starts)
    end_ids = Counter(b.get(W + "id") for b in root.iter(W + "bookmarkEnd"))
    body = root.find(W + "body")
    assert body is not None
    paragraphs = list(body)
    toc_start, = [i for i, p in enumerate(paragraphs)
                  if any((n.text or "").strip().startswith("TOC ")
                         for n in p.findall(f"./{W}r/{W}instrText"))]
    rows = []
    targets = set()
    for i, page in enumerate(PAGES):
        p = paragraphs[toc_start + i]
        assert p.tag == W + "p" and p[0].tag == W + "pPr", i
        tab, = p[0].findall(f"./{W}tabs/{W}tab")
        assert tab.get(W + "val") == "right" and tab.get(W + "leader") == "dot", i
        assert int(tab.get(W + "pos")) in range(8346 - 40, 8346 + 41), i
        link, = p.findall(W + "hyperlink")
        assert p[-1] == link and list(link.iter(W + "hyperlink")) == [link], i
        assert not p.findall(f".//{W}fldSimple"), i
        assert not p.findall(f"./{W}r/{W}t") and not p.findall(f"./{W}r/{W}tab"), i
        assert all(node.get(R + "id") not in external for node in p.iter()), i
        assert set(link.attrib) == {W + "anchor", W + "history"}, link.attrib
        assert link.get(W + "history") == "1", i
        anchor = link.get(W + "anchor")
        assert anchor not in targets and names[anchor] == 1, anchor
        targets.add(anchor)
        target, = [b for b in starts if b.get(W + "name") == anchor]
        target_id = target.get(W + "id")
        assert start_ids[target_id] == end_ids[target_id] == 1, anchor

        outer = [n.get(W + "fldCharType") for n in p.findall(f"./{W}r/{W}fldChar")]
        assert outer == (["begin", "separate"] if i == 0 else []), outer
        if i == 0:
            toc, = p.findall(f"./{W}r/{W}instrText")
            assert "\\h" in toc.text.split(), toc.text
        content = [child for run in link.findall(W + "r") for child in run
                   if child.tag != W + "rPr"]
        tab_index = next(j for j, n in enumerate(content) if n.tag == W + "tab")
        title = "".join(n.text or "" for n in link.iter(W + "t")
                        if n in content[:tab_index])
        assert title.strip(), anchor
        tags = [n.tag.removeprefix(W) for n in content[tab_index:]]
        assert tags == ["tab", "fldChar", "instrText", "fldChar", "t", "fldChar"], tags
        assert [n.get(W + "fldCharType") for n in content[tab_index + 1:]
                if n.tag == W + "fldChar"] == ["begin", "separate", "end"], anchor
        assert content[tab_index + 2].text.split() == ["PAGEREF", anchor], anchor
        assert content[tab_index + 4].text == page, anchor
        rows.append(title)

    closing = paragraphs[toc_start + len(PAGES)]
    end, = closing.findall(f"./{W}r/{W}fldChar")
    assert end.get(W + "fldCharType") == "end" and not closing.findall(W + "hyperlink")
    return rows


def positioned_text(page, pdf):
    spans = [s for b in page.get_text("rawdict", flags=pdf.TEXTFLAGS_TEXT)["blocks"]
             for line in b.get("lines", []) for s in line["spans"]
             if s["color"] != 0xffffff and s.get("alpha", 255) > 0]
    rows = []
    for span in sorted(spans, key=lambda s: s["origin"][1]):
        y = span["origin"][1]
        if not rows or y - rows[-1][0] > 1.6:
            rows.append((y, []))
        rows[-1][1].extend(span["chars"])
    for y, chars in rows:
        chars.sort(key=lambda c: c["origin"][0])
        yield y, chars, "".join(c["c"] for c in chars)


def compact(text):
    return re.sub(r"\s+", "", text)


def check_footer_ink(path, page, bbox):
    # A footer in the text layer alone is not enough: make sure the digits
    # actually paint ink in this small slice of the exported page.
    left, top = map(lambda pt: floor(pt * 2), bbox[:2])
    right, bottom = map(lambda pt: ceil(pt * 2), bbox[2:])
    pgm = subprocess.run(
        ["pdftoppm", "-r", "144", "-f", str(page), "-l", str(page), "-singlefile",
         "-gray", "-x", str(left), "-y", str(top), "-W", str(right - left),
         "-H", str(bottom - top), str(path)],
        check=True, stdout=subprocess.PIPE,
    ).stdout
    magic, size, maximum, pixels = pgm.split(b"\n", 3)
    width, height = map(int, size.split())
    assert (magic, maximum) == (b"P5", b"255") and len(pixels) == width * height
    assert sum(value < 180 for value in pixels) >= 6, (page, bbox, "footer has no visible ink")


def check_pdf(path, rows):
    import pymupdf as pdf

    with pdf.open(path) as doc:
        candidates = []
        labels = []
        for page in doc:
            text = list(positioned_text(page, pdf))
            footers = []
            for y, chars, string in text:
                if y > page.rect.height * 0.93:
                    match = re.fullmatch(r"\s*(\d+|[IVXLCDM]+)\s*", string)
                    if match:
                        bbox = pdf.Rect(chars[match.start(1)]["bbox"])
                        for char in chars[match.start(1) + 1:match.end(1)]:
                            bbox |= pdf.Rect(char["bbox"])
                        footers.append((match[1], bbox))
            labels.append(footers)
            entries = []
            for y, chars, string in text:
                match = re.fullmatch(r"(.+?)([.·…]{3,})\s*(\d+)\s*", string)
                if match:
                    entries.append((y, chars, match))
            if len(entries) == len(rows):
                candidates.append((page, entries))
        (toc, entries), = candidates
        links = toc.get_links()
        assert len(links) == len(rows), (len(links), len(rows))
        assert all(link["kind"] == pdf.LINK_GOTO for link in links), links
        destinations = set()
        checked_footers = set()
        for title, (baseline, chars, match) in zip(rows, entries, strict=True):
            assert compact(match[1]) == compact(title), (title, match[0])
            # WPS may recalculate PAGEREF after it reflows the Word document.
            # The package check pins the original Typst page; the live check
            # compares the rendered result with the footer of the target page.
            number = match[3]
            assert 504 <= chars[match.end(3) - 1]["bbox"][2] <= 508, match[0]
            row_links = [link for link in links if link["from"].y0 < baseline < link["from"].y1]
            assert len(row_links) == 1, (title, row_links)
            link = row_links[0]
            target = link["page"]
            assert 0 <= target < len(doc), (title, link)
            assert [label for label, _ in labels[target]] == [number], (title, link, labels[target])
            if target not in checked_footers:
                check_footer_ink(path, target + 1, labels[target][0][1])
                checked_footers.add(target)
            destination = target, tuple(link["to"])
            assert destination not in destinations, (title, destination)
            destinations.add(destination)
            for part in (1, 2, 3):
                for char in chars[match.start(part):match.end(part)]:
                    bbox = char["bbox"]
                    point = pdf.Point((bbox[0] + bbox[2]) / 2, char["origin"][1])
                    assert point in link["from"], (title, part, char["c"], point, link)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("docx", type=Path)
    parser.add_argument("wps_pdf", type=Path)
    args = parser.parse_args()
    check_pdf(args.wps_pdf, read_cache(args.docx))


if __name__ == "__main__":
    main()
