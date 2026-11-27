using System.Runtime.InteropServices;
using System.Text;

namespace Tydocx;

/// <summary>Converts Typst source files to editable Word documents.</summary>
public static class TydocxConverter
{
    /// <summary>
    /// Convert <paramref name="inputPath"/> to <paramref name="outputPath"/>.
    /// Relative paths are resolved against the process's current directory.
    /// An existing output file is replaced.
    /// </summary>
    /// <param name="inputPath">The Typst source path.</param>
    /// <param name="outputPath">The DOCX destination path.</param>
    /// <param name="compatMode">The compatibility mode; the default is C2.</param>
    /// <exception cref="ArgumentException">A path is empty or contains NUL.</exception>
    /// <exception cref="ArgumentOutOfRangeException">The compatibility mode is unknown.</exception>
    /// <exception cref="TydocxException">The native conversion failed.</exception>
    public static void Convert(
        string inputPath,
        string outputPath,
        CompatMode compatMode = CompatMode.C2)
    {
        ValidatePath(inputPath, nameof(inputPath));
        ValidatePath(outputPath, nameof(outputPath));
        string compat = compatMode switch
        {
            CompatMode.C1 => "c1",
            CompatMode.C2 => "c2",
            CompatMode.C3 => "c3",
            _ => throw new ArgumentOutOfRangeException(
                nameof(compatMode), compatMode, "Unknown tydocx compatibility mode."),
        };

        byte[] input = Utf8Z(inputPath);
        byte[] output = Utf8Z(outputPath);
        byte[] nativeCompat = Utf8Z(compat);
        int status;
        unsafe
        {
            fixed (byte* inputPointer = input)
            fixed (byte* outputPointer = output)
            fixed (byte* compatPointer = nativeCompat)
            {
                status = NativeMethods.ConvertFile(inputPointer, outputPointer, compatPointer);
            }
        }

        if (status != 0)
        {
            string fallback = $"Native tydocx conversion failed with status {status}.";
            string message = ReadLastError(fallback);
            throw new TydocxException((TydocxErrorCode)status, message);
        }
    }

    private static void ValidatePath(string path, string parameterName)
    {
        ArgumentNullException.ThrowIfNull(path, parameterName);
        if (path.Length == 0)
        {
            throw new ArgumentException("The path must not be empty.", parameterName);
        }

        if (path.Contains('\0'))
        {
            throw new ArgumentException("The path must not contain a NUL character.", parameterName);
        }
    }

    private static byte[] Utf8Z(string value) => Encoding.UTF8.GetBytes(value + "\0");

    private static string ReadLastError(string fallback)
    {
        nuint required;
        unsafe
        {
            required = NativeMethods.LastErrorMessage(null, 0);
        }

        if (required == 0 || required > int.MaxValue)
        {
            return fallback;
        }

        byte[] buffer = new byte[(int)required];
        unsafe
        {
            fixed (byte* bufferPointer = buffer)
            {
                if (NativeMethods.LastErrorMessage(bufferPointer, required) == 0)
                {
                    return fallback;
                }
            }
        }

        int length = Array.IndexOf(buffer, (byte)0);
        if (length < 0)
        {
            return fallback;
        }

        string message = Encoding.UTF8.GetString(buffer, 0, length);
        return string.IsNullOrEmpty(message) ? fallback : message;
    }

    private static class NativeMethods
    {
        private const string LibraryName = "tydocx_ffi";

        static NativeMethods()
        {
            NativeLibrary.SetDllImportResolver(typeof(NativeMethods).Assembly, ResolveLibrary);
        }

        [DllImport(LibraryName, EntryPoint = "tydocx_convert_file", CallingConvention = CallingConvention.Cdecl)]
        internal static extern unsafe int ConvertFile(byte* inputPath, byte* outputPath, byte* compat);

        [DllImport(LibraryName, EntryPoint = "tydocx_last_error_message", CallingConvention = CallingConvention.Cdecl)]
        internal static extern unsafe nuint LastErrorMessage(byte* buffer, nuint bufferLength);

        private static IntPtr ResolveLibrary(
            string libraryName,
            System.Reflection.Assembly assembly,
            DllImportSearchPath? searchPath)
        {
            if (libraryName != LibraryName)
            {
                return IntPtr.Zero;
            }

            string? explicitPath = Environment.GetEnvironmentVariable("TYDOCX_NATIVE_LIBRARY");
            if (!string.IsNullOrWhiteSpace(explicitPath))
            {
                if (NativeLibrary.TryLoad(explicitPath, out IntPtr handle))
                {
                    return handle;
                }

                throw new DllNotFoundException(
                    $"Could not load the tydocx native library from TYDOCX_NATIVE_LIBRARY: {explicitPath}");
            }

            string? nativeDirectory = Environment.GetEnvironmentVariable("TYDOCX_NATIVE_DIR");
            if (!string.IsNullOrWhiteSpace(nativeDirectory))
            {
                string candidate = Path.Combine(nativeDirectory, NativeFileName());
                if (NativeLibrary.TryLoad(candidate, out IntPtr handle))
                {
                    return handle;
                }

                throw new DllNotFoundException(
                    $"Could not load the tydocx native library: {candidate}");
            }

            // Returning zero continues with the runtime's normal probing. In
            // particular, it finds runtimes/<rid>/native assets from NuGet.
            return IntPtr.Zero;
        }

        private static string NativeFileName()
        {
            if (RuntimeInformation.IsOSPlatform(OSPlatform.Windows))
            {
                return "tydocx_ffi.dll";
            }

            if (RuntimeInformation.IsOSPlatform(OSPlatform.OSX))
            {
                return "libtydocx_ffi.dylib";
            }

            return "libtydocx_ffi.so";
        }
    }
}
