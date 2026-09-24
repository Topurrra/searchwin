using System.Security.Cryptography;
using System.Text;
using System.Text.Json;

namespace SearchKit.FishCatcher;

/// The daily community feed from the FishCatcher registry, once its signature
/// has been checked. Port of remote.js.
///
/// The registry signs, with ECDSA P-256 / SHA-256 (signature as raw r||s,
/// base64 in `sig`), the JSON.stringify of exactly these fields in this order:
/// version, generated, sources, count, bloom. Only the Bloom filter and the
/// blocklist are used; nothing in a feed can widen the safe list or the brands.
public sealed class FeedBundle
{
    /// The version field as JSON text.
    public string? Version { get; private init; }
    public string? Generated { get; private init; }
    public long? Count { get; private init; }
    public Bloom? Bloom { get; private init; }
    public IReadOnlyList<string>? BlockList { get; private init; }

    /// Parses and verifies a downloaded feed. Null when it isn't JSON, isn't
    /// signed, or the signature doesn't match `spki`: the caller then keeps
    /// what it had (fail open, never fail closed).
    public static FeedBundle? Verify(ReadOnlySpan<byte> json, string spki)
    {
        try
        {
            using var doc = JsonDocument.Parse(json.ToArray());
            var root = doc.RootElement;
            if (root.ValueKind != JsonValueKind.Object || !VerifySignature(root, spki)) return null;
            return From(root);
        }
        catch (Exception e) when (e is JsonException or FormatException or CryptographicException or InvalidOperationException or ArgumentException)
        {
            return null;
        }
    }

    /// The exact bytes the registry signs (remote.js bundlePayload).
    public static string Payload(JsonElement bundle)
    {
        var sb = new StringBuilder("{");
        bool first = true;
        foreach (var name in (string[])["version", "generated", "sources", "count", "bloom"])
        {
            // JSON.stringify leaves out a property whose value is undefined.
            if (!bundle.TryGetProperty(name, out var value)) continue;
            if (!first) sb.Append(',');
            first = false;
            JsJson.Quote(sb, name);
            sb.Append(':');
            JsJson.Write(sb, value);
        }
        return sb.Append('}').ToString();
    }

    public static bool VerifySignature(JsonElement bundle, string spki)
    {
        if (bundle.ValueKind != JsonValueKind.Object || string.IsNullOrEmpty(spki)) return false;
        if (!bundle.TryGetProperty("sig", out var sig) || sig.ValueKind != JsonValueKind.String) return false;
        try
        {
            using var key = ECDsa.Create();
            key.ImportSubjectPublicKeyInfo(ForgivingBase64(spki), out _);
            if (key.KeySize != 256) return false; // WebCrypto imported it as P-256
            var data = Encoding.UTF8.GetBytes(Payload(bundle));
            return key.VerifyData(data, ForgivingBase64(sig.GetString()!), HashAlgorithmName.SHA256, DSASignatureFormat.IeeeP1363FixedFieldConcatenation);
        }
        catch (Exception e) when (e is CryptographicException or FormatException or JsonException or InvalidOperationException or ArgumentException)
        {
            return false;
        }
    }

    /// What applyBundle takes from a verified bundle.
    public static FeedBundle From(JsonElement root)
    {
        Bloom? bloom = null;
        if (root.TryGetProperty("bloom", out var b) && b.ValueKind == JsonValueKind.Object &&
            b.TryGetProperty("m", out var m) && U32(m, out var mv) && mv > 0 &&
            b.TryGetProperty("bits", out var bits) && bits.ValueKind == JsonValueKind.String && bits.GetString()!.Length > 0)
        {
            uint k = b.TryGetProperty("k", out var kv) && U32(kv, out var kk) ? kk : 0;
            uint seed = b.TryGetProperty("seed", out var sv) && U32(sv, out var ss) ? ss : 1;
            bloom = new Bloom(mv, k, seed, ForgivingBase64(bits.GetString()!));
        }
        List<string>? list = null;
        if (root.TryGetProperty("blocklist", out var bl) && bl.ValueKind == JsonValueKind.Array)
        {
            list = [];
            foreach (var x in bl.EnumerateArray())
                if (x.ValueKind == JsonValueKind.String) list.Add(x.GetString()!);
        }
        return new FeedBundle
        {
            Version = root.TryGetProperty("version", out var v) ? v.GetRawText() : null,
            Generated = root.TryGetProperty("generated", out var g) && g.ValueKind == JsonValueKind.String ? g.GetString() : null,
            Count = root.TryGetProperty("count", out var c) && c.ValueKind == JsonValueKind.Number && c.TryGetInt64(out var cv) ? cv : null,
            Bloom = bloom,
            BlockList = list,
        };
    }

    private static bool U32(JsonElement e, out uint value)
    {
        value = 0;
        return e.ValueKind == JsonValueKind.Number && e.TryGetUInt32(out value);
    }

    /// atob: ASCII white space ignored, padding optional.
    internal static byte[] ForgivingBase64(string text)
    {
        var s = new StringBuilder(text.Length + 3);
        foreach (char c in text)
            if (c is not (' ' or '\t' or '\n' or '\f' or '\r')) s.Append(c);
        if (s.Length % 4 == 0 && s.Length > 0 && s[^1] == '=')
        {
            s.Length--;
            if (s.Length > 0 && s[^1] == '=') s.Length--;
        }
        if (s.Length % 4 == 1) throw new FormatException("bad base64");
        while (s.Length % 4 != 0) s.Append('=');
        return Convert.FromBase64String(s.ToString());
    }
}
