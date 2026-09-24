using System.Text.RegularExpressions;
using static SearchKit.FishCatcher.JsText;

namespace SearchKit.FishCatcher;

/// Device-code phishing. Port of devicecode.js.
///
/// The page sends you to a real device sign-in address (microsoft.com/link,
/// google.com/device…) to type a code the scammer shows you. The address is
/// clean; only the text gives it away, and all three must be there: the entry
/// address, an instruction to type a code, and a code-shaped string next to
/// the word "code". A page whose title is about phishing or scams is a lesson,
/// not a lure, and is never flagged.
public static partial class DeviceCode
{
    public const int Weight = 100;

    [GeneratedRegex(@"(microsoft\.com/link|devicelogin|google\.com/device|amazon\.com/code)")]
    private static partial Regex Entry();

    [GeneratedRegex($@"(enter|input|type|use|შეიყვან|введи|введите){Dot}{{0,60}}(code|კოდი|код)")]
    private static partial Regex Instruct();

    [GeneratedRegex($@"code[^.\n]{{0,40}}{B}([A-Z0-9]{{4}}-[A-Z0-9]{{4,5}}|[A-Z0-9]{{8,9}}){B}|{B}([A-Z0-9]{{4}}-[A-Z0-9]{{4,5}}|[A-Z0-9]{{8,9}}){B}[^.\n]{{0,40}}code")]
    private static partial Regex CodeNear();

    [GeneratedRegex("phish|scam|fraud|attack|security|how to spot|awareness", RegexOptions.IgnoreCase | RegexOptions.CultureInvariant)]
    private static partial Regex EducationalTitle();

    public static bool MatchText(string text, string title = "")
    {
        if (EducationalTitle().IsMatch(title)) return false;
        var t = Lower(text);
        if (!Entry().IsMatch(t) || !Instruct().IsMatch(t)) return false;
        return CodeNear().IsMatch(text);
    }
}
