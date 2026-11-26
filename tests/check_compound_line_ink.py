"""Measure painted parallel strokes in the Typst and WPS PDFs made by Cargo."""

from itertools import groupby
from math import ceil, floor
from pathlib import Path
from statistics import median
import argparse
import subprocess

import pymupdf


DPI = 288
SCALE = DPI / 72


def measure_bands(path, page, start, stop):
    top, bottom = floor(start * SCALE), ceil(stop * SCALE)
    ppm = subprocess.run(
        ["pdftoppm", "-r", str(DPI), "-f", str(page), "-l", str(page),
         "-singlefile", "-y", str(top), "-H", str(bottom - top), str(path)],
        check=True, stdout=subprocess.PIPE,
    ).stdout
    magic, size, maximum, pixels = ppm.split(b"\n", 3)
    width, height = map(int, size.split())
    assert (magic, maximum) == (b"P6", b"255") and len(pixels) == width * height * 3

    profile = []
    for y in range(height):
        row = pixels[y * width * 3:(y + 1) * width * 3]
        # Maroon is #85144B, so its green channel goes from 255 on paper
        # to 20 at full ink. Only long chromatic spans count, not text.
        ink = [255 - g for r, g, b in zip(row[::3], row[1::3], row[2::3], strict=True)
               if r - b > 8 and b - g > 8]
        profile.append(median(ink) / (255 - 20) if len(ink) >= 50 * SCALE else 0)

    bands = []
    for painted, rows in groupby(enumerate(profile), key=lambda row: row[1] > 0):
        if painted:
            samples = list(rows)
            thickness = sum(coverage for _, coverage in samples)
            center = sum((y + 0.5) * coverage for y, coverage in samples) / thickness
            bands.append((thickness / SCALE, center / SCALE))
    return bands


def check(path):
    with pymupdf.open(path) as doc:
        for label, next_label, gap in [
            ("Aligned horizontal", "Aligned vertical", 2.5),
            ("Another exact gap", "Overlapping strokes", 3.0),
        ]:
            # Locate each specimen by its labels, allowing body text to reflow
            # without accepting the wrong pair of strokes from the fixture.
            (page, start), = [(p.number + 1, r) for p in doc for r in p.search_for(label)]
            stop, = doc[page - 1].search_for(next_label)
            bands = measure_bands(path, page, start.y1, stop.y0)
            assert len(bands) == 2, (path, label, bands)
            # Coverage weighting keeps partial antialias rows from rounding
            # to full pixels. The center tolerance is below half the difference
            # between the requested gaps, so a fixed 2.5pt gap cannot pass both.
            for thickness, _ in bands:
                assert abs(thickness - 1.25) <= 0.26, (path, label, "thickness", bands)
            distance = bands[1][1] - bands[0][1]
            assert abs(distance - gap) <= 0.24, (path, label, "center distance", distance, bands)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("typst_pdf", type=Path)
    parser.add_argument("wps_pdf", type=Path)
    args = parser.parse_args()
    check(args.typst_pdf)
    check(args.wps_pdf)
