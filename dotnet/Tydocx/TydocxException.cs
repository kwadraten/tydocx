namespace Tydocx;

/// <summary>An error returned by the native tydocx converter.</summary>
public sealed class TydocxException : Exception
{
    /// <summary>Create an exception for a native conversion error.</summary>
    public TydocxException(TydocxErrorCode errorCode, string message)
        : base(message)
    {
        ErrorCode = errorCode;
    }

    /// <summary>The native error category.</summary>
    public TydocxErrorCode ErrorCode { get; }
}
