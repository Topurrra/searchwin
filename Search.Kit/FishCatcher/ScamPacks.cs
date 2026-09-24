using System.Text.RegularExpressions;
using static SearchKit.FishCatcher.JsText;

namespace SearchKit.FishCatcher;

/// Scam packs: wallet drainers and tech-support scares. Port of scampacks.js.
///
/// Both text matchers need two things at once, to stay near zero false
/// alarms: an action verb next to the wallet-secret noun ("never share your
/// recovery phrase" has the noun but no instruction), and a scare phrase
/// together with a call to action.
public static partial class ScamPacks
{
    public const int CryptoWeight = 60; // a seed-phrase request: high alone
    public const int TechWeight = 55;   // a support scare with a phone number or full-screen lock: high alone

    // An instruction verb near a wallet-secret noun, in the same sentence.
    [GeneratedRegex(@"(enter|input|type|paste|import|confirm|validate|verify|provide|re-?enter)[^.!?\n]{0,30}(secret recovery phrase|seed phrase|seed words|recovery phrase|secret phrase|mnemonic( phrase)?|private key)")]
    private static partial Regex Seed();

    // Kills real wallets' own warnings ("we will never ask you to enter your seed phrase").
    [GeneratedRegex($@"{B}(never|not|avoid|nobody|no one|don'?t|won'?t|can'?t|will not|should not|shouldn'?t){B}")]
    private static partial Regex SeedNegation();

    [GeneratedRegex($@"(your |this )?(computer|pc|laptop|windows|mac|device|system){B}[^.!?\n]{{0,40}}{B}(infected|locked|blocked|compromised|hacked|disabled|suspended|at risk){B}|(virus|trojan|spyware|malware|ransomware)[^.!?\n]{{0,20}}{B}(detected|found|infection){B}|security (alert|warning|breach)|suspicious (sign[{S}-]?in|activity|login)|windows defender|do ?n'?t (restart|close|shut|turn off|power off)|do not (restart|close|shut|turn off|power off)|ვირუს|კომპიუტერი (დაბლოკ|ვირუს)|заблокирован|заражен|вирус")]
    private static partial Regex TechScare();

    [GeneratedRegex($@"{B}call{B}[^.!?\n]{{0,30}}{B}(support|technician|help ?line|number|toll|microsoft|apple|windows|now|immediately){B}|{B}(contact|dial|phone){B}[^.!?\n]{{0,25}}{B}(support|technician|help ?line|number|toll|microsoft|apple){B}|toll[{S}-]?free|{B}1[{S}.\-]?\(?8(00|88|77|66|55|44|33)\)?[{S}.\-]?[0-9]{{3}}[{S}.\-]?[0-9]{{4}}{B}|დარეკ|позвони")]
    private static partial Regex TechCallToAction();

    /// The page asks for a wallet's recovery phrase or private key.
    public static bool CryptoSeedText(string? text)
    {
        if (string.IsNullOrEmpty(text)) return false;
        var t = Lower(text);
        for (var m = Seed().Match(t); m.Success; m = m.NextMatch())
        {
            // A 24-char look-back for a negation.
            int from = Math.Max(0, m.Index - 24);
            if (!SeedNegation().IsMatch(t.AsSpan(from, m.Index - from))) return true;
        }
        return false;
    }

    /// A browser-locker or fake-support scare together with a call to action.
    public static bool TechSupportText(string? text)
    {
        if (string.IsNullOrEmpty(text)) return false;
        var t = Lower(text);
        return TechScare().IsMatch(t) && TechCallToAction().IsMatch(t);
    }

    internal static int Run(ScamFacts scam, List<Signal> reasons)
    {
        int score = 0;
        if (scam.CryptoSeed || scam.SeedInput)
        {
            score += CryptoWeight;
            reasons.Add(new Signal("reasonCryptoSeed", CryptoWeight));
        }
        if (scam.TechScare && (scam.Phone || scam.Fullscreen))
        {
            score += TechWeight;
            reasons.Add(new Signal("reasonTechSupport", TechWeight));
        }
        return score;
    }
}
