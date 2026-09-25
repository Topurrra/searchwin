using SearchKit.Commands;

namespace SearchKit.Field;

/// What kind of thing was typed. Decided before any source is asked, so each
/// source can tell at a glance whether the question is for it.
public enum QueryKind
{
    /// Nothing, or only a scope (`clip:`).
    Empty,
    /// Something that can only be a place: `github.com`, `localhost:3000`, `https://…`.
    Address,
    /// Words: search the web, and everything local.
    Words,
    /// `23*47`, `15% of 240`, `2^10`: the engine's calculator.
    Calculation,
    /// `10 km to mi`, `32f to c`: the engine's unit conversion.
    Conversion,
    /// `#e07a5f`, `#e07a5f to hsl`, `rgb(224, 122, 95)`.
    Color,
    /// `sha256 hello`, `md5 hello`.
    Hash,
    /// `base64 hello`, `base64 decode aGk=`, `url encode a b`.
    Encode,
    /// `json2yaml {"a":1}`.
    Format,
    /// `>close others`.
    Command,
    /// `!yt cats`, `cats !yt`.
    Bang,
    /// `? what's the catch here`.
    Ask,
}

/// Where a `files:`-style prefix narrows the field to.
public enum Scope
{
    All,
    Files,
    Apps,
    Clipboard,
    Tabs,
    History,
}

/// The field's text, read once per keystroke. Reading is plain string work
/// (no regex, no number parsing) so it costs microseconds, well inside the
/// 0.5 ms it is allowed.
public sealed record FieldQuery(string Typed, QueryKind Kind, string Text, Scope Scope = Scope.All)
{
    /// The bang, for `QueryKind.Bang`.
    public Bang? Bang { get; init; }

    /// For answers: the tool (`sha256`, `base64`, `json`, `hex`…), the mode
    /// (`encode`/`decode`, or the target of a format or a colour, like `yaml`
    /// or `hsl`) and what it works on. Null for anything else.
    public string? Tool { get; init; }
    public string? Mode { get; init; }
    public string? Operand { get; init; }

    /// An instant answer is wanted: the engine (or a hash or colour worked
    /// out here) answers, and Enter copies it.
    public bool IsAnswer => Kind is QueryKind.Calculation or QueryKind.Conversion or QueryKind.Color
        or QueryKind.Hash or QueryKind.Encode or QueryKind.Format;

    public static readonly FieldQuery None = new("", QueryKind.Empty, "");

    /// Reads what was typed. `bangs` finds `!yt`; without it a bang is plain
    /// text. `isAddress` is the browser's own test (`Address.Url(t) != null`);
    /// without it, `LooksLikeAddress`, a close copy of that test.
    public static FieldQuery Read(string typed, Bangs? bangs = null, Func<string, bool>? isAddress = null)
    {
        var text = typed.Trim();
        if (text.Length == 0) return None with { Typed = typed };

        switch (text[0])
        {
            // `\!important` searches for "!important": the rest, as words.
            case '\\':
                var literal = text[1..].TrimStart();
                return new(typed, literal.Length == 0 ? QueryKind.Empty : QueryKind.Words, literal);
            case '>' when text.Length > 1:
                return new(typed, QueryKind.Command, text[1..].Trim());
            case '?' when text.Length > 1:
                return new(typed, QueryKind.Ask, text[1..].Trim());
        }

        if (bangs != null && text.Contains('!') && FieldInput.Read(text, bangs) is FieldInput.ToBang bang)
            return new(typed, QueryKind.Bang, bang.Query) { Bang = bang.Bang };

        if (Scoped(typed, text) is { } scoped) return scoped;
        if (Answer(typed, text) is { } answer) return answer;
        if ((isAddress ?? LooksLikeAddress)(text)) return new(typed, QueryKind.Address, text);
        return new(typed, QueryKind.Words, text);
    }

    // MARK: - scopes

    private static readonly (string Prefix, Scope Scope)[] Prefixes =
    [
        ("clip", Scope.Clipboard), ("clipboard", Scope.Clipboard),
        ("files", Scope.Files),
        ("apps", Scope.Apps), ("app", Scope.Apps),
        ("tabs", Scope.Tabs),
        ("history", Scope.History),
    ];

    /// `clip: invoice`. Not `file:`, which starts a file:// address.
    private static FieldQuery? Scoped(string typed, string text)
    {
        var colon = text.IndexOf(':');
        if (colon < 3 || colon > 9) return null;
        var head = text.AsSpan(0, colon);
        foreach (var (prefix, scope) in Prefixes)
        {
            if (!head.Equals(prefix, StringComparison.OrdinalIgnoreCase)) continue;
            var rest = text[(colon + 1)..].Trim();
            // `apps://x` is somebody's link, not a scope.
            if (rest.StartsWith("//", StringComparison.Ordinal)) return null;
            return new(typed, rest.Length == 0 ? QueryKind.Empty : QueryKind.Words, rest, scope);
        }
        return null;
    }

    // MARK: - instant answers

    private static FieldQuery? Answer(string typed, string text)
    {
        var first = text[0];
        if (first == '#') return Color(typed, text);
        if (char.IsAsciiDigit(first) || ((first == '-' || first == '.' || first == '(') && text.Length > 1))
            return Conversion(typed, text) ?? Calculation(typed, text);
        if (char.IsAsciiLetter(first)) return ToolAnswer(typed, text) ?? CssColor(typed, text);
        return null;
    }

    /// `#e07a5f`, `#fff`, `#e07a5f80`, then optionally `to hsl`.
    private static FieldQuery? Color(string typed, string text)
    {
        var (token, rest) = Split(text);
        var hex = token.AsSpan(1);
        if (hex.Length is not (3 or 4 or 6 or 8)) return null;
        foreach (var c in hex) if (!char.IsAsciiHexDigit(c)) return null;
        var target = Target(rest);
        if (rest.Length > 0 && target == null) return null;
        return new(typed, QueryKind.Color, text) { Tool = "hex", Mode = target, Operand = token };
    }

    /// `rgb(224, 122, 95)` or `hsl(12, 67%, 63%)`, then optionally `to hex`.
    private static FieldQuery? CssColor(string typed, string text)
    {
        if (!(text.StartsWith("rgb", StringComparison.OrdinalIgnoreCase) || text.StartsWith("hsl", StringComparison.OrdinalIgnoreCase)))
            return null;
        var open = text.IndexOf('(');
        var close = text.IndexOf(')');
        if (open is < 3 or > 4 || close < open) return null;
        var rest = text[(close + 1)..].Trim();
        var target = Target(rest);
        if (rest.Length > 0 && target == null) return null;
        return new(typed, QueryKind.Color, text) { Tool = text[..3].ToLowerInvariant(), Mode = target, Operand = text[..(close + 1)] };
    }

    /// "to hsl", "in rgb", "as hex" → the target; anything else → null.
    private static string? Target(string rest)
    {
        if (rest.Length == 0) return null;
        var (word, format) = Split(rest);
        if (!(word.Equals("to", StringComparison.OrdinalIgnoreCase) || word.Equals("in", StringComparison.OrdinalIgnoreCase)
            || word.Equals("as", StringComparison.OrdinalIgnoreCase))) return null;
        return format.ToLowerInvariant() switch
        {
            "hex" => "hex",
            "rgb" or "rgba" => "rgb",
            "hsl" or "hsla" => "hsl",
            _ => null,
        };
    }

    private static readonly Dictionary<string, string> Hashes = new(StringComparer.OrdinalIgnoreCase)
    {
        ["md5"] = "md5", ["sha1"] = "sha1", ["sha-1"] = "sha1", ["sha256"] = "sha256", ["sha-256"] = "sha256",
        ["sha384"] = "sha384", ["sha-384"] = "sha384", ["sha512"] = "sha512", ["sha-512"] = "sha512",
    };

    /// The engine's encode_decode algorithms. The ones that are also everyday
    /// words ("url shortener", "html tutorial") need an explicit encode or
    /// decode, or typing them would turn a search into an answer.
    private static readonly Dictionary<string, (string Algorithm, bool NeedsMode)> Encoders = new(StringComparer.OrdinalIgnoreCase)
    {
        ["base64"] = ("base64", false), ["b64"] = ("base64", false),
        ["base64url"] = ("base64url", false), ["b64url"] = ("base64url", false),
        ["rot13"] = ("rot13", false),
        ["url"] = ("url", true), ["html"] = ("html", true), ["hex"] = ("hex", true), ["binary"] = ("binary", true),
    };

    private static readonly string[] Formats = ["json", "yaml", "yml", "toml", "xml"];

    /// `sha256 hello`, `base64 decode aGk=`, `urlencode a b`, `json2yaml {…}`.
    private static FieldQuery? ToolAnswer(string typed, string text)
    {
        var (word, rest) = Split(text);
        if (rest.Length == 0) return null;

        if (Hashes.TryGetValue(word, out var hash))
            return new(typed, QueryKind.Hash, text) { Tool = hash, Operand = rest };

        // `urlencode x`, `base64decode x`: the mode glued on.
        foreach (var glued in (ReadOnlySpan<string>)["encode", "decode"])
        {
            if (word.Length > glued.Length && word.EndsWith(glued, StringComparison.OrdinalIgnoreCase)
                && Encoders.TryGetValue(word[..^glued.Length], out var joined))
                return new(typed, QueryKind.Encode, text) { Tool = joined.Algorithm, Mode = glued, Operand = rest };
        }

        if (Encoders.TryGetValue(word, out var encoder))
        {
            var (next, after) = Split(rest);
            var mode = next.ToLowerInvariant() switch
            {
                "encode" or "enc" => "encode",
                "decode" or "dec" => "decode",
                _ => null,
            };
            if (mode != null && after.Length == 0) return null;
            if (mode == null && encoder.NeedsMode) return null;
            return new(typed, QueryKind.Encode, text)
            {
                Tool = encoder.Algorithm,
                Mode = mode ?? "encode",
                Operand = mode == null ? rest : after,
            };
        }

        var two = word.IndexOf('2');
        if (two > 0)
        {
            var from = Format(word[..two]);
            var to = Format(word[(two + 1)..]);
            if (from != null && to != null && from != to)
                return new(typed, QueryKind.Format, text) { Tool = from, Mode = to, Operand = rest };
        }
        return null;
    }

    private static string? Format(string name)
    {
        foreach (var format in Formats)
            if (name.Equals(format, StringComparison.OrdinalIgnoreCase)) return format == "yml" ? "yaml" : format;
        return null;
    }

    private static readonly string[] Separators = [" to ", " in ", " as ", "->"];

    /// The engine's units (Engine/src/commands/quick_actions.rs, `UNITS`), so
    /// "5 things to do" stays words. Add here when the engine learns more.
    private static readonly HashSet<string> Units = new(StringComparer.OrdinalIgnoreCase)
    {
        "m", "meter", "meters", "metre", "metres", "km", "kilometer", "kilometers", "kilometre", "kilometres",
        "cm", "centimeter", "centimeters", "centimetre", "centimetres", "mm", "millimeter", "millimeters", "millimetre", "millimetres",
        "in", "inch", "inches", "ft", "foot", "feet", "yd", "yard", "yards", "mi", "mile", "miles",
        "g", "gram", "grams", "kg", "kilogram", "kilograms", "mg", "milligram", "milligrams",
        "t", "ton", "tons", "tonne", "tonnes", "oz", "ounce", "ounces", "lb", "lbs", "pound", "pounds",
        "b", "byte", "bytes", "kb", "kib", "mb", "mib", "gb", "gib", "tb", "tib",
        "s", "sec", "secs", "second", "seconds", "ms", "millisecond", "milliseconds",
        "min", "mins", "minute", "minutes", "h", "hr", "hrs", "hour", "hours", "d", "day", "days",
        "w", "wk", "week", "weeks", "c", "celsius", "°c", "f", "fahrenheit", "°f", "k", "kelvin",
    };

    /// `10 km to mi`, `5km in m`, `32 °F to °C`: a number, a unit the engine
    /// knows, a word that joins them, another unit.
    private static FieldQuery? Conversion(string typed, string text)
    {
        foreach (var separator in Separators)
        {
            var at = text.IndexOf(separator, StringComparison.OrdinalIgnoreCase);
            if (at <= 0) continue;
            var left = text.AsSpan(0, at).Trim();
            var right = text.AsSpan(at + separator.Length).Trim();
            var number = 0;
            while (number < left.Length && (char.IsAsciiDigit(left[number]) || left[number] is '.' or '-' or ' ')) number++;
            if (number == 0 || number == left.Length) return null;
            var digits = left[..number].Trim();
            if (digits.Length == 0 || !char.IsAsciiDigit(digits[^1])) return null;
            if (!IsUnit(left[number..]) || !IsUnit(right)) return null;
            return new(typed, QueryKind.Conversion, text);
        }
        return null;
    }

    private static bool IsUnit(ReadOnlySpan<char> unit)
    {
        var trimmed = unit.Trim();
        return trimmed.Length is > 0 and <= 16 && Units.Contains(trimmed.ToString());
    }

    /// `23*47`, `(2+3)^2`, `15% of 240`, `240 + 15%`. The engine evaluates;
    /// this only makes sure it's arithmetic, and not a date, a phone number
    /// or an expression still being typed ("23*").
    private static FieldQuery? Calculation(string typed, string text)
    {
        var operators = 0;
        var digits = 0;
        var words = false;
        var i = 0;
        while (i < text.Length)
        {
            var c = text[i];
            if (char.IsAsciiDigit(c)) { digits++; i++; continue; }
            if (c is '.' or ' ' or '(' or ')') { i++; continue; }
            if (c is '+' or '*' or '/' or '×' or '÷' or '^' or '%') { operators++; i++; continue; }
            if (c == '-') { if (i > 0) operators++; i++; continue; }
            if (!char.IsAsciiLetter(c)) return null;
            // The only words are the engine's percentages: "15% of 240",
            // "15 percent of 240", "240 plus 15%", "240 minus 15%".
            var end = i;
            while (end < text.Length && char.IsAsciiLetter(text[end])) end++;
            var word = text.AsSpan(i, end - i);
            if (!(word.Equals("of", StringComparison.OrdinalIgnoreCase) || word.Equals("percent", StringComparison.OrdinalIgnoreCase)
                || word.Equals("plus", StringComparison.OrdinalIgnoreCase) || word.Equals("minus", StringComparison.OrdinalIgnoreCase)))
                return null;
            words = true;
            operators++;
            i = end;
        }
        if (digits == 0 || operators == 0) return null;
        if (words && !text.Contains('%') && !text.Contains("percent", StringComparison.OrdinalIgnoreCase)) return null;
        var last = text[^1];
        if (!char.IsAsciiDigit(last) && last != ')' && last != '%') return null;
        if (LooksLikeDateOrNumber(text)) return null;
        return new(typed, QueryKind.Calculation, text);
    }

    /// `2024-01-05`, `12/31/2024`, `555-1234`: digits joined by one kind of
    /// mark and nothing else are a date, a phone number or an id, not a sum.
    private static bool LooksLikeDateOrNumber(string text)
    {
        char? mark = null;
        var groups = 1;
        var shortest = int.MaxValue;
        var run = 0;
        foreach (var c in text)
        {
            if (char.IsAsciiDigit(c)) { run++; continue; }
            if (c != '-' && c != '/') return false;
            if (mark != null && mark != c) return false;
            mark = c;
            groups++;
            shortest = Math.Min(shortest, run);
            run = 0;
        }
        shortest = Math.Min(shortest, run);
        if (mark == null) return false;
        return groups >= 3 || (mark == '-' && shortest >= 3);
    }

    // MARK: - addresses

    /// Whether the text can only be a place. The same shape the browser's
    /// Address.Url accepts: a scheme, `about:`/`data:`, localhost, four
    /// numbers, or dotted labels ending in letters. "hello world" and "todo"
    /// aren't places, and neither is "1.5".
    public static bool LooksLikeAddress(string text)
    {
        if (text.Length == 0 || text.Contains(' ')) return false;
        var scheme = text.IndexOf("://", StringComparison.Ordinal);
        if (scheme > 0)
        {
            foreach (var c in text.AsSpan(0, scheme)) if (!char.IsAsciiLetter(c) && c != '+' && c != '-' && c != '.') return false;
            return true;
        }
        if (text.StartsWith("about:", StringComparison.OrdinalIgnoreCase) || text.StartsWith("data:", StringComparison.OrdinalIgnoreCase))
            return true;

        var end = text.AsSpan().IndexOfAny('/', '?', '#');
        var head = end < 0 ? text.AsSpan() : text.AsSpan(0, end);
        if (head.Contains('@')) return false;
        var colon = head.IndexOf(':');
        var host = colon < 0 ? head : head[..colon];
        if (host.Equals("localhost", StringComparison.OrdinalIgnoreCase)) return true;

        var labels = 0;
        var numeric = true;
        var start = 0;
        for (var i = 0; i <= host.Length; i++)
        {
            if (i < host.Length && host[i] != '.') continue;
            var label = host[start..i];
            if (label.Length == 0 || label[0] == '-' || label[^1] == '-') return false;
            foreach (var c in label) if (!char.IsLetterOrDigit(c) && c != '-') return false;
            if (!byte.TryParse(label, out _)) numeric = false;
            labels++;
            start = i + 1;
        }
        if (labels == 4 && numeric) return true;
        if (labels < 2) return false;
        var tld = host[(host.LastIndexOf('.') + 1)..];
        if (tld.Length < 2) return false;
        foreach (var c in tld) if (!char.IsLetter(c)) return false;
        return true;
    }

    private static (string Word, string After) Split(string text)
    {
        var space = text.IndexOf(' ');
        return space < 0 ? (text, "") : (text[..space], text[(space + 1)..].Trim());
    }
}
