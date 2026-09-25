using System.Globalization;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json.Nodes;
using SearchKit.Engine;

namespace SearchKit.Field;

/// An engine call: the method and its parameters.
public sealed record EngineAsk(string Method, JsonObject Args);

/// Instant answers: `23*47`, `10 km to mi`, `base64 hello`, `json2yaml {…}`,
/// `sha256 hello`, `#e07a5f`. Enter copies the answer.
///
/// Which queries go to the engine, and with what, is `Plan`; turning its
/// answer into rows is `Shape`. Hashes of typed text and colours are worked
/// out here: the engine's `compute_hashes` hashes files, not text, and has no
/// colour command.
public static class Answers
{
    /// The engine call for this query, or null when the engine isn't asked
    /// (not an answer, or one worked out here).
    public static EngineAsk? Plan(FieldQuery query) => query.Kind switch
    {
        // Web search stays off: the field has its own bangs, and a `g foo`
        // must never come back as somebody else's search.
        QueryKind.Calculation or QueryKind.Conversion =>
            new("evaluate_quick_query", new JsonObject { ["query"] = query.Text, ["webSearchEnabled"] = false }),
        QueryKind.Encode when query.Tool != null && query.Operand != null =>
            new("encode_decode", new JsonObject { ["algorithm"] = query.Tool, ["mode"] = query.Mode ?? "encode", ["input"] = query.Operand }),
        QueryKind.Format when query.Tool != null && query.Mode != null && query.Operand != null =>
            new("convert_format", new JsonObject { ["from"] = query.Tool, ["to"] = query.Mode, ["input"] = query.Operand }),
        _ => null,
    };

    /// The engine's answer as rows. Only a calculator or unit-conversion
    /// answer counts from `evaluate_quick_query`: it also knows system
    /// commands ("lock", "shutdown"), addresses and bangs, and none of those
    /// may come back as something Enter would copy, let alone run.
    public static IReadOnlyList<FieldRow> Shape(FieldQuery query, JsonNode? answer)
    {
        switch (query.Kind)
        {
            case QueryKind.Calculation or QueryKind.Conversion:
                var type = Nodes.Str(answer, "type");
                var result = Nodes.Str(answer, "result");
                if (result.Length == 0) return [];
                return type switch
                {
                    "calculator" => [Row(result, Nodes.Str(answer, "expression") + " =")],
                    "unitConversion" => [Row(result, Nodes.Str(answer, "original") + " =")],
                    _ => [],
                };
            case QueryKind.Encode:
                if (answer is not JsonValue encoded || !encoded.TryGetValue<string>(out var text) || text.Length == 0) return [];
                return [Row(text, $"{Name(query.Tool)} {(query.Mode == "decode" ? "decoded" : "encoded")}")];
            case QueryKind.Format:
                if (answer is not JsonValue converted || !converted.TryGetValue<string>(out var output) || output.Length == 0) return [];
                return [Row(output, $"{Name(query.Tool)} → {Name(query.Mode)}")];
            default:
                return [];
        }
    }

    /// `sha256 hello`: the text's hash, in lower-case hex.
    public static IReadOnlyList<FieldRow> Hash(FieldQuery query)
    {
        if (query.Kind != QueryKind.Hash || query.Operand is not { Length: > 0 } operand) return [];
        var bytes = Encoding.UTF8.GetBytes(operand);
        var digest = query.Tool switch
        {
            "md5" => MD5.HashData(bytes),
            "sha1" => SHA1.HashData(bytes),
            "sha256" => SHA256.HashData(bytes),
            "sha384" => SHA384.HashData(bytes),
            "sha512" => SHA512.HashData(bytes),
            _ => null,
        };
        if (digest == null) return [];
        return [Row(Convert.ToHexStringLower(digest), $"{Name(query.Tool)} of “{Nodes.Line(operand, 40)}”")];
    }

    /// `#e07a5f` → rgb(224, 122, 95) and hsl(12, 67%, 63%); `… to hsl` puts
    /// that one first. The format typed isn't offered back.
    public static IReadOnlyList<FieldRow> Color(FieldQuery query)
    {
        if (query.Kind != QueryKind.Color || query.Operand == null || Parse(query.Tool, query.Operand) is not { } color)
            return [];
        var forms = new List<(string Format, string Text)>
        {
            ("hex", Hex(color)), ("rgb", Rgb(color)), ("hsl", Hsl(color)),
        };
        forms.RemoveAll(f => f.Format == query.Tool);
        if (query.Mode != null)
        {
            var wanted = forms.FindIndex(f => f.Format == query.Mode);
            if (wanted > 0) { var first = forms[wanted]; forms.RemoveAt(wanted); forms.Insert(0, first); }
        }
        return [.. forms.Select(f => Row(f.Text, $"{query.Operand} as {f.Format.ToUpperInvariant()}"))];
    }

    private static FieldRow Row(string answer, string detail) =>
        new(Group.Answer, RowKey.Answer(answer), Nodes.Line(answer, 160), detail, RowAction.Copy, answer);

    private static string Name(string? tool) => tool switch
    {
        "base64" => "Base64",
        "base64url" => "Base64 URL",
        "url" => "URL",
        "html" => "HTML",
        "hex" => "Hex",
        "binary" => "Binary",
        "rot13" => "ROT13",
        "json" => "JSON",
        "yaml" => "YAML",
        "toml" => "TOML",
        "xml" => "XML",
        "md5" => "MD5",
        "sha1" => "SHA-1",
        "sha256" => "SHA-256",
        "sha384" => "SHA-384",
        "sha512" => "SHA-512",
        _ => tool ?? "",
    };

    // MARK: - colours

    /// Red, green, blue 0–255 and alpha 0–1.
    private readonly record struct Rgba(int R, int G, int B, double A);

    private static Rgba? Parse(string? tool, string text)
    {
        if (tool == "hex")
        {
            var hex = text[1..];
            int Digit(int i) => Convert.ToInt32(hex[i].ToString(), 16);
            int Pair(int i) => Digit(i) * 16 + Digit(i + 1);
            return hex.Length switch
            {
                3 => new Rgba(Digit(0) * 17, Digit(1) * 17, Digit(2) * 17, 1),
                4 => new Rgba(Digit(0) * 17, Digit(1) * 17, Digit(2) * 17, Digit(3) * 17 / 255.0),
                6 => new Rgba(Pair(0), Pair(2), Pair(4), 1),
                8 => new Rgba(Pair(0), Pair(2), Pair(4), Pair(6) / 255.0),
                _ => null,
            };
        }
        var open = text.IndexOf('(');
        var close = text.IndexOf(')');
        if (open < 0 || close < open) return null;
        var parts = text[(open + 1)..close].Split([',', ' ', '/'], StringSplitOptions.RemoveEmptyEntries);
        if (parts.Length is < 3 or > 4) return null;
        var numbers = new double[parts.Length];
        for (var i = 0; i < parts.Length; i++)
        {
            var part = parts[i].TrimEnd('%');
            if (!double.TryParse(part, NumberStyles.Float, CultureInfo.InvariantCulture, out numbers[i])) return null;
            if (parts[i].EndsWith('%') && (tool == "rgb" || i == 3)) numbers[i] = i == 3 ? numbers[i] / 100 : numbers[i] * 2.55;
        }
        var alpha = parts.Length == 4 ? Math.Clamp(numbers[3], 0, 1) : 1;
        if (tool == "rgb")
            return new Rgba(Byte(numbers[0]), Byte(numbers[1]), Byte(numbers[2]), alpha);
        if (tool == "hsl")
        {
            var (r, g, b) = FromHsl(numbers[0], numbers[1] / 100, numbers[2] / 100);
            return new Rgba(r, g, b, alpha);
        }
        return null;
    }

    private static int Byte(double value) => (int)Math.Round(Math.Clamp(value, 0, 255));

    private static string Hex(Rgba c) =>
        c.A < 1 ? $"#{c.R:x2}{c.G:x2}{c.B:x2}{Byte(c.A * 255):x2}" : $"#{c.R:x2}{c.G:x2}{c.B:x2}";

    private static string Rgb(Rgba c) =>
        c.A < 1 ? $"rgba({c.R}, {c.G}, {c.B}, {Alpha(c.A)})" : $"rgb({c.R}, {c.G}, {c.B})";

    private static string Hsl(Rgba c)
    {
        var (h, s, l) = ToHsl(c.R, c.G, c.B);
        var body = string.Create(CultureInfo.InvariantCulture, $"{Math.Round(h)}, {Math.Round(s * 100)}%, {Math.Round(l * 100)}%");
        return c.A < 1 ? $"hsla({body}, {Alpha(c.A)})" : $"hsl({body})";
    }

    private static string Alpha(double a) => Math.Round(a, 2).ToString(CultureInfo.InvariantCulture);

    private static (double H, double S, double L) ToHsl(int red, int green, int blue)
    {
        double r = red / 255.0, g = green / 255.0, b = blue / 255.0;
        var max = Math.Max(r, Math.Max(g, b));
        var min = Math.Min(r, Math.Min(g, b));
        var l = (max + min) / 2;
        if (max == min) return (0, 0, l);
        var d = max - min;
        var s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
        var h = max == r ? (g - b) / d + (g < b ? 6 : 0) : max == g ? (b - r) / d + 2 : (r - g) / d + 4;
        return (h * 60 % 360, s, l);
    }

    private static (int R, int G, int B) FromHsl(double h, double s, double l)
    {
        h = ((h % 360) + 360) % 360 / 360;
        s = Math.Clamp(s, 0, 1);
        l = Math.Clamp(l, 0, 1);
        if (s == 0) { var grey = Byte(l * 255); return (grey, grey, grey); }
        var q = l < 0.5 ? l * (1 + s) : l + s - l * s;
        var p = 2 * l - q;
        double Channel(double t)
        {
            if (t < 0) t += 1;
            if (t > 1) t -= 1;
            if (t < 1.0 / 6) return p + (q - p) * 6 * t;
            if (t < 1.0 / 2) return q;
            if (t < 2.0 / 3) return p + (q - p) * (2.0 / 3 - t) * 6;
            return p;
        }
        return (Byte(Channel(h + 1.0 / 3) * 255), Byte(Channel(h) * 255), Byte(Channel(h - 1.0 / 3) * 255));
    }
}

/// Instant answers as a field source: the engine's when `Answers.Plan` says
/// so, the ones worked out here otherwise. An engine error ("Invalid
/// Base64") is simply no answer.
public sealed class AnswerSource(IEngineCalls engine) : IEngineSource
{
    public Group Group => Group.Answer;

    public bool Wants(FieldQuery query) => query.IsAnswer && query.Scope == Scope.All;

    public TimeSpan Delay(FieldQuery query) => TimeSpan.Zero;

    public async Task<IReadOnlyList<FieldRow>> SuggestAsync(FieldQuery query, int limit, CancellationToken cancel)
    {
        IReadOnlyList<FieldRow> rows;
        if (Answers.Plan(query) is { } ask)
        {
            try
            {
                rows = Answers.Shape(query, await engine.CallAsync(ask.Method, ask.Args, cancel).ConfigureAwait(false));
            }
            catch (EngineException)
            {
                return [];
            }
        }
        else
        {
            rows = query.Kind == QueryKind.Hash ? Answers.Hash(query) : Answers.Color(query);
        }
        return [.. rows.Take(limit)];
    }
}
