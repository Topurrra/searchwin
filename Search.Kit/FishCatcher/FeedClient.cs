using System.Globalization;
using System.Net;
using System.Net.Http.Headers;
using System.Text.Json;

namespace SearchKit.FishCatcher;

/// What a refresh did.
public enum FeedOutcome
{
    /// The last download is under a day old; the network wasn't touched.
    Fresh,
    /// The registry says nothing changed (304).
    NotModified,
    /// A new, verified feed was saved.
    Updated,
    /// Download, JSON or signature failed; the last good feed stays in force.
    Failed,
}

public sealed record FeedResult(FeedOutcome Outcome, FeedBundle? Bundle = null, long? Count = null, DateTimeOffset? UpdatedAt = null);

/// Downloads the registry's daily signed feed, like the extension's
/// loadRemoteLists: only downloads, never uploads anything; ETag-cached; the
/// network is skipped while the last download is under a day old; a feed that
/// isn't signed by the registry key, or is older than the one saved, is
/// refused; and any failure keeps the
/// last good feed (fail open). The last good feed is kept on disk in `folder`
/// and read back on the next start.
///
/// The HttpClient and the folder come from the caller, so tests run without
/// a network and the browser decides when (and whether) this runs: in the
/// extension it was opt-in.
public sealed class FeedClient
{
    /// The FishCatcher registry, the feed's single fixed source.
    public const string DefaultUrl = "https://raw.githubusercontent.com/topurrra/fishcatcher-registry/main/fishcatcher-lists.json";

    public static readonly TimeSpan RefreshEvery = TimeSpan.FromDays(1);

    /// A feed is a Bloom filter of about 100k domains; anything far bigger isn't one.
    public const long MaxBytes = 16 * 1024 * 1024;

    private const string FeedFile = "fishcatcher-feed.json";
    private const string StateFile = "fishcatcher-feed-state.json";

    private readonly HttpClient http;
    private readonly string folder;
    private readonly string spki;
    private readonly string url;
    private readonly Func<DateTimeOffset> now;

    public FeedClient(HttpClient http, string folder, string spki, string url = DefaultUrl, Func<DateTimeOffset>? clock = null)
    {
        this.http = http;
        this.folder = folder;
        this.spki = spki;
        this.url = url;
        now = clock ?? (() => DateTimeOffset.UtcNow);
    }

    /// The last good feed saved in the folder, checked again, or null.
    public FeedBundle? LoadSaved()
    {
        try
        {
            var path = Path.Combine(folder, FeedFile);
            return File.Exists(path) ? FeedBundle.Verify(File.ReadAllBytes(path), spki) : null;
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            return null;
        }
    }

    /// When the feed was last downloaded, and how many domains it had.
    public (DateTimeOffset? UpdatedAt, long? Count, string? ETag) State()
    {
        try
        {
            var path = Path.Combine(folder, StateFile);
            if (!File.Exists(path)) return (null, null, null);
            using var doc = JsonDocument.Parse(File.ReadAllBytes(path));
            var root = doc.RootElement;
            DateTimeOffset? updated = root.TryGetProperty("updatedAt", out var u) && u.ValueKind == JsonValueKind.Number && u.TryGetInt64(out var ms)
                ? DateTimeOffset.FromUnixTimeMilliseconds(ms) : null;
            long? count = root.TryGetProperty("count", out var c) && c.ValueKind == JsonValueKind.Number && c.TryGetInt64(out var n) ? n : null;
            string? etag = root.TryGetProperty("etag", out var e) && e.ValueKind == JsonValueKind.String ? e.GetString() : null;
            return (updated, count, etag);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or JsonException or ArgumentOutOfRangeException)
        {
            return (null, null, null);
        }
    }

    /// Downloads the feed if it's due (or `force`), verifies it and saves it.
    /// Never throws for network, server or data trouble: that's Failed.
    public async Task<FeedResult> RefreshAsync(bool force = false, CancellationToken cancel = default)
    {
        var (updatedAt, count, etag) = State();
        if (!force && updatedAt is { } last && now() - last < RefreshEvery)
            return new FeedResult(FeedOutcome.Fresh, null, count, updatedAt);

        try
        {
            using var request = new HttpRequestMessage(HttpMethod.Get, url);
            request.Headers.CacheControl = new CacheControlHeaderValue { NoStore = true };
            // Use the ETag only once a count was recorded; otherwise fetch in full
            // so the count and date get filled in (the extension self-heals the same way).
            if (etag is not null && count is not null && EntityTagHeaderValue.TryParse(etag, out var tag))
                request.Headers.IfNoneMatch.Add(tag);

            using var response = await http.SendAsync(request, HttpCompletionOption.ResponseHeadersRead, cancel).ConfigureAwait(false);
            if (response.StatusCode == HttpStatusCode.NotModified) return new FeedResult(FeedOutcome.NotModified, null, count, updatedAt);
            if (!response.IsSuccessStatusCode) return new FeedResult(FeedOutcome.Failed);
            if (response.Content.Headers.ContentLength > MaxBytes) return new FeedResult(FeedOutcome.Failed);

            var body = await ReadCapped(response.Content, cancel).ConfigureAwait(false);
            if (body is null) return new FeedResult(FeedOutcome.Failed);

            // Signed by the registry's key, or refused: the bundled lists stay in force.
            var bundle = FeedBundle.Verify(body, spki);
            if (bundle is null) return new FeedResult(FeedOutcome.Failed);
            // An older feed replayed is still signed; its date gives it away.
            // It would take back sites reported since, so the newer one stays.
            if (Older(bundle.Generated, SavedGenerated())) return new FeedResult(FeedOutcome.Failed);

            var stamp = now();
            Directory.CreateDirectory(folder);
            WriteAtomic(Path.Combine(folder, FeedFile), body);
            WriteAtomic(Path.Combine(folder, StateFile), StateJson(stamp, bundle.Count, response.Headers.ETag?.ToString(), bundle.Generated));
            return new FeedResult(FeedOutcome.Updated, bundle, bundle.Count, stamp);
        }
        catch (Exception e) when (e is HttpRequestException or IOException or UnauthorizedAccessException or TaskCanceledException or InvalidOperationException)
        {
            if (cancel.IsCancellationRequested) throw;
            return new FeedResult(FeedOutcome.Failed);
        }
    }

    /// Forgets the download (the extension did this when the feed was turned off).
    public void Forget()
    {
        foreach (var name in (string[])[FeedFile, StateFile])
        {
            try { File.Delete(Path.Combine(folder, name)); }
            catch (Exception e) when (e is IOException or UnauthorizedAccessException) { }
        }
    }

    private static async Task<byte[]?> ReadCapped(HttpContent content, CancellationToken cancel)
    {
        await using var stream = await content.ReadAsStreamAsync(cancel).ConfigureAwait(false);
        using var buffer = new MemoryStream();
        var chunk = new byte[81920];
        int read;
        while ((read = await stream.ReadAsync(chunk, cancel).ConfigureAwait(false)) > 0)
        {
            buffer.Write(chunk, 0, read);
            if (buffer.Length > MaxBytes) return null;
        }
        return buffer.ToArray();
    }

    /// The signed date of the feed in force: from the state file, or from
    /// the saved feed itself (state written before the date was kept).
    private string? SavedGenerated()
    {
        try
        {
            var path = Path.Combine(folder, StateFile);
            if (File.Exists(path))
            {
                using var doc = JsonDocument.Parse(File.ReadAllBytes(path));
                if (doc.RootElement.TryGetProperty("generated", out var g) && g.ValueKind == JsonValueKind.String) return g.GetString();
            }
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException or JsonException) { }
        return LoadSaved()?.Generated;
    }

    /// Both dates read, and the candidate is the earlier.
    internal static bool Older(string? candidate, string? saved) =>
        Date(candidate) is { } a && Date(saved) is { } b && a < b;

    private static DateTimeOffset? Date(string? text) =>
        DateTimeOffset.TryParse(text, CultureInfo.InvariantCulture, DateTimeStyles.AssumeUniversal, out var when) ? when : null;

    private static byte[] StateJson(DateTimeOffset updatedAt, long? count, string? etag, string? generated)
    {
        using var buffer = new MemoryStream();
        using (var w = new Utf8JsonWriter(buffer))
        {
            w.WriteStartObject();
            w.WriteNumber("updatedAt", updatedAt.ToUnixTimeMilliseconds());
            if (count is { } n) w.WriteNumber("count", n);
            if (etag is not null) w.WriteString("etag", etag);
            if (generated is not null) w.WriteString("generated", generated);
            w.WriteEndObject();
        }
        return buffer.ToArray();
    }

    // A crash halfway through leaves the old file, never half a new one.
    private static void WriteAtomic(string path, byte[] bytes)
    {
        var temp = path + ".tmp";
        File.WriteAllBytes(temp, bytes);
        File.Move(temp, path, overwrite: true);
    }
}
