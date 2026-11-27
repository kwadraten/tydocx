from os import PathLike
from typing import Literal, Optional, Union

_Path = Union[str, PathLike[str]]
_Compat = Literal["c1", "c2", "c3"]

__version__: str

class TydocxError(Exception):
    """Base class for conversion and Typst compilation errors."""

class CompilationError(TydocxError):
    """The Typst source could not be compiled."""

class ConversionError(TydocxError):
    """The compiled document could not be encoded as DOCX."""

def convert(
    input_path: _Path,
    output_path: _Path,
    *,
    compat: _Compat = "c2",
    bibliography_style: Optional[_Path] = None,
) -> None:
    """Convert a Typst source file to a DOCX file."""

def convert_to_bytes(
    input_path: _Path,
    *,
    compat: _Compat = "c2",
    bibliography_style: Optional[_Path] = None,
) -> bytes:
    """Convert a Typst source file and return the DOCX package as bytes."""
