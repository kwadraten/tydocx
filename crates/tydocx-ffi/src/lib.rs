#![warn(clippy::pedantic)]
//! Small, panic-contained C ABI for converting a Typst file to a DOCX file.
//!
//! All paths and the optional compatibility mode are NUL-terminated UTF-8.
//! Errors are retained per thread and copied into a caller-owned buffer; the
//! ABI never returns an allocation that the caller would have to free.

use std::cell::RefCell;
use std::ffi::{CStr, c_char, c_int};
use std::fs::File;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use tydocx::{CompatMode, ConvertOptions, TydocxWorld};
use typst::World as _;
use typst::diag::SourceDiagnostic;
use typst::syntax::VirtualRoot;

/// Conversion completed successfully.
pub const TYDOCX_SUCCESS: c_int = 0;
/// A pointer, UTF-8 value, path, or compatibility mode was invalid.
pub const TYDOCX_INVALID_ARGUMENT: c_int = 1;
/// The input document could not be read.
pub const TYDOCX_INPUT_ERROR: c_int = 2;
/// Typst rejected the document.
pub const TYDOCX_COMPILATION_ERROR: c_int = 3;
/// The output document could not be written.
pub const TYDOCX_OUTPUT_ERROR: c_int = 4;
/// A panic was caught before it could cross the FFI boundary.
pub const TYDOCX_INTERNAL_ERROR: c_int = 5;

thread_local! {
    static LAST_ERROR: RefCell<String> = const { RefCell::new(String::new()) };
}

#[derive(Debug)]
struct FfiError {
    status: c_int,
    message: String,
}

impl FfiError {
    fn new(status: c_int, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    fn invalid(message: impl Into<String>) -> Self {
        Self::new(TYDOCX_INVALID_ARGUMENT, message)
    }
}

/// Convert `input_path` to `output_path`.
///
/// `compat` accepts `"c1"`, `"c2"`, or `"c3"`; a null pointer selects the
/// default (`c2`). On failure, call [`tydocx_last_error_message`] on the same
/// thread for a human-readable diagnostic.
///
/// # Safety
///
/// Every non-null argument must point to a readable, NUL-terminated string for
/// the duration of this call. The strings must remain unmodified while the
/// conversion is running.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tydocx_convert_file(
    input_path: *const c_char,
    output_path: *const c_char,
    compat: *const c_char,
) -> c_int {
    run_ffi(|| {
        // SAFETY: The exported function's contract requires valid C strings.
        // They are copied into owned Rust values before conversion starts.
        unsafe { convert_raw(input_path, output_path, compat) }
    })
}

/// Copy the calling thread's last error into a caller-owned UTF-8 buffer.
///
/// The return value is the required buffer size in bytes, including the
/// trailing NUL. Pass a null pointer or a zero length to query that size. If
/// the buffer is too small, the message is truncated at a byte boundary and
/// still NUL-terminated. An empty error requires one byte. A return value of
/// zero indicates that error retrieval itself failed unexpectedly.
///
/// # Safety
///
/// If `buffer_len` is nonzero, `buffer` must be valid for writes of exactly
/// `buffer_len` bytes and must not be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tydocx_last_error_message(
    buffer: *mut c_char,
    buffer_len: usize,
) -> usize {
    catch_unwind(AssertUnwindSafe(|| {
        LAST_ERROR.with(|last_error| {
            let last_error = last_error.borrow();
            let bytes = last_error.as_bytes();
            let required = bytes.len().saturating_add(1);
            if buffer.is_null() || buffer_len == 0 {
                return required;
            }

            let copied = bytes.len().min(buffer_len - 1);
            // SAFETY: The caller guarantees a writable buffer of buffer_len
            // bytes. copied + the trailing NUL always fit within that region.
            unsafe {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), buffer.cast::<u8>(), copied);
                buffer.add(copied).write(0);
            }
            required
        })
    }))
    .unwrap_or(0)
}

fn run_ffi(operation: impl FnOnce() -> Result<(), FfiError>) -> c_int {
    clear_last_error();
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(Ok(())) => TYDOCX_SUCCESS,
        Ok(Err(error)) => {
            let status = error.status;
            set_last_error(error.message);
            status
        }
        Err(payload) => {
            let detail = panic_detail(payload.as_ref());
            set_last_error(format!("tydocx internal error: caught a panic{detail}"));
            TYDOCX_INTERNAL_ERROR
        }
    }
}

fn panic_detail(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        format!(": {message}")
    } else if let Some(message) = payload.downcast_ref::<String>() {
        format!(": {message}")
    } else {
        String::new()
    }
}

fn clear_last_error() {
    LAST_ERROR.with(|last_error| last_error.borrow_mut().clear());
}

fn set_last_error(message: String) {
    LAST_ERROR.with(|last_error| *last_error.borrow_mut() = message);
}

unsafe fn convert_raw(
    input_path: *const c_char,
    output_path: *const c_char,
    compat: *const c_char,
) -> Result<(), FfiError> {
    // SAFETY: Forwarded from tydocx_convert_file's caller contract.
    let input = unsafe { required_path(input_path, "input_path") }?;
    // SAFETY: Forwarded from tydocx_convert_file's caller contract.
    let output = unsafe { required_path(output_path, "output_path") }?;
    // SAFETY: Forwarded from tydocx_convert_file's caller contract.
    let compat = unsafe { optional_compat(compat) }?;
    convert_paths(&input, &output, compat)
}

unsafe fn required_path(pointer: *const c_char, name: &str) -> Result<PathBuf, FfiError> {
    if pointer.is_null() {
        return Err(FfiError::invalid(format!("{name} must not be null")));
    }
    // SAFETY: The caller guarantees a readable NUL-terminated string.
    let value = unsafe { CStr::from_ptr(pointer) };
    let value = value
        .to_str()
        .map_err(|_| FfiError::invalid(format!("{name} is not valid UTF-8")))?;
    if value.is_empty() {
        return Err(FfiError::invalid(format!("{name} must not be empty")));
    }
    Ok(PathBuf::from(value))
}

unsafe fn optional_compat(pointer: *const c_char) -> Result<CompatMode, FfiError> {
    if pointer.is_null() {
        return Ok(CompatMode::default());
    }
    // SAFETY: The caller guarantees a readable NUL-terminated string.
    let value = unsafe { CStr::from_ptr(pointer) };
    let value = value
        .to_str()
        .map_err(|_| FfiError::invalid("compat is not valid UTF-8"))?;
    CompatMode::from_short_name(value).ok_or_else(|| {
        FfiError::invalid(format!(
            "invalid compat mode {value:?}: expected c1, c2, or c3"
        ))
    })
}

fn convert_paths(input: &Path, output: &Path, compat: CompatMode) -> Result<(), FfiError> {
    let world = TydocxWorld::new(input).map_err(|error| {
        FfiError::new(
            TYDOCX_INPUT_ERROR,
            format!("failed to read input {}: {error}", input.display()),
        )
    })?;
    let options = ConvertOptions::new(compat);
    let outcome = tydocx::convert(&world, &options).map_err(|diagnostics| {
        let details = diagnostics
            .iter()
            .map(|diagnostic| render_diagnostic(&world, diagnostic))
            .collect::<Vec<_>>()
            .join("\n");
        FfiError::new(
            TYDOCX_COMPILATION_ERROR,
            format!("Typst compilation failed:\n{details}"),
        )
    })?;

    let file = File::create(output).map_err(|error| output_error(output, error))?;
    tydocx::ooxml::write_docx(&outcome.document, file)
        .map_err(|error| output_error(output, error))?;
    Ok(())
}

fn output_error(output: &Path, error: impl std::fmt::Display) -> FfiError {
    FfiError::new(
        TYDOCX_OUTPUT_ERROR,
        format!("failed to write output {}: {error}", output.display()),
    )
}

fn render_diagnostic(world: &TydocxWorld, diagnostic: &SourceDiagnostic) -> String {
    let file = diagnostic.span.id().and_then(|id| {
        let source = world.source(id).ok()?;
        let name = match id.root() {
            VirtualRoot::Project => world
                .root()
                .join(id.vpath().get_without_slash())
                .display()
                .to_string(),
            VirtualRoot::Package(spec) => {
                format!("{spec}/{}", id.vpath().get_without_slash())
            }
        };
        Some((name, source))
    });
    let file = file.as_ref().map(|(name, source)| (name.as_str(), source));
    tydocx::diag::format_diagnostic(diagnostic, file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panic_is_converted_to_an_internal_error() {
        let status = run_ffi(|| -> Result<(), FfiError> { panic!("intentional FFI test panic") });
        assert_eq!(status, TYDOCX_INTERNAL_ERROR);
        LAST_ERROR.with(|message| {
            assert!(message.borrow().contains("intentional FFI test panic"));
        });
    }

    #[test]
    fn successful_operation_clears_an_earlier_error() {
        set_last_error("old error".to_owned());
        let status = run_ffi(|| Ok(()));
        assert_eq!(status, TYDOCX_SUCCESS);
        LAST_ERROR.with(|message| assert!(message.borrow().is_empty()));
    }
}
