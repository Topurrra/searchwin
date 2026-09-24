using System.Buffers.Binary;

namespace SearchKit.FishCatcher;

/// The on-device address model (S17). Port of ml.js: a logistic regression
/// over hashed character n-grams of "^host$" plus a few shape tokens. Each
/// token is hashed with FNV-1a into one of `Buckets` int8 weights; a bucket
/// counts once however many tokens land in it. Tokens are hashed straight
/// from spans, so a prediction allocates almost nothing.
public sealed class MlModel
{
    public int Version { get; }
    public uint Buckets { get; }
    public int[] Ngrams { get; }
    public double Scale { get; }
    public double Bias { get; }
    public double Threshold { get; }
    private readonly sbyte[] table;

    public MlModel(int version, uint buckets, int[] ngrams, double scale, double bias, double threshold, sbyte[] table)
    {
        Version = version;
        Buckets = buckets;
        Ngrams = ngrams;
        Scale = scale;
        Bias = bias;
        Threshold = threshold;
        this.table = table;
    }

    /// The converted resource (see parity/convert-data.mjs).
    public static MlModel FromBinary(byte[] file)
    {
        if (file.Length < 16 || !file.AsSpan(0, 4).SequenceEqual("FCML"u8)) throw new InvalidDataException("not a FishCatcher model");
        int at = 4;
        uint U32() { uint v = BinaryPrimitives.ReadUInt32LittleEndian(file.AsSpan(at)); at += 4; return v; }
        double F64() { double v = BinaryPrimitives.ReadDoubleLittleEndian(file.AsSpan(at)); at += 8; return v; }
        int version = (int)U32();
        uint buckets = U32();
        var ngrams = new int[U32()];
        for (int i = 0; i < ngrams.Length; i++) ngrams[i] = (int)U32();
        double scale = F64(), bias = F64(), threshold = F64();
        int length = (int)U32();
        var table = new sbyte[length];
        file.AsSpan(at, length).CopyTo(System.Runtime.InteropServices.MemoryMarshal.AsBytes(table.AsSpan()));
        return new MlModel(version, buckets, ngrams, scale, bias, threshold, table);
    }

    /// Lower case, one trailing dot and a leading "www." dropped.
    public static string NormHost(string host)
    {
        var h = host.ToLowerInvariant();
        if (h.EndsWith('.')) h = h[..^1];
        return h.StartsWith("www.", StringComparison.Ordinal) ? h[4..] : h;
    }

    /// The feature tokens, as ml.js mlTokens makes them (for tests and the panel).
    public List<string> Tokens(string host)
    {
        var tokens = new List<string>();
        ForEachToken(host, t => tokens.Add(t.ToString()));
        return tokens;
    }

    private delegate void TokenSink(ReadOnlySpan<char> token);

    private void ForEachToken(string host, TokenSink sink)
    {
        string h = NormHost(host);
        string s = "^" + h + "$";
        foreach (int n in Ngrams)
            for (int i = 0; i + n <= s.Length; i++) sink(s.AsSpan(i, n));

        int labels = h.AsSpan().Count('.') + 1;
        int len = h.Length == 0 ? 1 : h.Length;
        int digits = 0, hyphens = 0, vowels = 0, run = 0, maxRun = 0;
        foreach (char c in h)
        {
            if (c is >= '0' and <= '9') digits++;
            else if (c == '-') hyphens++;
            else if ("aeiou".Contains(c)) vowels++;
            // consonant run: random letter soup (DGA) piles consonants up, words do not
            run = c is >= 'a' and <= 'z' && !"aeiouy".Contains(c) ? run + 1 : 0;
            if (run > maxRun) maxRun = run;
        }
        int dig = Math.Min(5, JsRound((double)digits / len * 10));
        int vow = Math.Min(5, JsRound((double)vowels / len * 10));
        int runB = Math.Min(7, maxRun);
        int lastDot = h.LastIndexOf('.');
        sink("len:" + Math.Min(12, h.Length >> 2));
        sink("dig:" + dig);
        sink("vow:" + vow);
        sink("run:" + runB);
        sink("shp:" + runB + ":" + vow + ":" + dig); // one conjunction so a linear model can see "letter soup"
        sink("hy:" + Math.Min(4, hyphens));
        sink("lab:" + Math.Min(5, labels));
        sink("tld:" + h[(lastDot + 1)..]);
    }

    // Math.round: halves go up (C#'s Math.Round would go to even).
    private static int JsRound(double x) => (int)Math.Floor(x + 0.5);

    /// Probability that the host looks like a phishing-feed address (0..1).
    public double Predict(string host)
    {
        if (Version != 2 || table.Length == 0) return 0; // old or missing weights: never fire
        var seen = new HashSet<uint>();
        long sum = 0;
        bool missing = false;
        ForEachToken(host, t =>
        {
            uint id = Bloom.Fnv1a(t) % Buckets;
            if (!seen.Add(id)) return; // binary presence
            if (id < table.Length) sum += table[id];
            else missing = true; // w[id] is undefined in JS: the sum turns NaN
        });
        if (missing) return double.NaN;
        double z = Bias + Scale * sum;
        return 1 / (1 + Math.Exp(-z));
    }
}
