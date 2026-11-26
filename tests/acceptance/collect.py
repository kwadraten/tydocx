"""Snapshot Universe starter projects and preserve their provenance."""

import hashlib
import io
import json
import re
import tarfile
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

HERE = Path(__file__).resolve().parent
INDEX = "https://packages.typst.org/preview/index.json"
GROUPS = {
    "conference paper": "charged-ieee faithful-acmart blind-cvpr",
    "mathematics": "clean-math-paper euler-math adaptable-pset",
    "CJK": "modern-nju-thesis canonical-nthu-thesis cv-nikudanngo bone-resume exam-zh-next",
    "resume": "basic-resume cobalt-cv rendercv",
    "report / running furniture": "biz-report modern-project-report ntnu-physics-report-replica tufte-memo yuuko-report",
    "slides": "touying dudi-colorful-slides barbet-slides",
    "poster": "simple-research-poster peace-of-posters isc-hei-poster",
    "book / running furniture": "orange-book wonderous-book araby-book latexy-book",
    "table": "classy-german-invoice neat-timetable chef-cookbook",
    "letter": "appreciated-letter",
}
VERSIONS = {"touying": "0.3.2"}
FLOATS = re.compile(r"\b(figure|place)\s*\([^)]{0,800}?\b(positioning\s*:\s*(?:top|bottom|auto)|float\s*:\s*true)", re.S)


def fetch(name, entry):
    version = entry["version"]
    url = f"https://packages.typst.org/preview/{name}-{version}.tar.gz"
    directory = HERE / "sources" / name
    archive = HERE / "archives" / f"{name}-{version}.tar.gz"
    archive.parent.mkdir(exist_ok=True)
    if not archive.exists():
        archive.write_bytes(urllib.request.urlopen(url, timeout=30).read())
    data = archive.read_bytes()
    directory.mkdir(parents=True, exist_ok=True)
    with tarfile.open(fileobj=io.BytesIO(data)) as tar:
        tar.extractall(directory, filter="data")
    template = entry["template"]
    root = directory / template["path"]
    main = root / template["entrypoint"]
    if not main.is_file():
        raise FileNotFoundError(main)
    local_files = sorted(p.relative_to(root).as_posix() for p in root.rglob("*") if p.is_file())
    pkg_refs = set()
    float_markers = []
    for path in directory.rglob("*.typ"):
        content = path.read_text(encoding="utf-8", errors="replace")
        pkg_refs.update(re.findall(r"@(?:preview|local)/[\w.-]+:[\d.]+", content))
        if FLOATS.search(content):
            float_markers.append(path.relative_to(directory).as_posix())
    return {
        "id": name,
        "version": version,
        "license": entry.get("license"),
        "minimum_typst": entry.get("compiler"),
        "universe_url": f"https://typst.app/universe/package/{name}/{version}/",
        "repository_url": entry.get("repository"),
        "archive_url": url,
        "archive_sha256": hashlib.sha256(data).hexdigest(),
        "archive": archive.relative_to(HERE).as_posix(),
        "root": root.relative_to(HERE).as_posix(),
        "entrypoint": main.relative_to(HERE).as_posix(),
        "package_entrypoint": entry["entrypoint"],
        "resources_in_template_root": [p for p in local_files if p != template["entrypoint"]],
        "referenced_packages_in_archive": sorted(pkg_refs),
        "possible_float_sources": sorted(float_markers),
    }


def version_key(entry):
    return tuple(map(int, entry["version"].split(".")))


def main():
    index = HERE / "universe-index.json"
    if not index.exists():
        index.write_bytes(urllib.request.urlopen(INDEX, timeout=30).read())
    entries = json.loads(index.read_bytes())
    with ThreadPoolExecutor(max_workers=10) as pool:
        jobs = []
        for group, names in GROUPS.items():
            for name in names.split():
                matches = [e for e in entries if e["name"] == name and "template" in e]
                if name in VERSIONS:
                    matches = [e for e in matches if e["version"] == VERSIONS[name]]
                entry = max(matches, key=version_key)
                jobs.append((group, name, pool.submit(fetch, name, entry)))
        manifest = {"index_url": INDEX, "index_sha256": hashlib.sha256(index.read_bytes()).hexdigest(), "entries": [], "collection_errors": []}
        for group, name, job in jobs:
            try:
                item = job.result()
                item["coverage"] = group
                manifest["entries"].append(item)
                print(name, item["version"], item["entrypoint"], item["possible_float_sources"], flush=True)
            except Exception as exc:
                manifest["collection_errors"].append({"id": name, "error": str(exc)})
                print(name, exc, flush=True)
        (HERE / "manifest.json").write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
