"""Collect immutable Universe inputs and export the three survey artifacts."""

import argparse
import concurrent.futures
import hashlib
import io
import json
import os
from pathlib import Path
import re
import ssl
import subprocess
import sys
import tarfile
import tomllib
import urllib.request

HERE = Path(__file__).resolve().parent
TRIALS = HERE.parent
ROOT = TRIALS.parents[1]
OUT = ROOT / "output" / "thesis-visual-survey"
CACHE = TRIALS / "package-cache"
EXE = OUT / "tydocx-review.exe"
INDEX = TRIALS / "universe-index.json"
ORIGIN = "https://packages.typst.org/preview/"
EPOCH = "1795622400"
PRIMARY = {
    "modern-scut-thesis": "0.1.2", "pku-thesis-pass": "0.3.1",
    "minerva-thesis": "0.3.0", "cthesis": "0.1.0",
    "ethz-iis-dissertation": "2.0.0", "wut-thesis": "0.2.0",
    "thesist": "1.1.0", "modern-tud-thesis": "1.0.0",
    "ls3-uni-stuttgart-dissertation": "0.1.1", "modern-uit-thesis": "0.1.6",
    "tudelft-thesis-novanext": "0.1.0", "modern-xdu-thesis": "0.3.1",
}
EXTRA = ("kthesis", "unofficial-lnu-thesis", "modern-usp-thesis", "classic-ppgsi",
         "epsi-thesis", "clean-agh-thesis", "classy-tudelft-thesis",
         "simple-thesis-ger-host", "ukw-bipg-unofficial-thesis")
PACKAGE_RE = re.compile(r"@preview/([a-z0-9-]+):(\d+\.\d+\.\d+)")
PART_RE = re.compile(r"^[^<>:\"\\|?*\x00-\x1f]+$")
RESERVED = re.compile(r"(?i)^(CON|PRN|AUX|NUL|COM[1-9¹²³]|LPT[1-9¹²³])(?:\.|$)")


def json_write(path, data):
    scratch = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    scratch.write_text(json.dumps(data, indent=2, ensure_ascii=False, default=str) + "\n", encoding="utf-8")
    scratch.replace(path)


def sha(data):
    return hashlib.sha256(data).hexdigest()


class OnlyOrigin(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        if newurl != req.full_url:
            raise ValueError(f"unexpected archive redirect: {newurl}")
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def relparts(value):
    pieces = value.split("/")
    if not pieces or any(p in ("", ".", "..") or not PART_RE.fullmatch(p)
                         or p.endswith((" ", ".")) or RESERVED.match(p) for p in pieces):
        raise ValueError(f"unsafe path {value!r}")
    return pieces


def members(data):
    seen = {}
    chosen = []
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
        total = 0
        for m in tar:
            name = m.name.removeprefix("./")
            if m.isdir() and name in ("", "."):
                continue
            parts = relparts(name)
            if not (m.isdir() or m.type in (tarfile.REGTYPE, tarfile.AREGTYPE)):
                raise ValueError(f"not a regular archive path: {name}")
            for i in range(1, len(parts) + 1):
                prefix = "/".join(parts[:i])
                folded = prefix.casefold()
                previous = seen.get(folded)
                is_file = i == len(parts) and m.isfile()
                if previous and (previous[0] != prefix or previous[1] or is_file):
                    raise ValueError(f"duplicate/conflicting archive path: {name}")
                seen[folded] = (prefix, is_file)
            if m.isfile():
                total += m.size
                if total > 260_000_000 or m.size > 100_000_000 or len(chosen) >= 10000:
                    raise ValueError("uncompressed archive exceeds safety limit")
                chosen.append((name, tar.extractfile(m).read()))
    return chosen


def install(files, dest, expected_sha):
    seal = HERE / "seals" / f"{sha(str(dest.relative_to(ROOT)).encode())}.json"
    hashes = {name: sha(data) for name, data in files}
    if dest.exists():
        # Never overwrite another job's cache or original starter file.
        for name, digest in hashes.items():
            existing = dest.joinpath(*relparts(name))
            if not existing.is_file() or not existing.resolve().is_relative_to(dest.resolve()) or sha(existing.read_bytes()) != digest:
                raise ValueError(f"existing path is not the sealed official archive: {existing}")
    else:
        if not dest.parent.resolve().is_relative_to(ROOT):
            raise ValueError(f"destination parent escapes workspace: {dest.parent}")
        dest.mkdir(parents=True)
        for name, data in files:
            path = dest.joinpath(*relparts(name))
            path.parent.mkdir(parents=True, exist_ok=True)
            with path.open("xb") as fp:
                fp.write(data)
    seal.parent.mkdir(exist_ok=True)
    json_write(seal, {"archive_sha256": expected_sha, "files_sha256": hashes})


def package(entry, template=False, qualifier=None):
    name, version = entry["name"], entry["version"]
    if not re.fullmatch(r"[a-z0-9-]+", name) or not re.fullmatch(r"\d+\.\d+\.\d+", version):
        raise ValueError((name, version))
    url = f"{ORIGIN}{name}-{version}.tar.gz"
    archive = HERE / "archives" / f"{name}-{version}.tar.gz"
    archive.parent.mkdir(exist_ok=True)
    digest_file = archive.with_suffix(".gz.sha256")
    if archive.exists() and digest_file.exists():
        data = archive.read_bytes()
        if sha(data) != digest_file.read_text().strip():
            raise ValueError(f"saved archive SHA-256 mismatch: {archive}")
    else:
        opener = urllib.request.build_opener(OnlyOrigin, urllib.request.HTTPSHandler(context=ssl.create_default_context()))
        with opener.open(url, timeout=35) as response:
            if response.geturl() != url:
                raise ValueError(f"unexpected archive origin: {response.geturl()}")
            data = response.read(85_000_001)
            if len(data) > 85_000_000:
                raise ValueError("compressed archive exceeds safety limit")
        archive.write_bytes(data)
        digest_file.write_text(sha(data) + "\n", encoding="ascii")
    digest = sha(data)
    recorded = entry.get("sha256") or entry.get("checksum")
    if recorded and recorded.lower().removeprefix("sha256:") != digest:
        raise ValueError("official index archive digest mismatch")
    files = members(data)
    metadata = tomllib.loads(dict(files)["typst.toml"].decode("utf-8"))["package"]
    for field in ("name", "version", "entrypoint", "license"):
        if metadata[field] != entry[field]:
            raise ValueError(f"package/index {field} mismatch")
    identifier = f"{name}-v{version}" if qualifier else name
    dest = (HERE / "sources" / identifier) if template else (CACHE / "preview" / name / version)
    install(files, dest, digest)
    refs = set()
    for path, raw in files:
        if path.endswith(".typ"):
            refs.update(PACKAGE_RE.findall(raw.decode("utf-8", "replace")))
    item = {
        "id": identifier, "package_name": name, "version": version, "license": entry["license"],
        "minimum_typst": entry.get("compiler"), "repository": entry.get("repository"),
        "archive_url": url, "archive_sha256": digest,
        "sha256_authority": "official index" if recorded else "TLS archive, locally sealed; index gives no digest",
        "archive": archive.relative_to(ROOT).as_posix(), "package_entrypoint": entry["entrypoint"],
        "source_directory": dest.relative_to(ROOT).as_posix(),
        "static_references": [f"@preview/{name}:{version}" for name, version in sorted(refs)],
    }
    if template:
        starter = entry["template"]
        root = dest.joinpath(*relparts(starter["path"]))
        main = root.joinpath(*relparts(starter["entrypoint"]))
        if not main.is_file() or not main.resolve().is_relative_to(dest):
            raise ValueError(f"absent starter entry: {main}")
        selection = "historical reserve" if qualifier else "requested" if name in PRIMARY else "diversity/reserve"
        item.update(root=root.relative_to(ROOT).as_posix(), entrypoint=main.relative_to(ROOT).as_posix(),
                    index_template=starter, selection=selection)
        if qualifier:
            item["selection_argument"] = qualifier
        output = OUT / identifier
        output.mkdir(exist_ok=True)
        json_write(output / "source.json", item)
    return item, refs


def collect(file, selected):
    raw = INDEX.read_bytes()
    entries = {(e["name"], e["version"]): e for e in json.loads(raw)}
    picks = selected or [*PRIMARY, *EXTRA]
    candidates = {name: PRIMARY[name] for name in picks if name in PRIMARY}
    for spec in set(picks) - candidates.keys():
        name, sep, explicit = spec.partition(":")
        versions = (v for (n, v), e in entries.items()
                    if n == name and "template" in e and "thesis" in e["categories"]
                    and (not sep or explicit == v))
        version = max(versions, default=None, key=lambda v: tuple(map(int, v.split("."))))
        if version is not None:
            candidates[spec] = version
    manifest = {"index_url": ORIGIN + "index.json", "index_sha256": sha(raw),
                "index_file": INDEX.relative_to(ROOT).as_posix(), "typst": "0.15.1 (9dfd3a08)",
                "reference_creation_timestamp": int(EPOCH), "converter_sha256": sha(EXE.read_bytes()),
                "converter": str(EXE.relative_to(ROOT)), "cache": str(CACHE.relative_to(ROOT)),
                "entries": [], "dependencies": {}, "collection_errors": {}}

    def fetch(key, template, qualifier=None):
        try:
            item, refs = package(entries[key], template, qualifier)
            print("starter" if template else "dependency", *key, flush=True)
            return item, refs
        except Exception as exc:
            print("collect error", *key, repr(exc), flush=True)
            return f"{type(exc).__name__}: {exc}", set()

    with concurrent.futures.ThreadPoolExecutor(max_workers=8) as executor:
        keys = [((name.split(":")[0], candidates[name]), name if ":" in name else None)
                for name in picks if name in candidates]
        for name in set(picks) - candidates.keys():
            manifest["collection_errors"][name] = "no starter in official thesis category"
        combined = set()
        for (key, _), (item, refs) in zip(keys, executor.map(lambda k: fetch(k[0], True, k[1]), keys)):
            if isinstance(item, dict):
                manifest["entries"].append(item)
                combined |= {key} | refs
            else:
                manifest["collection_errors"][f"{key[0]}:{key[1]}"] = item
        done = set()
        while todo := sorted(combined - done):
            done.update(todo)
            for key, (item, refs) in zip(todo, executor.map(lambda k: fetch(k, False), todo)):
                manifest["dependencies"][f"@preview/{key[0]}:{key[1]}"] = item
                combined |= refs
    json_write(file, manifest)


def invoke(command, log, env=None, timeout=180):
    try:
        result = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                cwd=ROOT, env=env, timeout=timeout, check=False)
        log.write_bytes(result.stdout)
        return result.returncode
    except subprocess.TimeoutExpired as exc:
        log.write_bytes((exc.stdout or b"") + b"\nTIMEOUT\n")
        return "timeout"


def build(manifest, selected, stage, reuse=False, suffix="", docx_jobs=2):
    if stage == "docx":
        alias = HERE / "typst" / "packages"
        if not alias.exists() and not os.path.isjunction(alias):
            alias.parent.mkdir(exist_ok=True)
            subprocess.run(["cmd.exe", "/d", "/c", "mklink", "/j", str(alias), str(CACHE)], check=True)
        if not alias.samefile(CACHE):
            raise SystemExit(f"workspace-only cache alias is invalid: {alias}")
    env = dict(os.environ, TYPST_PACKAGE_CACHE_PATH=str(CACHE), TYPST_PACKAGE_PATH=str(CACHE),
               SOURCE_DATE_EPOCH=EPOCH, NO_COLOR="1")
    # All listed archives have already been obtained through OnlyOrigin above.
    # Keep any unresolved automatic HTTP download from bypassing that validation.
    env.update({key: "http://127.0.0.1:9" for key in
                ("HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy")})
    env.update(NO_PROXY="", no_proxy="")
    items = [i for i in manifest["entries"] if not selected or i["id"] in selected
             or i.get("selection_argument") in selected]
    jobs = 4 if stage == "typst" else docx_jobs if stage == "docx" else 1
    shell32 = Path(os.environ["WINDIR"]) / "SysWOW64/WindowsPowerShell/v1.0/powershell.exe"
    status_path = OUT / f"status-{stage}{suffix}.json"
    status = json.loads(status_path.read_bytes()) if status_path.exists() else {}

    def one(item):
        ident = item["id"]
        output = OUT / ident
        entry = ROOT / item["entrypoint"]
        log = output / f"{stage}.log"
        if stage == "typst":
            artifact = output / "typst.pdf"
            command = ["typst", "compile", "--jobs", "2", "--creation-timestamp", EPOCH,
                       "--package-path", str(CACHE), "--package-cache-path", str(CACHE),
                       "--root", str(ROOT / item["root"]), "--deps", str(output / "dependencies.json"),
                       str(entry), str(artifact)]
        elif stage == "docx":
            artifact = output / "tydocx.docx"
            if not (output / "typst.pdf").is_file():
                return {"exit": "no reference", "wrote": False}
            command = [sys.executable, "-B", str(HERE / "workspace_cache.py"), str(EXE), str(entry),
                       "-o", str(artifact), "--compat", "c2",
                       "--degradation-report", str(output / "degradations.json")]
        else:
            artifact = output / "wps.pdf"
            docx = output / "tydocx.docx"
            if not docx.is_file():
                return {"exit": "no docx", "wrote": False}
            command = [str(shell32), "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass",
                       "-File", str(TRIALS / "wps-export.ps1"), "-Docx", str(docx), "-Pdf", str(artifact)]
        if reuse and status.get(ident, {}).get("wrote") and artifact.is_file():
            return status[ident]
        artifact.unlink(missing_ok=True)
        code = invoke(command, log, None if stage == "wps" else env)
        record = {"exit": code, "wrote": code == 0 and artifact.is_file(), "command": command}
        if artifact.is_file():
            record |= {"sha256": sha(artifact.read_bytes()), "bytes": artifact.stat().st_size}
        if stage == "wps":
            record["method"] = log.read_text(errors="replace").strip() if record["wrote"] else None
        return record

    with concurrent.futures.ThreadPoolExecutor(max_workers=jobs) as executor:
        tasks = {executor.submit(one, item): item["id"] for item in items}
        for task in concurrent.futures.as_completed(tasks):
            ident = tasks[task]
            record = task.result()
            status[ident] = record
            json_write(status_path, status)
            print(stage, ident, record["exit"], record["wrote"], record.get("method", ""), flush=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("stages", nargs="+", choices=("collect", "typst", "docx", "wps"))
    parser.add_argument("--id", action="append")
    parser.add_argument("--reuse", action="store_true")
    parser.add_argument("--suffix", default="")
    parser.add_argument("--docx-jobs", type=int, default=2)
    args = parser.parse_args()
    if args.suffix and not re.fullmatch(r"-[a-z0-9-]+", args.suffix):
        parser.error("--suffix should look like -reserve")
    if subprocess.check_output(["typst", "--version"], text=True).strip() != "typst 0.15.1 (9dfd3a08)":
        raise SystemExit("Typst must be 0.15.1 (9dfd3a08)")
    file = OUT / f"manifest{args.suffix}.json"
    for stage in args.stages:
        if stage == "collect":
            collect(file, args.id)
        else:
            manifest = json.loads(file.read_bytes())
            if manifest["converter_sha256"] != sha(EXE.read_bytes()):
                raise SystemExit("fixed review executable changed")
            build(manifest, args.id, stage, args.reuse, args.suffix, args.docx_jobs)


if __name__ == "__main__":
    main()
