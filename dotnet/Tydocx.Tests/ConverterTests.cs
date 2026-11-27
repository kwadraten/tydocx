using System.IO.Compression;
using System.Runtime.InteropServices;
using Xunit;

namespace Tydocx.Tests;

public sealed class ConverterTests
{
    static ConverterTests()
    {
        NativeTestLibrary.Configure();
    }

    [Fact]
    public void ConvertMinimalTypstCreatesValidDocx()
    {
        using var directory = new TestDirectory("文档");
        string input = directory.PathFor("source.typ");
        string output = directory.PathFor("result.docx");
        File.WriteAllText(input, "= .NET integration\n\nHello from the managed converter.\n");

        TydocxConverter.Convert(input, output);

        Assert.True(File.Exists(output));
        using ZipArchive archive = ZipFile.OpenRead(output);
        Assert.NotNull(archive.GetEntry("[Content_Types].xml"));
        ZipArchiveEntry document = archive.GetEntry("word/document.xml")!;
        Assert.NotNull(document);
        using StreamReader reader = new(document.Open());
        string xml = reader.ReadToEnd();
        Assert.Contains("Hello from the managed converter.", xml);
        Assert.Contains(".NET integration", xml);
    }

    [Fact]
    public void ConvertWithExplicitCompatibilityModeUsesNativeConverter()
    {
        using var directory = new TestDirectory();
        string input = directory.PathFor("source.typ");
        string output = directory.PathFor("result.docx");
        File.WriteAllText(input, "Compatibility C1.\n");

        TydocxConverter.Convert(input, output, CompatMode.C1);

        using ZipArchive archive = ZipFile.OpenRead(output);
        ZipArchiveEntry document = archive.GetEntry("word/document.xml")!;
        Assert.NotNull(document);
        using StreamReader reader = new(document.Open());
        Assert.Contains("Compatibility C1.", reader.ReadToEnd());
    }

    [Fact]
    public void InvalidTypstThrowsCompilationExceptionWithNativeDiagnostic()
    {
        using var directory = new TestDirectory();
        string input = directory.PathFor("invalid.typ");
        string output = directory.PathFor("invalid.docx");
        File.WriteAllText(input, "#this_function_does_not_exist()\n");

        TydocxException exception = Assert.Throws<TydocxException>(
            () => TydocxConverter.Convert(input, output));

        Assert.Equal(TydocxErrorCode.Compilation, exception.ErrorCode);
        Assert.Contains("Typst compilation failed", exception.Message);
        Assert.False(File.Exists(output));
    }

    [Fact]
    public void MissingInputThrowsInputException()
    {
        using var directory = new TestDirectory();
        string input = directory.PathFor("missing.typ");
        string output = directory.PathFor("missing.docx");

        TydocxException exception = Assert.Throws<TydocxException>(
            () => TydocxConverter.Convert(input, output));

        Assert.Equal(TydocxErrorCode.Input, exception.ErrorCode);
        Assert.Contains("failed to read input", exception.Message);
    }

    [Fact]
    public void InvalidManagedArgumentsAreRejectedClearly()
    {
        Assert.Throws<ArgumentNullException>(() => TydocxConverter.Convert(null!, "out.docx"));
        Assert.Throws<ArgumentNullException>(() => TydocxConverter.Convert("in.typ", null!));
        Assert.Throws<ArgumentException>(() => TydocxConverter.Convert("", "out.docx"));
        Assert.Throws<ArgumentException>(() => TydocxConverter.Convert("in.typ", "out\0.docx"));
        Assert.Throws<ArgumentOutOfRangeException>(
            () => TydocxConverter.Convert("in.typ", "out.docx", (CompatMode)99));
    }

    private sealed class TestDirectory : IDisposable
    {
        private readonly string path;

        public TestDirectory(string? prefix = null)
        {
            path = Path.Combine(
                Path.GetTempPath(),
                $"Tydocx.Tests-{prefix}-{Guid.NewGuid():N}");
            Directory.CreateDirectory(path);
        }

        public string PathFor(string fileName) => Path.Combine(path, fileName);

        public void Dispose()
        {
            try
            {
                Directory.Delete(path, recursive: true);
            }
            catch (IOException)
            {
                // A temporary test directory must not hide a conversion result.
            }
            catch (UnauthorizedAccessException)
            {
                // A temporary test directory must not hide a conversion result.
            }
        }
    }

    private static class NativeTestLibrary
    {
        public static void Configure()
        {
            if (!string.IsNullOrWhiteSpace(
                    Environment.GetEnvironmentVariable("TYDOCX_NATIVE_LIBRARY")))
            {
                return;
            }

            DirectoryInfo? directory = new(AppContext.BaseDirectory);
            while (directory is not null)
            {
                if (File.Exists(Path.Combine(directory.FullName, "crates", "tydocx-ffi", "Cargo.toml")))
                {
                    string nativePath = Path.Combine(directory.FullName, "target", "debug", NativeFileName());
                    if (!File.Exists(nativePath))
                    {
                        throw new InvalidOperationException(
                            $"Native test library not found: {nativePath}. Run 'cargo build -p tydocx-ffi' first.");
                    }

                    Environment.SetEnvironmentVariable("TYDOCX_NATIVE_LIBRARY", nativePath);
                    return;
                }

                directory = directory.Parent;
            }

            throw new InvalidOperationException(
                "Could not locate the repository. Set TYDOCX_NATIVE_LIBRARY to the native test library.");
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
