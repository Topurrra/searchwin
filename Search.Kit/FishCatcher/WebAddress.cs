using System.Net;
using System.Text;

namespace SearchKit.FishCatcher;

/// The parts of an address the engine reads, as the WHATWG URL parser (what
/// the extension's `new URL()` and Chromium use) would give them.
///
/// .NET's Uri gets most of the way: it lower-cases, maps Unicode names to
/// Punycode with the same UTS #46 rules, and reads the short and hex IPv4
/// forms (0x7f.1, 3232235777). What it does differently is fixed up here:
/// - a name ending in a number (999.1.1.1, example.1) is a bad IPv4 address
///   to WHATWG, so there's no verdict for it, as there was none in the extension;
/// - IPv6 is written the WHATWG way (::ffff:1.2.3.4 is [::ffff:102:304]);
/// - the user name is the part of the user info before the colon;
/// - `%2e` and friends in a host name are decoded (Uri refuses them, and
///   paypal.com%2eevil.com is exactly the kind of trick worth a verdict).
public readonly record struct WebAddress(string Scheme, string Host, string UserName)
{
    public bool IsHttp => Scheme == "http";

    /// Null for anything that isn't an http(s) address with a host.
    public static WebAddress? From(Uri url)
    {
        if (!url.IsAbsoluteUri) return null;
        string scheme = url.Scheme;
        if (scheme != "http" && scheme != "https") return null;

        string host;
        if (url.HostNameType == UriHostNameType.IPv6)
        {
            if (!IPAddress.TryParse(url.DnsSafeHost, out var ip)) return null;
            host = Ipv6(ip);
        }
        else
        {
            try { host = url.IdnHost; }
            catch (Exception) { return null; }
            if (host.Length == 0) return null;
            host = host.ToLowerInvariant();
            if (EndsInNumber(host))
            {
                var v4 = Ipv4(host);
                if (v4 is null) return null;
                host = v4;
            }
        }

        string info = url.UserInfo;
        int colon = info.IndexOf(':');
        string user = colon < 0 ? info : info[..colon];
        return new WebAddress(scheme, host, user);
    }

    /// Parses text the way the extension's `new URL(text)` would, as far as the
    /// engine can tell. Null where WHATWG would throw or the address isn't web.
    public static WebAddress? From(string text)
    {
        if (string.IsNullOrWhiteSpace(text)) return null;
        if (Uri.TryCreate(text, UriKind.Absolute, out var url)) return From(url);
        // Uri refuses percent-escapes in a host; WHATWG decodes them first.
        var fixedUp = DecodeHostEscapes(text);
        return fixedUp is not null && Uri.TryCreate(fixedUp, UriKind.Absolute, out url) ? From(url) : null;
    }

    private static string? DecodeHostEscapes(string text)
    {
        int scheme = text.IndexOf("://", StringComparison.Ordinal);
        if (scheme <= 0) return null;
        int start = scheme + 3;
        int end = text.IndexOfAny(['/', '?', '#', '\\'], start);
        if (end < 0) end = text.Length;
        var authority = text.AsSpan(start, end - start);
        int at = authority.LastIndexOf('@');
        int hostStart = start + at + 1;
        var host = text.AsSpan(hostStart, end - hostStart);
        if (!host.Contains('%')) return null;
        string decoded;
        try { decoded = Uri.UnescapeDataString(host.ToString()); }
        catch (Exception) { return null; }
        if (decoded.Contains('%')) return null;
        return string.Concat(text.AsSpan(0, hostStart), decoded, text.AsSpan(end));
    }

    // WHATWG "ends in a number": the last label (ignoring one empty label after
    // a trailing dot) is all digits, or 0x followed by hex digits.
    private static bool EndsInNumber(string host)
    {
        var labels = host.Split('.');
        int last = labels.Length - 1;
        if (labels[last].Length == 0)
        {
            if (labels.Length == 1) return false;
            last--;
        }
        string label = labels[last];
        if (label.Length > 0 && label.All(char.IsAsciiDigit)) return true;
        return label.StartsWith("0x", StringComparison.Ordinal) && label.AsSpan(2).ContainsAnyExcept("0123456789abcdef") is false;
    }

    // The WHATWG IPv4 parser: 1 to 4 parts, each decimal, 0x-hex or 0-octal;
    // the last part fills the remaining bytes. Null for a failure.
    private static string? Ipv4(string host)
    {
        var parts = host.Split('.').ToList();
        if (parts[^1].Length == 0 && parts.Count > 1) parts.RemoveAt(parts.Count - 1);
        if (parts.Count > 4) return null;
        var numbers = new List<ulong>(4);
        foreach (var part in parts)
        {
            var n = Ipv4Number(part);
            if (n is null) return null;
            numbers.Add(n.Value);
        }
        for (int i = 0; i < numbers.Count - 1; i++)
            if (numbers[i] > 255) return null;
        if (numbers[^1] >= Math.Pow(256, 5 - numbers.Count)) return null;
        ulong ipv4 = numbers[^1];
        for (int i = 0; i < numbers.Count - 1; i++) ipv4 += numbers[i] << (8 * (3 - i));
        return $"{ipv4 >> 24}.{(ipv4 >> 16) & 255}.{(ipv4 >> 8) & 255}.{ipv4 & 255}";
    }

    private static ulong? Ipv4Number(string part)
    {
        if (part.Length == 0) return null;
        int radix = 10;
        var digits = part.AsSpan();
        if (digits.Length >= 2 && (digits.StartsWith("0x") || digits.StartsWith("0X"))) { radix = 16; digits = digits[2..]; }
        else if (digits.Length >= 2 && digits[0] == '0') { radix = 8; digits = digits[1..]; }
        if (digits.Length == 0) return 0;
        ulong value = 0;
        foreach (char c in digits)
        {
            int d = c is >= '0' and <= '9' ? c - '0' : c is >= 'a' and <= 'f' ? c - 'a' + 10 : c is >= 'A' and <= 'F' ? c - 'A' + 10 : 99;
            if (d >= radix) return null;
            value = value * (ulong)radix + (ulong)d;
            if (value > uint.MaxValue * 256UL) value = uint.MaxValue * 256UL; // big enough to fail the range check, no overflow
        }
        return value;
    }

    // WHATWG IPv6 serialisation: lower-case hex pieces, the first longest run
    // of two or more zero pieces written as "::", no embedded IPv4 form.
    private static string Ipv6(IPAddress ip)
    {
        Span<byte> bytes = stackalloc byte[16];
        ip.TryWriteBytes(bytes, out _);
        Span<int> pieces = stackalloc int[8];
        for (int i = 0; i < 8; i++) pieces[i] = (bytes[2 * i] << 8) | bytes[2 * i + 1];
        int bestStart = -1, bestLen = 1;
        for (int i = 0; i < 8;)
        {
            if (pieces[i] != 0) { i++; continue; }
            int j = i;
            while (j < 8 && pieces[j] == 0) j++;
            if (j - i > bestLen) { bestStart = i; bestLen = j - i; }
            i = j;
        }
        var sb = new StringBuilder("[");
        bool ignore0 = false;
        for (int i = 0; i < 8; i++)
        {
            if (ignore0 && pieces[i] == 0) continue;
            ignore0 = false;
            if (i == bestStart)
            {
                sb.Append(i == 0 ? "::" : ":");
                ignore0 = true;
                continue;
            }
            sb.Append(pieces[i].ToString("x"));
            if (i != 7) sb.Append(':');
        }
        return sb.Append(']').ToString();
    }
}
