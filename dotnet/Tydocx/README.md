# Tydocx for .NET

Convert Typst source files to editable Word documents from .NET 8 or later.
The package contains the managed API and RID-specific native libraries.

```csharp
using Tydocx;

TydocxConverter.Convert("paper.typ", "paper.docx");
TydocxConverter.Convert("paper.typ", "paper-c3.docx", CompatMode.C3);
```

`TydocxConverter.Convert` throws `TydocxException` for native input,
compilation, or output failures. Its `ErrorCode` property identifies the
failure category. Paths are ordinary .NET strings and cross the native ABI as
UTF-8.

## Packaging native libraries

Before `dotnet pack`, copy each release artifact into this project using the
standard NuGet native layout:

| RID | Native package path |
| --- | --- |
| `win-x64` | `runtimes/win-x64/native/tydocx_ffi.dll` |
| `linux-x64` | `runtimes/linux-x64/native/libtydocx_ffi.so` |
| `osx-x64` | `runtimes/osx-x64/native/libtydocx_ffi.dylib` |
| `osx-arm64` | `runtimes/osx-arm64/native/libtydocx_ffi.dylib` |

The project packs those files directly from its own `runtimes/` directory.
Other RIDs can be added using the same names. The .NET host resolves these
assets automatically; no custom package targets file is needed. Override the
package version with `dotnet pack -p:PackageVersion=<version>`.

For development and CI tests, set `TYDOCX_NATIVE_LIBRARY` to the full path of
the freshly built native library, or `TYDOCX_NATIVE_DIR` to its directory.
These overrides take precedence over normal NuGet probing.
