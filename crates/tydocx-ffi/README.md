# tydocx-ffi

The native C ABI used by the .NET wrapper. It exports only two functions,
documented in [`include/tydocx.h`](include/tydocx.h):

- `tydocx_convert_file(input_path, output_path, compat)` converts one Typst
  file to DOCX. `compat` is `c1`, `c2`, `c3`, or `NULL` for the default `c2`.
- `tydocx_last_error_message(buffer, buffer_len)` reads the calling thread's
  last error into caller-owned storage.

Paths and errors are UTF-8. The converter catches Rust panics at the ABI
boundary. No function returns an allocation, so there is no corresponding
free function and no handle lifetime to manage.

```sh
cargo build -p tydocx-ffi
cargo test -p tydocx-ffi
```

The native artifact is named `tydocx_ffi.dll` on Windows, `libtydocx_ffi.so` on
Linux, and `libtydocx_ffi.dylib` on macOS.
