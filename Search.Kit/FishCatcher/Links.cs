using System.Text.RegularExpressions;

namespace SearchKit.FishCatcher;

/// A link on a page, as the probe collects it.
public sealed record PageLink(string Href, string Text = "", string Download = "");

/// A finding about a link: an i18n key, its parameters, and the link.
public sealed record LinkFinding(string Key, IReadOnlyList<string> Params, string Href)
{
    public string Sentence => Messages.Sentence(Key, Params);
}

/// A download whose name hides what it really is. Body is linkDownload's
/// message key, Arg its second parameter.
public sealed record DownloadWarning(string Body, string Arg)
{
    public string SentenceFor(string name) => Messages.Sentence(Body, [name, Arg]);
}

/// Link checks and download-type checks. Port of links.js.
public static partial class Links
{
    /// Real file types that are programs.
    public static readonly HashSet<string> DangerousExt = new(StringComparer.Ordinal)
    {
        "exe", "scr", "bat", "cmd", "com", "pif", "msi", "msix", "msp", "vbs", "vbe", "js", "jse", "jar", "apk", "dmg", "app", "ps1",
        "sh", "hta", "wsf", "wsh", "reg", "lnk", "gadget", "cpl", "deb", "rpm", "iso",
    };

    /// Types that look like harmless documents.
    public static readonly HashSet<string> DocExt = new(StringComparer.Ordinal)
    {
        "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "rtf", "csv", "odt", "ods", "jpg", "jpeg", "png", "gif", "webp", "svg",
        "bmp", "mp3", "wav", "mp4", "mov", "avi", "zip", "rar", "7z", "tar", "gz",
    };

    private const RegexOptions I = RegexOptions.IgnoreCase | RegexOptions.CultureInvariant;

    [GeneratedRegex("(x-msdownload|x-ms-installer|x-msi|x-dosexec|msdos-program|portable-executable|x-executable|x-elf|x-sh|x-shellscript|mach-o|x-apple-diskimage|vnd\\.android\\.package-archive|java-archive)", I)]
    private static partial Regex ExecMime();

    [GeneratedRegex("msdownload|dosexec|portable-executable|msdos-program", I)] private static partial Regex MimeExe();
    [GeneratedRegex("x-msi|ms-installer", I)] private static partial Regex MimeMsi();
    [GeneratedRegex("apple-diskimage", I)] private static partial Regex MimeDmg();
    [GeneratedRegex("android\\.package", I)] private static partial Regex MimeApk();
    [GeneratedRegex("java-archive", I)] private static partial Regex MimeJar();
    [GeneratedRegex("x-sh|shellscript|x-elf|x-executable|mach-o", I)] private static partial Regex MimeScript();

    private static readonly (Func<Regex> Pattern, string Label)[] MimeLabels =
    [
        (MimeExe, "Windows program (.exe)"), (MimeMsi, "Windows installer (.msi)"), (MimeDmg, "Mac disk image (.dmg)"),
        (MimeApk, "Android app (.apk)"), (MimeJar, "Java program (.jar)"), (MimeScript, "program or script"),
    ];

    // Known URL shorteners: the real destination is hidden behind them.
    private static readonly HashSet<string> Shorteners = new(StringComparer.Ordinal)
    {
        "bit.ly", "t.co", "tinyurl.com", "goo.gl", "ow.ly", "buff.ly", "is.gd", "cutt.ly", "rebrand.ly", "t.ly", "rb.gy",
        "shorturl.at", "tiny.cc", "bit.do", "soo.gd", "lnkd.in",
    };

    // (?:https?:\/\/)?([a-z0-9-]+(?:\.[a-z0-9-]+)+) with /i, the classes spelled
    // out so .NET's case folding can't add non-ASCII look-alikes.
    [GeneratedRegex("(?:[Hh][Tt][Tt][Pp][Ss]?://)?([A-Za-z0-9-]+(?:\\.[A-Za-z0-9-]+)+)")]
    private static partial Regex DomainPattern();

    /// The extension of a file name or address path, lower case, or "".
    public static string FileExt(string? name)
    {
        var s = name ?? "";
        int cut = s.IndexOfAny(['?', '#']);
        if (cut >= 0) s = s[..cut];
        s = s.TrimEnd('/');
        var tail = s[(s.LastIndexOf('/') + 1)..];
        int dot = tail.LastIndexOf('.');
        return dot > 0 ? JsText.Lower(tail[(dot + 1)..]) : "";
    }

    /// The registrable domain an http(s) address goes to, or null.
    public static string? RegistrableOf(string input, FishData data)
    {
        if (WebAddress.From(input) is not { } address) return null;
        var host = address.Host.EndsWith('.') ? address.Host[..^1] : address.Host;
        return Signals.IsIpAddress(host) ? host : data.Psl.RegistrableDomain(host);
    }

    /// The first domain-looking text in a link's text ("www.paypal.com"), or null.
    public static string? DomainInText(string? text)
    {
        var m = DomainPattern().Match(text ?? "");
        return m.Success ? JsText.Lower(m.Groups[1].Value) : null;
    }

    /// Findings for a page's links. `deep` also scores each destination with the engine.
    public static List<LinkFinding> Classify(IEnumerable<PageLink> links, FishData data, bool deep)
    {
        var findings = new List<LinkFinding>();
        foreach (var l in links)
        {
            var destReg = RegistrableOf(l.Href, data);
            if (string.IsNullOrEmpty(destReg)) continue;

            if (!string.IsNullOrEmpty(l.Download))
            {
                var claimed = FileExt(l.Download);
                var real = Uri.TryCreate(l.Href, UriKind.Absolute, out var u) ? FileExt(u.AbsolutePath) : "";
                if (claimed.Length > 0 && real.Length > 0 && claimed != real && (DangerousExt.Contains(real) || DangerousExt.Contains(claimed)))
                {
                    findings.Add(new LinkFinding("linkDownloadMismatch", [real.ToUpperInvariant(), claimed.ToUpperInvariant()], l.Href));
                    continue;
                }
            }

            if (Shorteners.Contains(destReg))
            {
                findings.Add(new LinkFinding("linkShortener", [destReg], l.Href));
                continue;
            }

            var shown = string.IsNullOrEmpty(l.Text) ? null : DomainInText(l.Text);
            if (!string.IsNullOrEmpty(shown))
            {
                var shownReg = RegistrableOf("http://" + shown, data);
                if (!string.IsNullOrEmpty(shownReg) && shownReg != destReg && !data.SafeList.Contains(destReg))
                {
                    findings.Add(new LinkFinding("linkTextMismatch", [shownReg, destReg], l.Href));
                    continue;
                }
            }

            if (deep && !data.SafeList.Contains(destReg) && !data.TrustList.Contains(destReg))
            {
                var r = Analyzer.Analyze(l.Href, data, null);
                if (r is not null && r.Level >= Level.High)
                    findings.Add(new LinkFinding("linkRisky", [destReg], l.Href));
            }
        }
        var seen = new HashSet<string>(StringComparer.Ordinal);
        return findings.Where(f => seen.Add(f.Key + "|" + f.Href)).Take(50).ToList();
    }

    /// A download named like one thing but really another, or a program. Null when it looks fine.
    public static DownloadWarning? InspectDownload(string? name, string? mime)
    {
        var ext = FileExt(name);
        var parts = (name ?? "").Split('.');
        var inner = parts.Length >= 3 ? JsText.Lower(parts[^2]) : "";
        if (DangerousExt.Contains(ext) && DocExt.Contains(inner)) return new DownloadWarning("downloadMismatchBody", ext.ToUpperInvariant() + " program");
        if (DocExt.Contains(ext) && ExecMime().IsMatch(mime ?? ""))
        {
            var label = MimeLabels.FirstOrDefault(x => x.Pattern().IsMatch(mime!)).Label ?? "program";
            return new DownloadWarning("downloadMismatchBody", label);
        }
        if (DangerousExt.Contains(ext)) return new DownloadWarning("downloadDangerousBody", ext.ToUpperInvariant());
        return null;
    }
}
