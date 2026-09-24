using System.Globalization;
using System.Text;
using System.Text.Json;

namespace SearchKit.FishCatcher;

/// JSON.stringify(JSON.parse(text)), byte for byte. The feed's signature is
/// over the JSON that the registry's JavaScript writes (remote.js
/// bundlePayload), so to check it we have to write the same text: JS key
/// order (array-index keys first, ascending), JS number forms (1e+21, 1e-7,
/// no -0) and JS string escapes (only quotes, backslashes and controls).
public static class JsJson
{
    public static string Stringify(JsonElement value)
    {
        var sb = new StringBuilder();
        Write(sb, value);
        return sb.ToString();
    }

    public static void Write(StringBuilder sb, JsonElement value)
    {
        switch (value.ValueKind)
        {
            case JsonValueKind.Null: sb.Append("null"); break;
            case JsonValueKind.True: sb.Append("true"); break;
            case JsonValueKind.False: sb.Append("false"); break;
            case JsonValueKind.Number: sb.Append(Number(double.Parse(value.GetRawText(), NumberStyles.Float, CultureInfo.InvariantCulture))); break;
            // From the raw text: GetString refuses a lone surrogate ("\ud800"),
            // which JSON.parse keeps and JSON.stringify writes back.
            case JsonValueKind.String: Quote(sb, Unescape(value.GetRawText())); break;
            case JsonValueKind.Array:
                sb.Append('[');
                bool firstItem = true;
                foreach (var item in value.EnumerateArray())
                {
                    if (!firstItem) sb.Append(',');
                    firstItem = false;
                    Write(sb, item);
                }
                sb.Append(']');
                break;
            case JsonValueKind.Object:
                sb.Append('{');
                bool firstKey = true;
                foreach (var (name, item) in JsOrder(value))
                {
                    if (!firstKey) sb.Append(',');
                    firstKey = false;
                    Quote(sb, name);
                    sb.Append(':');
                    Write(sb, item);
                }
                sb.Append('}');
                break;
            default: throw new JsonException("not a JSON value");
        }
    }

    // A JS object's own keys: integer indices ascending, then the rest in the
    // order they first appeared. A repeated key keeps its first place and its last value.
    private static List<(string, JsonElement)> JsOrder(JsonElement obj)
    {
        var order = new List<string>();
        var values = new Dictionary<string, JsonElement>(StringComparer.Ordinal);
        foreach (var p in obj.EnumerateObject())
        {
            if (!values.ContainsKey(p.Name)) order.Add(p.Name);
            values[p.Name] = p.Value;
        }
        var indices = order.Where(IsArrayIndex).OrderBy(k => uint.Parse(k, CultureInfo.InvariantCulture)).ToList();
        var rest = order.Where(k => !IsArrayIndex(k));
        return indices.Concat(rest).Select(k => (k, values[k])).ToList();
    }

    // A canonical numeric string of an integer below 2^32 - 1: "0", "10", not "01".
    private static bool IsArrayIndex(string key) =>
        key.Length > 0 && key.Length <= 10 && key.All(char.IsAsciiDigit) && (key == "0" || key[0] != '0') &&
        ulong.Parse(key, CultureInfo.InvariantCulture) < uint.MaxValue;

    /// Number.prototype.toString for a double (ECMA-262 Number::toString).
    public static string Number(double x)
    {
        if (double.IsNaN(x) || double.IsInfinity(x)) return "null"; // what JSON.stringify writes
        if (x == 0) return "0"; // -0 too
        if (x < 0) return "-" + Number(-x);

        // The shortest digits that round-trip, and the exponent n with x = 0.digits × 10^n.
        var shortest = x.ToString("R", CultureInfo.InvariantCulture); // "1.5", "1E+21", "1E-07"
        string mantissa = shortest;
        int exp = 0;
        int e = shortest.IndexOfAny(['E', 'e']);
        if (e >= 0)
        {
            mantissa = shortest[..e];
            exp = int.Parse(shortest[(e + 1)..], NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture);
        }
        int point = mantissa.IndexOf('.');
        string digits = point < 0 ? mantissa : mantissa[..point] + mantissa[(point + 1)..];
        int intLen = point < 0 ? mantissa.Length : point;
        int lead = 0;
        while (lead < digits.Length - 1 && digits[lead] == '0') lead++;
        digits = digits[lead..].TrimEnd('0');
        if (digits.Length == 0) return "0";
        int n = intLen - lead + exp; // position of the decimal point relative to the digits
        int k = digits.Length;

        if (k <= n && n <= 21) return digits + new string('0', n - k);
        if (0 < n && n <= 21) return digits[..n] + "." + digits[n..];
        if (-6 < n && n <= 0) return "0." + new string('0', -n) + digits;
        string sign = n - 1 < 0 ? "-" : "+";
        string expText = Math.Abs(n - 1).ToString(CultureInfo.InvariantCulture);
        return k == 1 ? digits + "e" + sign + expText : digits[0] + "." + digits[1..] + "e" + sign + expText;
    }

    /// A JSON string token (with its quotes) to its UTF-16 text, lone surrogates kept.
    public static string Unescape(string raw)
    {
        var sb = new StringBuilder(raw.Length);
        for (int i = 1; i < raw.Length - 1; i++)
        {
            char c = raw[i];
            if (c != '\\') { sb.Append(c); continue; }
            char e = raw[++i];
            switch (e)
            {
                case 'b': sb.Append('\b'); break;
                case 'f': sb.Append('\f'); break;
                case 'n': sb.Append('\n'); break;
                case 'r': sb.Append('\r'); break;
                case 't': sb.Append('\t'); break;
                case 'u':
                    sb.Append((char)int.Parse(raw.AsSpan(i + 1, 4), NumberStyles.AllowHexSpecifier, CultureInfo.InvariantCulture));
                    i += 4;
                    break;
                default: sb.Append(e); break; // \" \\ \/
            }
        }
        return sb.ToString();
    }

    /// JSON.stringify's QuoteJSONString (well-formed: lone surrogates as \uXXXX).
    public static void Quote(StringBuilder sb, string s)
    {
        sb.Append('"');
        for (int i = 0; i < s.Length; i++)
        {
            char c = s[i];
            switch (c)
            {
                case '\b': sb.Append("\\b"); break;
                case '\t': sb.Append("\\t"); break;
                case '\n': sb.Append("\\n"); break;
                case '\f': sb.Append("\\f"); break;
                case '\r': sb.Append("\\r"); break;
                case '"': sb.Append("\\\""); break;
                case '\\': sb.Append("\\\\"); break;
                default:
                    if (c < 0x20) sb.Append("\\u").Append(((int)c).ToString("x4", CultureInfo.InvariantCulture));
                    else if (char.IsHighSurrogate(c) && i + 1 < s.Length && char.IsLowSurrogate(s[i + 1])) sb.Append(c).Append(s[++i]);
                    else if (char.IsSurrogate(c)) sb.Append("\\u").Append(((int)c).ToString("x4", CultureInfo.InvariantCulture));
                    else sb.Append(c);
                    break;
            }
        }
        sb.Append('"');
    }
}
