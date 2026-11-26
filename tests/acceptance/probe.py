"""Compile each package-free reduction with the reference and DOCX targets."""

from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import sys

sys.dont_write_bytecode = True
from render import OUTPUT, ROOT, run


def probe(source):
    source = source.resolve()
    folder = OUTPUT / "reductions" / source.stem
    folder.mkdir(parents=True, exist_ok=True)
    exe = OUTPUT / "baseline-tydocx.exe"
    pdf, docx, json = (folder / f"probe.{ext}" for ext in ("pdf", "docx", "json"))
    for path in (pdf, docx, json):
        path.unlink(missing_ok=True)
    ref = run(["typst", "compile", "--diagnostic-format", "short", str(source), str(pdf)], folder / "typst.log")
    conv = run([str(exe), str(source), "-o", str(docx), "--degradation-report", str(json)], folder / "docx.log")
    return source.name, ref, conv, (folder / "docx.log").read_text(encoding="utf8", errors="replace").splitlines()[:4]


if __name__ == "__main__":
    root = ROOT / "tests" / "fixtures"
    sources = sorted(root.glob("render_*.typ")) if not sys.argv[1:] else list(map(Path, sys.argv[1:]))
    with ThreadPoolExecutor(max_workers=1) as pool:
        for result in pool.map(probe, sources):
            print(*result, flush=True)
