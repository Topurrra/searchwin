using System.Buffers.Binary;

namespace SearchKit.FishCatcher;

/// A Bloom filter with FNV-1a double hashing. Port of bloom.js: the bundled
/// "known legitimate" list (top ~100k sites) and the community feed both use
/// it, and the feed's bits are made by the registry's JS, so the hashing has
/// to match bit for bit: FNV-1a over UTF-16 code units, unsigned 32-bit wrap.
public sealed class Bloom
{
    public uint M { get; }
    public uint K { get; }
    public uint Seed { get; }
    private readonly byte[] bits;

    public Bloom(uint m, uint k, uint seed, byte[] bits)
    {
        if (m == 0) throw new ArgumentOutOfRangeException(nameof(m));
        M = m;
        K = k;
        Seed = seed;
        // Bloom.fromPayload: exactly ceil(m/8) bytes, filled as far as the payload goes.
        var own = new byte[(m + 7) / 8];
        Array.Copy(bits, own, Math.Min(bits.Length, own.Length));
        this.bits = own;
    }

    public static uint Fnv1a(ReadOnlySpan<char> s, uint seed = 0x811c9dc5)
    {
        uint h = seed;
        foreach (char c in s)
        {
            h ^= c;
            h *= 0x01000193;
        }
        return h;
    }

    public bool Has(string s)
    {
        uint h1 = Fnv1a(s, 0x811c9dc5 ^ Seed);
        uint h2 = Fnv1a(s, 0x01000193 + Seed);
        for (uint i = 0; i < K; i++)
        {
            // (h1 + Math.imul(i, h2)) >>> 0, then % m: all mod 2^32.
            uint bit = (h1 + i * h2) % M;
            if ((bits[bit >> 3] & (1 << (int)(bit & 7))) == 0) return false;
        }
        return true;
    }

    public void Add(string s)
    {
        uint h1 = Fnv1a(s, 0x811c9dc5 ^ Seed);
        uint h2 = Fnv1a(s, 0x01000193 + Seed);
        for (uint i = 0; i < K; i++)
        {
            uint bit = (h1 + i * h2) % M;
            bits[bit >> 3] |= (byte)(1 << (int)(bit & 7));
        }
    }

    /// The feed's form: { m, k, seed?, bits: base64 }.
    public static Bloom FromPayload(uint m, uint k, uint seed, string base64) =>
        new(m, k, seed, Convert.FromBase64String(base64));

    /// The converted resource (see parity/convert-data.mjs):
    /// "FCBF", u32 version, u32 m, u32 k, u32 seed, u32 count, then the bits.
    public static Bloom FromBinary(ReadOnlySpan<byte> file)
    {
        if (file.Length < 24 || !file[..4].SequenceEqual("FCBF"u8) || BinaryPrimitives.ReadUInt32LittleEndian(file[4..]) != 1)
            throw new InvalidDataException("not a FishCatcher Bloom filter");
        uint m = BinaryPrimitives.ReadUInt32LittleEndian(file[8..]);
        uint k = BinaryPrimitives.ReadUInt32LittleEndian(file[12..]);
        uint seed = BinaryPrimitives.ReadUInt32LittleEndian(file[16..]);
        return new Bloom(m, k, seed, file[24..].ToArray());
    }
}
