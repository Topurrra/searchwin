namespace SearchKit.FishCatcher;

/// FishCatcher's reasons as English sentences, from the extension's
/// _locales/en/messages.json. `$1`, `$2` are the signal's parameters.
public static class Messages
{
    private static readonly Dictionary<string, string> Text = new(StringComparer.Ordinal)
    {
        ["reasonBrand"] = "Domain looks like a misspelling of $1",
        ["reasonHomoglyph"] = "Uses look-alike characters to imitate $1",
        ["reasonBrandSubdomain"] = "Mentions $1 but is not a $1 domain",
        ["reasonIp"] = "Address is a raw IP number. Legitimate sites rarely do this",
        ["reasonAtSign"] = "Everything before @ in the address is ignored by the browser",
        ["reasonTld"] = "Uses a high-abuse domain ending (.$1)",
        ["reasonKeyword"] = "Contains a word often used in phishing pages ($1)",
        ["reasonSubdomains"] = "Unusually deep chain of subdomains ($1 parts)",
        ["reasonHttp"] = "Connection is not encrypted (http instead of https)",
        ["reasonDigits"] = "Domain has an unusual mix of digits and hyphens",
        ["reasonShort"] = "Very short, random-looking domain",
        ["reasonBlocklist"] = "This domain is on the local blocklist of known-bad sites",
        ["reasonPasswordForm"] = "The page asks for a password on a site that isn't a known legitimate domain",
        ["reasonDeviceCode"] = "This page tries to make you enter a device-login code, a known scam pattern",
        ["reasonYoungDomain"] = "Domain was registered only $1 days ago. Fresh domains are common in phishing",
        ["reasonBloom"] = "Matches the community threat feed (probabilistic match)",
        ["reasonMl"] = "The address matches letter patterns common in phishing and randomly generated domains",
        ["reasonAitmMismatch"] = "This page asks for your $1 sign-in, but $2 is not an official $1 site",
        ["reasonAitmResourceGraph"] = "Nearly all of this sign-in page is served from a single unrelated site, a pattern seen in relay attacks",
        ["reasonAitmDrift"] = "The site icon or app manifest is served from a different domain than the page itself",
        ["reasonGsbDeceptive"] = "Google Safe Browsing flags this as a deceptive (phishing) site",
        ["reasonGsbMalware"] = "Google Safe Browsing flags this as a malware site",
        ["reasonGsbUnwanted"] = "Google Safe Browsing flags this as hosting unwanted software",
        ["reasonGsbHarmfulApp"] = "Google Safe Browsing flags this as a harmful app",
        ["reasonGsbUnsafe"] = "Google Safe Browsing flags this site as unsafe",
        ["reasonCryptoSeed"] = "This page asks you to type your wallet recovery phrase or private key. Real wallets never ask for this",
        ["reasonTechSupport"] = "This page tries to scare you into calling a number to unlock your computer. That is a known scam",
        ["reasonFormAction"] = "The login form sends what you type to $1, not to this site",
        ["reasonFormExfil"] = "The login form sends what you type to $1, a service scammers use to collect passwords",
        ["linkTextMismatch"] = "Link shows $1 but actually goes to $2",
        ["linkDownloadMismatch"] = "Saved as .$2 but it is really a .$1 file",
        ["linkShortener"] = "Shortened link ($1). The real destination is hidden",
        ["linkRisky"] = "$1 looks risky",
        ["downloadDangerousBody"] = "\"$1\" is a program ($2), not a document. Only open it if you trust the source.",
        ["downloadMismatchBody"] = "\"$1\" is actually a $2, not what its name suggests. Do not open it unless you trust the source.",
    };

    /// Whether this key has a sentence. Every reason the engine gives has one.
    public static bool Knows(string key) => Text.ContainsKey(key);

    /// Safe Browsing's reason keys: the only ones a Safe Browsing answer may carry.
    public static bool IsSafeBrowsing(string key) => key.StartsWith("reasonGsb", StringComparison.Ordinal) && Knows(key);

    /// The sentence for a key, or "" for a key it doesn't know. Never the key
    /// itself: a key that came from outside (a page's facts) would otherwise
    /// put its own words on Search's warning.
    public static string Sentence(string key, IReadOnlyList<string> args)
    {
        if (!Text.TryGetValue(key, out var text)) return "";
        for (int i = args.Count; i >= 1; i--) text = text.Replace("$" + i, args[i - 1], StringComparison.Ordinal);
        return text;
    }

    public static string ForLevel(Level level) => level switch
    {
        Level.Critical => "Strong phishing signs. Don't enter passwords here",
        Level.High => "Multiple phishing indicators. Be careful before logging in",
        Level.Elevated => "Some unusual signs. Double-check the address",
        _ => "No suspicious signs found",
    };
}
