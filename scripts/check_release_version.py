#!/usr/bin/env python3
"""Fail unless a v-prefixed release tag matches the Rust workspace version."""

from __future__ import annotations

import argparse
import re
import sys
import tomllib
import xml.etree.ElementTree as ET
from pathlib import Path

VERSION_TAG = re.compile(r"v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tag", help="release tag, for example v0.1.0")
    args = parser.parse_args()

    match = VERSION_TAG.fullmatch(args.tag)
    if match is None:
        parser.error("release tags must have the form vMAJOR.MINOR.PATCH")

    root = Path(__file__).resolve().parent.parent
    with (root / "Cargo.toml").open("rb") as stream:
        workspace_version = tomllib.load(stream)["workspace"]["package"]["version"]
    with (root / "crates/tydocx-python/pyproject.toml").open("rb") as stream:
        python_version = tomllib.load(stream)["project"]["version"]
    dotnet_project = ET.parse(root / "dotnet/Tydocx/Tydocx.csproj")
    dotnet_version = dotnet_project.findtext("./PropertyGroup/Version")

    tag_version = args.tag[1:]
    versions = {
        "Cargo.toml workspace": workspace_version,
        "Python project": python_version,
        "NuGet project": dotnet_version,
    }
    mismatches = [
        f"{name} is {version}, not {tag_version}"
        for name, version in versions.items()
        if version != tag_version
    ]
    if mismatches:
        for mismatch in mismatches:
            print(mismatch, file=sys.stderr)
        return 1

    print(f"Release version {tag_version} matches Rust, Python, and NuGet metadata")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
