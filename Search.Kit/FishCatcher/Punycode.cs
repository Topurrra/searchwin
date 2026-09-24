using System.Text;

namespace SearchKit.FishCatcher;

/// Punycode (RFC 3492) decoding and look-alike folding. Port of punycode.js.
///
/// The decoder is our own rather than IdnMapping.GetUnicode on purpose: the
/// engine decodes each xn-- label on its own and without IDNA's validity rules
/// (IdnMapping refuses labels whose decoded text breaks IDNA rules, and the
/// point here is to *see* such a label, not to refuse it). The address itself
/// is already in ASCII form by the time this runs (see WebAddress).
public static class Punycode
{
    private const int Base = 36, TMin = 1, TMax = 26, Skew = 38, Damp = 700, InitialBias = 72, InitialN = 0x80;

    private static long Adapt(long delta, long numPoints, bool firstTime)
    {
        delta = firstTime ? delta / Damp : delta / 2;
        delta += delta / numPoints;
        long k = 0;
        while (delta > ((Base - TMin) * TMax) / 2)
        {
            delta /= Base - TMin;
            k += Base;
        }
        return k + ((Base - TMin + 1) * delta) / (delta + Skew);
    }

    private static int DecodeDigit(int cp)
    {
        if (cp - 48 < 10) return cp - 22;
        if (cp - 65 < 26) return cp - 65;
        if (cp - 97 < 26) return cp - 97;
        return Base;
    }

    /// Decodes one label's Punycode (without its "xn--"). Throws
    /// FormatException where punycode.js throws.
    public static string Decode(string input)
    {
        var output = new List<int>(input.Length);
        int delim = input.LastIndexOf('-');
        long i = 0, bias = InitialBias, n = InitialN;
        for (int j = 0; j < delim; j++) output.Add(input[j]);
        int index = delim < 0 ? 0 : delim + 1;
        while (index < input.Length)
        {
            long oldi = i;
            long w = 1;
            for (long k = Base; ; k += Base)
            {
                if (index >= input.Length) throw new FormatException("invalid punycode");
                int digit = DecodeDigit(input[index++]);
                // Math.floor((0x7fffffff - i) / w), as punycode.js computes it.
                if (digit > Math.Floor((0x7fffffff - i) / (double)w)) throw new FormatException("punycode overflow");
                i += digit * w;
                long t = k <= bias ? TMin : k >= bias + TMax ? TMax : k - bias;
                if (digit < t) break;
                w *= Base - t;
            }
            bias = Adapt(i - oldi, output.Count + 1, oldi == 0);
            n += i / (output.Count + 1);
            i %= output.Count + 1;
            output.Insert((int)i, (int)n);
            i++;
        }
        return FromCodePoints(output);
    }

    // String.fromCodePoint: lone surrogate values pass through as themselves,
    // anything past U+10FFFF is an error.
    private static string FromCodePoints(List<int> points)
    {
        var sb = new StringBuilder(points.Count + 4);
        foreach (int cp in points)
        {
            if (cp < 0 || cp > 0x10FFFF) throw new FormatException("invalid code point");
            if (cp < 0x10000) sb.Append((char)cp);
            else
            {
                int v = cp - 0x10000;
                sb.Append((char)(0xD800 + (v >> 10)));
                sb.Append((char)(0xDC00 + (v & 0x3FF)));
            }
        }
        return sb.ToString();
    }

    /// Decodes every xn-- label of a host name; other labels pass through.
    public static string DecodeHost(string host)
    {
        if (!host.Contains("xn--", StringComparison.Ordinal)) return host;
        var labels = host.Split('.');
        for (int i = 0; i < labels.Length; i++)
            if (labels[i].StartsWith("xn--", StringComparison.Ordinal)) labels[i] = Decode(labels[i][4..]);
        return string.Join('.', labels);
    }

    // Confusable non-ASCII characters folded to their Latin look-alikes.
    private static readonly Dictionary<char, char> Homoglyphs = new()
    {
        ['а'] = 'a', ['е'] = 'e', ['о'] = 'o', ['с'] = 'c', ['р'] = 'p', ['х'] = 'x', ['у'] = 'y', ['ѕ'] = 's',
        ['і'] = 'i', ['ј'] = 'j', ['ԁ'] = 'd', ['ɡ'] = 'g', ['һ'] = 'h', ['κ'] = 'k', ['м'] = 'm', ['т'] = 't',
        ['в'] = 'b', ['ο'] = 'o', ['α'] = 'a', ['ε'] = 'e', ['ι'] = 'i', ['ν'] = 'v', ['ω'] = 'w',
        ['ｌ'] = 'l', ['０'] = '0', ['１'] = '1', ['２'] = '2', ['５'] = '5', ['－'] = '-',
    };

    /// Every key is a single UTF-16 unit, so walking chars (not code points)
    /// folds exactly what the JS walk over code points folds.
    public static string AsciiFold(string text)
    {
        char[]? copy = null;
        for (int i = 0; i < text.Length; i++)
        {
            if (text[i] < 0x80 || !Homoglyphs.TryGetValue(text[i], out var latin)) continue;
            copy ??= text.ToCharArray();
            copy[i] = latin;
        }
        return copy is null ? text : new string(copy);
    }

    private static bool IsLatin(char c) => c is >= 'a' and <= 'z' or >= '0' and <= '9';
    private static bool IsCyrillic(char c) => c is >= 'а' and <= 'я' or 'і' or 'ї' or 'ѓ' or 'ѕ' or 'ј' or 'ԁ' or 'һ';
    private static bool IsGreek(char c) => c is >= 'α' and <= 'ω';

    /// Mixed scripts in one host is a strong IDN-attack sign.
    public static bool HasMixedScripts(string text)
    {
        bool latin = false, cyrillic = false, greek = false;
        foreach (char c in text)
        {
            if (IsLatin(c)) latin = true;
            else if (IsCyrillic(c)) cyrillic = true;
            else if (IsGreek(c)) greek = true;
        }
        return (latin && cyrillic) || (latin && greek) || (cyrillic && greek);
    }
}
