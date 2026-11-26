"""Run Typst and the cargo-built converter over the manifest entries."""

import argparse
import json
import os
import subprocess
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
OUTPUT = ROOT / "output" / "render-trials"
CACHE = HERE / "package-cache"


def run(command, log, timeout=130, env=None):
    try:
        result = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                cwd=ROOT, env=env, timeout=timeout, check=False)
        log.write_bytes(result.stdout)
        return result.returncode
    except subprocess.TimeoutExpired as exc:
        log.write_bytes((exc.stdout or b"") + b"\nTIMEOUT\n")
        return "timeout"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("stage", choices=("typst", "docx"))
    parser.add_argument("--converter", type=Path, default=OUTPUT / "baseline-tydocx.exe")
    parser.add_argument("--jobs", type=int)
    parser.add_argument("--entry", action="append")
    args = parser.parse_args()
    if args.stage == "docx" and not args.converter.is_file():
        parser.error(f"missing converter: {args.converter}")
    manifest = json.loads((HERE / "manifest.json").read_bytes())
    OUTPUT.mkdir(parents=True, exist_ok=True)
    CACHE.mkdir(exist_ok=True)
    cache_env = dict(os.environ, TYPST_PACKAGE_PATH=str(CACHE), TYPST_PACKAGE_CACHE_PATH=str(CACHE))
    status_path = OUTPUT / f"status-{args.stage}.json"
    status = json.loads(status_path.read_bytes()) if status_path.is_file() else {}

    def render(item):
        output = OUTPUT / item["id"]
        output.mkdir(parents=True, exist_ok=True)
        entry = HERE / item["entrypoint"]
        if args.stage == "typst":
            artifact = output / "typst.pdf"
            command = ["typst", "compile", "--jobs", "2", "--creation-timestamp", "1795622400",
                       "--root", str(HERE / item["root"]), "--deps", str(output / "dependencies.json"),
                       str(entry), str(artifact)]
        else:
            artifact = output / "tydocx.docx"
            command = [str(args.converter.resolve()), str(entry), "-o", str(artifact),
                       "--degradation-report", str(output / "degradations.json")]
        artifact.unlink(missing_ok=True)
        result = run(command, output / f"{args.stage}.log", env=cache_env)
        return {"exit": result, "wrote": artifact.is_file()}

    items = [item for item in manifest["entries"] if not args.entry or item["id"] in args.entry]
    jobs = args.jobs or (1 if args.stage == "docx" else 4)
    with ThreadPoolExecutor(max_workers=jobs) as pool:
        jobs = [(item["id"], pool.submit(render, item)) for item in items]
        for identifier, job in jobs:
            status[identifier] = job.result()
            print(args.stage, identifier, status[identifier], flush=True)
            status_path.write_text(json.dumps(status, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
