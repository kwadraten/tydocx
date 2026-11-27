namespace Tydocx;

/// <summary>Controls the OOXML compatibility features used during conversion.</summary>
public enum CompatMode
{
    /// <summary>Only the Typst/OOXML intersection; unsupported aspects are skipped.</summary>
    C1,

    /// <summary>Graphical compatibility, the default.</summary>
    C2,

    /// <summary>Dynamic compatibility, including field codes in addition to C2.</summary>
    C3,
}
