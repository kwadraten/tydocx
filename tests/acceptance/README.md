# Universe acceptance inputs

The official index snapshot, starter archives and extracted sources are fixed
inputs. The workspace package cache is locally rebuildable and ignored by Git.
Do not edit extracted templates to make a conversion pass. Package-free
integration reductions belong in `tests/fixtures/`.

- `collect.py`, `manifest.json`, `sources/`, `archives/`: the original
  Universe acceptance set. `render.py`, `compare.py`, `probe.py`, and
  `wps-export.ps1` put regenerated artifacts in `output/render-trials/`.
- `thesis-survey/`: separate immutable starters, archives, hashes and survey
  drivers. They write regenerated material to `output/thesis-visual-survey/`.
  They require Python 3.13+ on Windows, Typst 0.15.1, WPS Writer COM for WPS
  export, and PyMuPDF, lxml, NumPy, Pillow and Poppler for analysis.
- `package-cache/`: ignored workspace-scoped cache. The fixed Windows thesis review
  binary resolves AppData through `SHGetKnownFolderPath`; the survey's wrapper
  redirects only that child's requested known folders to the local cache
  alias. Do not create an AppData junction or populate a system-wide cache to
  run the survey. The fixed review EXE is retained in `output/`, not here.

Run the scripts with `python -B` to avoid writing Python bytecode into this
directory. Run all WPS acceptance jobs serially. The thesis output's
`report.md`, manifests, and status files describe the tool versions, starter
selection, and exact inputs for that run.
