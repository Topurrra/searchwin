namespace SearchKit.Shields;

/// What compiling a list produced, for a Settings page ("847,203 rules
/// loaded, 1,204 skipped") and for the tests: how much of the list this
/// engine actually understood, so a gap in coverage is visible rather than
/// silently partial.
public sealed class FilterStats
{
    public int NetworkRules;
    public int NetworkExceptions;
    public int CosmeticRules;
    public int CosmeticExceptions;

    /// A line whose shape this parser recognised (a scriptlet, `#?#`
    /// procedural cosmetics, a network option outside the supported set)
    /// but doesn't act on — counted rather than approximated, so nothing is
    /// half-applied.
    public int SkippedUnsupported;

    public int Comments;
    public int Blank;

    public int Total => NetworkRules + NetworkExceptions + CosmeticRules + CosmeticExceptions + SkippedUnsupported + Comments + Blank;
}
