using System.Buffers.Binary;
using System.IO.Compression;
using System.Security.Cryptography;
using System.Text;
using System.Text.RegularExpressions;

namespace Search;

// Chrome extensions, straight from the Chrome Web Store.
//
// An extension there is a .crx: a zip with a signed header in front of it.
// It is fetched from the same public update address every Chromium browser
// asks, the header is checked, and the zip is unpacked into the app's own
// folder for WebView2 to load.
//
// The check that matters: a Chrome extension's id is not a name anybody
// chose — it is the first sixteen bytes of the SHA-256 of its public key,
// written as letters a to p. So the header must carry a public key that
// hashes to the id that was asked for, and a signature by that key over the
// zip that came with it. A file that was altered on the way, or an
// extension passed off under another's id, fails one or the other.
public static class Crx
{
    public sealed partial class Refused(string why) : Exception(why)
    {
        public static Refused NotAnID => new("That isn't a Chrome Web Store link or extension id");
        public static Refused Download(int code) => new($"The Chrome Web Store answered {code}");
        public static Refused Empty => new("The Chrome Web Store has nothing for that id — it may have been taken down, or only exist for old versions of Chrome");
        public static Refused NotCrx => new("What came back isn't a Chrome extension");
        public static Refused UnsignedOrWrong => new("The extension's signature doesn't hold up");
        public static Refused Unpack => new("The extension couldn't be unpacked");
    }

    /// The version of Chrome the store is told it is talking to: the engine's
    /// own, whose major version is Chromium's. Some extensions set a minimum;
    /// the store refuses to hand those to a browser that says it is older.
    public static string ChromeVersion
    {
        get
        {
            try
            {
                var found = Microsoft.Web.WebView2.Core.CoreWebView2Environment.GetAvailableBrowserVersionString();
                var match = Regex.Match(found ?? "", @"^\d+(\.\d+){3}");
                if (match.Success) return match.Value;
            }
            catch { }
            return "140.0.0.0";
        }
    }

    private static readonly Regex Letters = new("(?<![a-z])([a-p]{32})(?![a-z])", RegexOptions.Compiled);

    /// Thirty-two letters from a to p, wherever they are — a bare id, a store
    /// link, an old chrome.google.com/webstore link.
    public static string? Id(string? text)
    {
        if (string.IsNullOrEmpty(text)) return null;
        var match = Letters.Match(text.ToLowerInvariant());
        return match.Success ? match.Groups[1].Value : null;
    }

    public static Uri DownloadUrl(string id) => new(
        "https://clients2.google.com/service/update2/crx?response=redirect" +
        $"&prodversion={ChromeVersion}&acceptformat=crx2,crx3" +
        $"&x={Uri.EscapeDataString($"id={id}&installsource=ondemand&uc")}");

    public static Uri UpdateCheckUrl(string id, string version) => new(
        "https://clients2.google.com/service/update2/crx?response=updatecheck" +
        $"&prodversion={ChromeVersion}&acceptformat=crx2,crx3" +
        $"&x={Uri.EscapeDataString($"id={id}&v={version}&uc")}");

    public static readonly HttpClient Http = new() { Timeout = TimeSpan.FromSeconds(60) };

    public static async Task<byte[]> Fetch(string id)
    {
        using var response = await Http.GetAsync(DownloadUrl(id));
        if (!response.IsSuccessStatusCode) throw Refused.Download((int)response.StatusCode);
        var data = await response.Content.ReadAsByteArrayAsync();
        if (data.Length == 0) throw Refused.Empty;
        return data;
    }

    /// The zip inside a CRX3, once its signature has been checked against
    /// `id` — and the public key it was signed with, which the unpacked
    /// manifest is given so the engine files it under the same id.
    public static (byte[] Zip, byte[] Key) VerifiedZip(byte[] crx, string id)
    {
        if (crx.Length <= 12 || crx[0] != 'C' || crx[1] != 'r' || crx[2] != '2' || crx[3] != '4') throw Refused.NotCrx;
        var version = BinaryPrimitives.ReadUInt32LittleEndian(crx.AsSpan(4));
        if (version != 3) throw Refused.NotCrx;
        var headerSize = (long)BinaryPrimitives.ReadUInt32LittleEndian(crx.AsSpan(8));
        if (12 + headerSize > crx.Length) throw Refused.NotCrx;
        var header = crx.AsSpan(12, (int)headerSize).ToArray();
        var zip = crx.AsSpan(12 + (int)headerSize).ToArray();

        // CrxFileHeader: 2 = sha256_with_rsa proofs, 3 = sha256_with_ecdsa
        // proofs, 10000 = signed_header_data (SignedData: 1 = crx_id).
        var fields = Protobuf(header);
        var signedHeader = fields.FirstOrDefault(f => f.Field == 10000).Bytes ?? throw Refused.UnsignedOrWrong;
        var crxID = Protobuf(signedHeader).FirstOrDefault(f => f.Field == 1).Bytes;
        if (crxID == null || IdOf(crxID) != id) throw Refused.UnsignedOrWrong;

        // What is signed: a fixed prefix, the signed header, and the zip.
        var prefix = Encoding.ASCII.GetBytes("CRX3 SignedData\0");
        var message = new byte[prefix.Length + 4 + signedHeader.Length + zip.Length];
        prefix.CopyTo(message, 0);
        BinaryPrimitives.WriteUInt32LittleEndian(message.AsSpan(prefix.Length), (uint)signedHeader.Length);
        signedHeader.CopyTo(message, prefix.Length + 4);
        zip.CopyTo(message, prefix.Length + 4 + signedHeader.Length);

        // One proof whose key is the one the id is made from, and whose
        // signature holds over the message. The store adds a proof of its
        // own; that one is fine to hold too but is not the one that counts.
        foreach (var (field, bytes) in fields)
        {
            if (field is not (2 or 3)) continue;
            var proof = Protobuf(bytes);
            var key = proof.FirstOrDefault(f => f.Field == 1).Bytes;
            var signature = proof.FirstOrDefault(f => f.Field == 2).Bytes;
            if (key == null || signature == null) continue;
            if (IdOf(SHA256.HashData(key).AsSpan(0, 16).ToArray()) != id) continue;
            if (Verify(field == 3, key, signature, message)) return (zip, key);
        }
        throw Refused.UnsignedOrWrong;
    }

    /// The id a public key (SubjectPublicKeyInfo DER) gives an extension.
    public static string IdOfKey(byte[] key) => IdOf(SHA256.HashData(key).AsSpan(0, 16).ToArray());

    /// Unpacks a zip into `folder`, which is replaced whole.
    public static void Unpack(byte[] zip, string folder)
    {
        var scratch = Path.Combine(Path.GetTempPath(), "search-crx-" + Guid.NewGuid().ToString("N"));
        try
        {
            using (var stream = new MemoryStream(zip, writable: false))
                // The archive's own paths are kept inside the folder: an entry
                // that climbs out of it is refused by ExtractToDirectory.
                ZipFile.ExtractToDirectory(stream, scratch, overwriteFiles: true);
            if (!File.Exists(Path.Combine(scratch, "manifest.json"))) throw Refused.Unpack;
            if (Directory.Exists(folder)) Directory.Delete(folder, recursive: true);
            Directory.CreateDirectory(Path.GetDirectoryName(folder)!);
            Directory.Move(scratch, folder);
        }
        catch (Refused) { throw; }
        catch { throw Refused.Unpack; }
        finally
        {
            try { if (Directory.Exists(scratch)) Directory.Delete(scratch, recursive: true); } catch { }
        }
    }

    // MARK: - pieces

    /// The id's letters: each half-byte is a letter from a (0) to p (15).
    public static string IdOf(byte[] bytes)
    {
        var text = new StringBuilder(bytes.Length * 2);
        foreach (var b in bytes)
        {
            text.Append((char)('a' + (b >> 4)));
            text.Append((char)('a' + (b & 0x0f)));
        }
        return text.ToString();
    }

    /// Length-delimited fields only, which is all a CRX header holds.
    private static List<(int Field, byte[] Bytes)> Protobuf(byte[] b)
    {
        var output = new List<(int, byte[])>();
        var i = 0;
        long? Varint()
        {
            long value = 0;
            var shift = 0;
            while (i < b.Length)
            {
                long next = b[i++];
                value |= (next & 0x7f) << shift;
                if ((next & 0x80) == 0) return value;
                shift += 7;
                if (shift > 56) return null;
            }
            return null;
        }
        while (i < b.Length)
        {
            if (Varint() is not { } key) break;
            var field = (int)(key >> 3);
            switch (key & 7)
            {
                case 2:
                    if (Varint() is not { } length || length < 0 || i + length > b.Length) return output;
                    output.Add((field, b.AsSpan(i, (int)length).ToArray()));
                    i += (int)length;
                    break;
                case 0: Varint(); break;
                case 1: i += 8; break;
                case 5: i += 4; break;
                default: return output;
            }
        }
        return output;
    }

    /// A public key as Chrome writes it (SubjectPublicKeyInfo DER), checked
    /// against a SHA-256 signature: PKCS#1 v1.5 for RSA, DER for ECDSA.
    private static bool Verify(bool ecdsa, byte[] spki, byte[] signature, byte[] message)
    {
        try
        {
            if (ecdsa)
            {
                using var ec = ECDsa.Create();
                ec.ImportSubjectPublicKeyInfo(spki, out _);
                return ec.VerifyData(message, signature, HashAlgorithmName.SHA256, DSASignatureFormat.Rfc3279DerSequence);
            }
            using var rsa = RSA.Create();
            rsa.ImportSubjectPublicKeyInfo(spki, out _);
            return rsa.VerifyData(message, signature, HashAlgorithmName.SHA256, RSASignaturePadding.Pkcs1);
        }
        catch { return false; }
    }
}
