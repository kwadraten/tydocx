namespace Tydocx;

/// <summary>The category reported by the native converter.</summary>
public enum TydocxErrorCode
{
    /// <summary>An argument or UTF-8 value was invalid.</summary>
    InvalidArgument = 1,

    /// <summary>The input document could not be read.</summary>
    Input = 2,

    /// <summary>Typst compilation failed.</summary>
    Compilation = 3,

    /// <summary>The output document could not be written.</summary>
    Output = 4,

    /// <summary>A native panic was caught at the FFI boundary.</summary>
    Internal = 5,
}
