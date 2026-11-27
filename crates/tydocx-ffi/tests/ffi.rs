use std::ffi::{CStr, CString};
use std::io::Read as _;
use std::path::Path;

use tydocx_ffi::{
    TYDOCX_COMPILATION_ERROR, TYDOCX_INPUT_ERROR, TYDOCX_INVALID_ARGUMENT, TYDOCX_SUCCESS,
    tydocx_convert_file, tydocx_last_error_message,
};

fn c_path(path: &Path) -> CString {
    CString::new(path.to_str().expect("test paths must be Unicode")).expect("no interior NUL")
}

unsafe fn convert(input: &CStr, output: &CStr, compat: Option<&CStr>) -> i32 {
    let compat = compat.map_or(std::ptr::null(), CStr::as_ptr);
    // SAFETY: All pointers refer to live CStrings for the complete call.
    unsafe { tydocx_convert_file(input.as_ptr(), output.as_ptr(), compat) }
}

fn last_error() -> String {
    // SAFETY: A null buffer and zero length are the documented size query.
    let required = unsafe { tydocx_last_error_message(std::ptr::null_mut(), 0) };
    assert!(required > 0);
    let mut buffer = vec![0_u8; required];
    // SAFETY: The vector owns required writable bytes.
    let copied = unsafe { tydocx_last_error_message(buffer.as_mut_ptr().cast(), buffer.len()) };
    assert_eq!(copied, required);
    let length = buffer
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(buffer.len());
    String::from_utf8(buffer[..length].to_vec()).expect("native errors must be UTF-8")
}

#[test]
fn converts_minimal_typst_to_a_valid_docx() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let input = temp.path().join("minimal.typ");
    let output = temp.path().join("minimal.docx");
    std::fs::write(&input, "= Native test\n\nHello from the C ABI.\n").expect("write Typst");
    let input_c = c_path(&input);
    let output_c = c_path(&output);

    // A null compat pointer exercises the default C2 mode.
    let status = unsafe { convert(&input_c, &output_c, None) };
    assert_eq!(status, TYDOCX_SUCCESS, "{}", last_error());
    assert!(output.is_file());

    let file = std::fs::File::open(output).expect("open DOCX");
    let mut archive = zip::ZipArchive::new(file).expect("DOCX must be a valid ZIP");
    let mut document = archive
        .by_name("word/document.xml")
        .expect("DOCX main document part");
    let mut xml = String::new();
    document
        .read_to_string(&mut xml)
        .expect("read document XML");
    assert!(xml.contains("Hello from the C ABI."));
    assert!(xml.contains("Native test"));
    assert_eq!(last_error(), "");
}

#[test]
fn rejects_null_pointers_and_invalid_utf8() {
    let output = CString::new("out.docx").expect("output CString");
    // SAFETY: Null input is explicitly handled by the ABI.
    let status =
        unsafe { tydocx_convert_file(std::ptr::null(), output.as_ptr(), std::ptr::null()) };
    assert_eq!(status, TYDOCX_INVALID_ARGUMENT);
    assert!(last_error().contains("input_path must not be null"));

    let input = CString::new("in.typ").expect("input CString");
    // SAFETY: Null output is explicitly handled by the ABI.
    let status = unsafe { tydocx_convert_file(input.as_ptr(), std::ptr::null(), std::ptr::null()) };
    assert_eq!(status, TYDOCX_INVALID_ARGUMENT);
    assert!(last_error().contains("output_path must not be null"));

    let invalid_utf8 = [0xff_u8, 0];
    // SAFETY: The byte array is NUL-terminated and readable; invalid UTF-8 is
    // rejected without dereferencing it as a Rust string.
    let status = unsafe {
        tydocx_convert_file(
            invalid_utf8.as_ptr().cast(),
            output.as_ptr(),
            std::ptr::null(),
        )
    };
    assert_eq!(status, TYDOCX_INVALID_ARGUMENT);
    assert!(last_error().contains("not valid UTF-8"));
}

#[test]
fn reports_input_compilation_and_truncated_errors() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let missing = temp.path().join("missing.typ");
    let output = temp.path().join("out.docx");
    let missing_c = c_path(&missing);
    let output_c = c_path(&output);

    let status = unsafe { convert(&missing_c, &output_c, None) };
    assert_eq!(status, TYDOCX_INPUT_ERROR);
    let required = unsafe { tydocx_last_error_message(std::ptr::null_mut(), 0) };
    let mut short_buffer = [b'x'; 8];
    // SAFETY: short_buffer is valid for exactly its declared length.
    let copied =
        unsafe { tydocx_last_error_message(short_buffer.as_mut_ptr().cast(), short_buffer.len()) };
    assert_eq!(copied, required);
    assert!(required > short_buffer.len());
    assert_eq!(short_buffer[7], 0);

    let invalid = temp.path().join("invalid.typ");
    std::fs::write(&invalid, "#this_function_does_not_exist()\n").expect("write invalid Typst");
    let invalid_c = c_path(&invalid);
    let compat = CString::new("c2").expect("compat CString");
    let status = unsafe { convert(&invalid_c, &output_c, Some(&compat)) };
    assert_eq!(status, TYDOCX_COMPILATION_ERROR);
    assert!(last_error().contains("Typst compilation failed"));
}

#[test]
fn rejects_unknown_compatibility_mode() {
    let input = CString::new("in.typ").expect("input CString");
    let output = CString::new("out.docx").expect("output CString");
    let compat = CString::new("C2").expect("compat CString");
    let status = unsafe { convert(&input, &output, Some(&compat)) };
    assert_eq!(status, TYDOCX_INVALID_ARGUMENT);
    assert!(last_error().contains("expected c1, c2, or c3"));
}
