#![warn(clippy::pedantic)]

use std::fs;
use std::io::{self, Cursor};
use std::path::{Path, PathBuf};

use pyo3::exceptions::{
    PyException, PyFileNotFoundError, PyOSError, PyPermissionError, PyValueError,
};
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use pyo3::{create_exception, wrap_pyfunction};
use tydocx_core::{CompatMode, ConvertOptions, TydocxWorld};

create_exception!(tydocx, TydocxError, PyException);
create_exception!(tydocx, CompilationError, TydocxError);
create_exception!(tydocx, ConversionError, TydocxError);

/// Convert a Typst source file to a Word document on disk.
#[pyfunction]
#[pyo3(signature = (input_path, output_path, *, compat = "c2", bibliography_style = None))]
fn convert(
    py: Python<'_>,
    input_path: PathBuf,
    output_path: PathBuf,
    compat: &str,
    bibliography_style: Option<PathBuf>,
) -> PyResult<()> {
    let options = conversion_options(compat, bibliography_style)?;
    let output_for_error = output_path.clone();
    let bytes = py.allow_threads(move || convert_path(&input_path, &options))?;
    fs::write(output_path, bytes)
        .map_err(|error| io_error("failed to write output", &output_for_error, &error))?;
    Ok(())
}

/// Convert a Typst source file and return the Word document as bytes.
#[pyfunction]
#[pyo3(signature = (input_path, *, compat = "c2", bibliography_style = None))]
fn convert_to_bytes<'py>(
    py: Python<'py>,
    input_path: PathBuf,
    compat: &str,
    bibliography_style: Option<PathBuf>,
) -> PyResult<Bound<'py, PyBytes>> {
    let options = conversion_options(compat, bibliography_style)?;
    let bytes = py.allow_threads(move || convert_path(&input_path, &options))?;
    Ok(PyBytes::new(py, &bytes))
}

fn conversion_options(
    compat: &str,
    bibliography_style: Option<PathBuf>,
) -> PyResult<ConvertOptions> {
    let mode = CompatMode::from_short_name(compat).ok_or_else(|| {
        PyValueError::new_err(format!(
            "invalid compat mode {compat:?}: expected 'c1', 'c2', or 'c3'"
        ))
    })?;
    let mut options = ConvertOptions::new(mode);
    options.bibliography_style = bibliography_style.map(|path| path.to_string_lossy().into_owned());
    Ok(options)
}

fn convert_path(input_path: &Path, options: &ConvertOptions) -> PyResult<Vec<u8>> {
    let world = TydocxWorld::new(input_path)
        .map_err(|error| io_error("failed to read input", input_path, &error))?;
    let outcome = tydocx_core::convert(&world, options).map_err(|diagnostics| {
        let input_name = input_path.to_string_lossy();
        let messages = diagnostics
            .iter()
            .map(|diagnostic| {
                tydocx_core::diag::format_diagnostic(
                    diagnostic,
                    Some((&input_name, world.main_source())),
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        CompilationError::new_err(format!("Typst compilation failed:\n{messages}"))
    })?;

    let mut bytes = Cursor::new(Vec::new());
    tydocx_ooxml::write_docx(&outcome.document, &mut bytes).map_err(|error| {
        let path = input_path.display();
        ConversionError::new_err(format!("failed to generate DOCX for '{path}': {error}"))
    })?;
    Ok(bytes.into_inner())
}

fn io_error(action: &str, path: &Path, error: &io::Error) -> PyErr {
    let message = format!("{action} '{}': {error}", path.display());
    match error.kind() {
        io::ErrorKind::NotFound => PyFileNotFoundError::new_err(message),
        io::ErrorKind::PermissionDenied => PyPermissionError::new_err(message),
        _ => PyOSError::new_err(message),
    }
}

#[pymodule]
fn tydocx(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(convert, module)?)?;
    module.add_function(wrap_pyfunction!(convert_to_bytes, module)?)?;
    module.add("TydocxError", module.py().get_type::<TydocxError>())?;
    module.add(
        "CompilationError",
        module.py().get_type::<CompilationError>(),
    )?;
    module.add("ConversionError", module.py().get_type::<ConversionError>())?;
    module.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
