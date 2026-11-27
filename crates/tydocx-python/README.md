# tydocx for Python

`tydocx` converts Typst source files to editable Word (`.docx`) documents. The
extension calls the Rust `tydocx-core` conversion engine directly; it does not
invoke the CLI or require a separately installed Typst executable.

Python 3.9 and newer are supported. Release wheels use PyO3's stable `abi3`
ABI, so one wheel serves all supported Python versions on a given OS and
architecture.

## Usage

```python
from pathlib import Path

import tydocx

# Strings and os.PathLike objects (including pathlib.Path) are accepted.
tydocx.convert("paper.typ", Path("paper.docx"))

# Produce an in-memory DOCX package instead of writing an output file.
docx_bytes = tydocx.convert_to_bytes("paper.typ")
```

Both functions run the conversion without holding Python's GIL. The input is a
path rather than source text so relative Typst imports, includes, and assets
resolve from the source file's directory.

The optional keyword-only `compat` argument accepts `"c1"`, `"c2"` (the
default), or `"c3"`. `bibliography_style` optionally names a Word bibliography
XSL style and accepts the same path types as the other arguments.

## Errors

- Missing or unreadable inputs and output write failures raise the appropriate
  `OSError` subclass, such as `FileNotFoundError` or `PermissionError`.
- Invalid option values raise `ValueError`.
- Typst compilation failures raise `tydocx.CompilationError`, with the source
  location and compiler diagnostics in the message.
- DOCX encoding failures raise `tydocx.ConversionError`.

`CompilationError` and `ConversionError` both derive from
`tydocx.TydocxError`.

## Development and tests

Use a virtual environment, then install the build and test tools:

```console
python -m pip install maturin pytest
maturin develop
python -m pytest tests
```

To test exactly what will be published, build a wheel, install it into a clean
virtual environment, and run the tests from outside the source checkout:

```console
maturin build --release
python -m pip install --force-reinstall target/wheels/tydocx-*.whl
python -m pytest /path/to/crates/tydocx-python/tests
```

The tests generate their own minimal Typst inputs and inspect the resulting
DOCX as a ZIP package. They need neither network access nor external document
rendering tools.
