namespace SearchKit.FishCatcher;

/// Pieces of JavaScript's text semantics the matchers depend on. The page
/// matchers are ports of JS regular expressions, and .NET's `\b`, `\s` and
/// `.` mean slightly different things, so the patterns spell JS's out.
/// (Special characters are written as C# `\x` escapes, so these are plain
/// strings, not verbatim ones.)
internal static class JsText
{
    /// JS `\b` (not in Unicode mode): a boundary between [A-Za-z0-9_] and anything else.
    public const string B = "(?:(?<=[A-Za-z0-9_])(?![A-Za-z0-9_])|(?<![A-Za-z0-9_])(?=[A-Za-z0-9_]))";

    /// The members of JS `\s`, for use inside a character class: the ASCII
    /// controls, every space separator, the two line separators and the BOM.
    public const string S = "\\t\\n\\v\\f\\r\\p{Zs}\\p{Zl}\\p{Zp}\xFEFF";

    /// JS `.`: anything but a line terminator.
    public const string Dot = "[^\\n\\r\x2028\x2029]";

    private const char DottedCapitalI = '\x0130';

    /// String.prototype.toLowerCase. .NET's invariant lower-casing maps one
    /// char to one char; JS also expands U+0130 (capital I with a dot) to "i"
    /// plus a combining dot, which shifts positions in the text, so that one
    /// is done by hand.
    public static string Lower(string text)
    {
        if (!text.Contains(DottedCapitalI)) return text.ToLowerInvariant();
        return string.Join("i\x0307", text.Split(DottedCapitalI).Select(p => p.ToLowerInvariant()));
    }

    /// String.prototype.trim() left nothing: JS's white space and line terminators only.
    public static bool IsBlank(string text)
    {
        foreach (char c in text)
        {
            bool space = c is '\t' or '\n' or '\v' or '\f' or '\r' or '\xFEFF'
                || char.GetUnicodeCategory(c) is System.Globalization.UnicodeCategory.SpaceSeparator
                    or System.Globalization.UnicodeCategory.LineSeparator
                    or System.Globalization.UnicodeCategory.ParagraphSeparator;
            if (!space) return false;
        }
        return true;
    }
}
