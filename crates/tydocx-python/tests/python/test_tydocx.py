from __future__ import annotations

import io
import zipfile

import pytest

import tydocx


def write_minimal_source(tmp_path):
    source = tmp_path / "minimal.typ"
    source.write_text("= Hello\n\nHello from Typst.\n", encoding="utf-8")
    return source


def assert_docx_package(package):
    with zipfile.ZipFile(package) as archive:
        assert archive.testzip() is None
        assert "word/document.xml" in archive.namelist()
        document_xml = archive.read("word/document.xml").decode("utf-8")
    assert "Hello from Typst." in document_xml


def test_convert_accepts_pathlike_and_writes_valid_docx(tmp_path):
    source = write_minimal_source(tmp_path)
    output = tmp_path / "minimal.docx"

    assert tydocx.convert(source, output) is None

    assert zipfile.is_zipfile(output)
    assert_docx_package(output)


def test_convert_to_bytes_returns_valid_docx_bytes(tmp_path):
    source = write_minimal_source(tmp_path)

    result = tydocx.convert_to_bytes(str(source))

    assert isinstance(result, bytes)
    assert_docx_package(io.BytesIO(result))


def test_missing_input_raises_file_not_found(tmp_path):
    missing = tmp_path / "does-not-exist.typ"

    with pytest.raises(FileNotFoundError, match="failed to read input"):
        tydocx.convert(missing, tmp_path / "unused.docx")


def test_invalid_typst_raises_compilation_error_without_output(tmp_path):
    source = tmp_path / "invalid.typ"
    source.write_text("#definitely_missing_function()\n", encoding="utf-8")
    output = tmp_path / "invalid.docx"

    with pytest.raises(tydocx.CompilationError, match="Typst compilation failed") as error:
        tydocx.convert(source, output)

    assert isinstance(error.value, tydocx.TydocxError)
    assert "unknown variable" in str(error.value)
    assert not output.exists()


def test_invalid_compat_raises_value_error(tmp_path):
    with pytest.raises(ValueError, match="expected 'c1', 'c2', or 'c3'"):
        tydocx.convert_to_bytes(tmp_path / "unused.typ", compat="minimal")
