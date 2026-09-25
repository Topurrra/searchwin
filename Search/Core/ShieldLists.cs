using System.Net;
using System.Net.Http.Headers;
using System.Text.Json;
using SearchKit.Shields;

namespace Search;

/// EasyList and EasyPrivacy: downloaded from their publisher (easylist.to),
/// compiled into one FilterList, kept under Store.Folder\Shields, and
/// checked again once a day. Opt-in — they're downloads, the one network
/// call the blocker makes (Decisions D28) — and kept under Store.Folder,
/// so a test world has its own.
///
/// Nothing here runs before the first window: the saved list is read a
/// couple of seconds after it (a compiled blob, ~35 ms to load), and the
/// daily check comes later still, on a thread of its own at the lowest
/// priority. Fails open: a download that doesn't come, or doesn't look like
/// a filter list, leaves the last good one in force — or the built-in 44
/// domains, which are always there.
public static class ShieldLists
{
    private static string Folder => Path.Combine(Store.Folder, "Shields");
    private const string Blob = "lists.bin";
    private const string State = "lists.json";

    private static readonly TimeSpan Every = TimeSpan.FromDays(1);
    private static HttpClient? http;
    private static int running;
    private static int queuedForce;
    private static Microsoft.UI.Dispatching.DispatcherQueueTimer? hourly;

    /// Advanced by Forget, so a worker already in flight can tell it has
    /// been overtaken and undo whatever it wrote instead of quietly bringing
    /// back what Forget just deleted. Whether a list is loaded belongs to a
    /// turn too: one loaded in a turn Forget ended isn't.
    private static readonly ListGeneration turns = new();

    /// The saved list soon after the first window; the daily check a little
    /// after that, out of the first pages' way, and then every hour (it only
    /// goes to the network once a day).
    public static void Schedule()
    {
        UI.After(2, () => Refresh(force: false, download: false));
        UI.After(20, () =>
        {
            Refresh(force: false);
            hourly ??= UI.Every(3600, () => Refresh(force: false));
        });
    }

    /// Loads the saved list once, then downloads new ones if they're due (or
    /// `force`), compiles and saves them, and hands the result to Shield. A
    /// `force` that arrives while a worker is already running is not
    /// dropped: it is picked up the moment that one finishes.
    public static void Refresh(bool force, bool download = true)
    {
        if (Browser.Shared is not { Prefs.ShieldLists: true }) return;
        if (Interlocked.Exchange(ref running, 1) == 1)
        {
            if (force) Volatile.Write(ref queuedForce, 1);
            return;
        }
        var worker = new Thread(() =>
        {
            try { Run(force, download).GetAwaiter().GetResult(); }
            catch (Exception e) { Log.Write($"shield lists: {e.Message}"); }
            finally
            {
                Volatile.Write(ref running, 0);
                if (Interlocked.Exchange(ref queuedForce, 0) == 1) Refresh(force: true);
            }
        })
        {
            IsBackground = true,
            Priority = ThreadPriority.Lowest,
            Name = "Shield lists",
        };
        worker.Start();
    }

    private static async Task Run(bool force, bool download)
    {
        // Forget can run on the UI thread at any moment this worker is out
        // here on its own — between reading this and every write below —
        // and delete what it's about to write right back into existence.
        // So every write is checked again after it lands (Overtaken), and
        // Publish checks once more on the UI thread, where Forget runs.
        var gen = turns.Current;
        Directory.CreateDirectory(Folder);
        var state = Saved.Read(Path.Combine(Folder, State));
        if (!turns.Loaded && LoadSaved() is { } saved)
        {
            turns.MarkLoaded(gen);
            Publish(saved, gen);
        }
        // Forget may have run while the saved list was being read, and found
        // lists.bin held open: whatever it couldn't delete goes now.
        if (turns.Overtaken(gen, Cleanup)) return;
        if (!download) return;
        if (!force && turns.Loaded && DateTime.UtcNow - state.Checked < Every) return;

        var changed = false;
        foreach (var source in ListSource.BuiltIn)
        {
            if (Browser.Shared is not { Prefs.ShieldLists: true }) return;
            if (turns.Overtaken(gen, Cleanup)) return;
            var entry = state.Lists.GetValueOrDefault(source.Name) ?? new Saved.Entry();
            try
            {
                if (await Fetch(source, entry).ConfigureAwait(false)) changed = true;
                state.Lists[source.Name] = entry;
                if (turns.Overtaken(gen, Cleanup)) return;
            }
            catch (Exception e)
            {
                Log.Write($"shield lists: {source.Name} didn't come: {e.Message}");
            }
        }
        state.Checked = DateTime.UtcNow;

        if (turns.Overtaken(gen, Cleanup)) return;

        if (changed || !turns.Loaded)
        {
            var paths = ListSource.BuiltIn.Select(s => Path.Combine(Folder, FileOf(s))).Where(File.Exists).ToArray();
            if (paths.Length > 0)
            {
                var started = System.Diagnostics.Stopwatch.GetTimestamp();
                var list = FilterList.Compile(paths.Select(File.ReadLines));
                var took = System.Diagnostics.Stopwatch.GetElapsedTime(started).TotalMilliseconds;
                if (turns.Overtaken(gen, Cleanup)) return;
                Save(list);
                if (turns.Overtaken(gen, Cleanup)) return;
                state.Rules = Rules(list);
                turns.MarkLoaded(gen);
                Publish(list, gen);
                Log.Write($"shield lists: compiled {state.Rules} rules in {took:F0} ms");
            }
        }
        if (turns.Overtaken(gen, Cleanup) || Browser.Shared is not { Prefs.ShieldLists: true }) return;
        state.Write(Path.Combine(Folder, State));
        turns.Overtaken(gen, Cleanup);
    }

    /// One list, if it has changed since last time. The publisher is asked
    /// with the tag it gave last time, so an unchanged list costs a "304" —
    /// but only when the file the tag describes is still here. A tag left
    /// over after the file was deleted by hand, by antivirus, or by a race
    /// with Forget would otherwise draw a 304 forever, with nothing to serve
    /// it from and no way to ask for the file itself again.
    private static async Task<bool> Fetch(ListSource source, Saved.Entry entry)
    {
        http ??= Client();
        var path = Path.Combine(Folder, FileOf(source));
        var hasFile = File.Exists(path);
        using var request = new HttpRequestMessage(HttpMethod.Get, source.Url);
        if (hasFile && entry.ETag is { Length: > 0 } tag && EntityTagHeaderValue.TryParse(tag, out var etag))
            request.Headers.IfNoneMatch.Add(etag);
        if (hasFile && entry.Modified is { } modified) request.Headers.IfModifiedSince = modified;
        using var response = await http.SendAsync(request, HttpCompletionOption.ResponseHeadersRead).ConfigureAwait(false);
        if (response.StatusCode == HttpStatusCode.NotModified)
        {
            if (hasFile) return false;
            // Asked unconditionally (no file, so no condition was sent) and
            // still told "not modified": the tag lied. Drop it so the next
            // try, if this one fails too, asks with nothing to go stale.
            entry.ETag = null;
            entry.Modified = null;
            return false;
        }
        response.EnsureSuccessStatusCode();
        if (response.Content.Headers.ContentLength is > MaxBytes) throw new InvalidDataException("too big");
        // Read through a capped stream rather than buffering the whole body
        // first: with compression the server rarely sends a Content-Length,
        // so that check alone doesn't stop a large or decompression-bomb
        // response from being fully read into memory before anyone notices.
        var text = await ReadCapped(response.Content).ConfigureAwait(false);
        // What a filter list looks like: its header, and a good many lines.
        // Anything else — a captive portal's page, an error page served as
        // 200, or a response that ran past the cap — is not a list, and the
        // last good one stays.
        if (text == null || !text.StartsWith("[Adblock", StringComparison.Ordinal) || text.Count(c => c == '\n') < 1000)
            throw new InvalidDataException("not a filter list");
        var partial = path + ".part";
        await File.WriteAllTextAsync(partial, text).ConfigureAwait(false);
        File.Move(partial, path, overwrite: true);
        entry.ETag = response.Headers.ETag?.ToString();
        entry.Modified = response.Content.Headers.LastModified;
        entry.Version = Header(text, "! Version:");
        entry.Bytes = text.Length;
        return true;
    }

    private const int MaxBytes = 30 * 1024 * 1024;

    /// Reads `content` into text, aborting the moment it runs past
    /// `MaxBytes` instead of buffering the whole thing first — the same
    /// shape as FeedClient.ReadCapped, for the same reason.
    private static async Task<string?> ReadCapped(HttpContent content)
    {
        await using var stream = await content.ReadAsStreamAsync().ConfigureAwait(false);
        using var buffer = new MemoryStream();
        var chunk = new byte[81920];
        int read;
        while ((read = await stream.ReadAsync(chunk).ConfigureAwait(false)) > 0)
        {
            buffer.Write(chunk, 0, read);
            if (buffer.Length > MaxBytes) return null;
        }
        buffer.Position = 0;
        using var reader = new StreamReader(buffer);
        return await reader.ReadToEndAsync().ConfigureAwait(false);
    }

    private static HttpClient Client()
    {
        var handler = new SocketsHttpHandler { AutomaticDecompression = DecompressionMethods.All };
        var client = new HttpClient(handler) { Timeout = TimeSpan.FromSeconds(60) };
        client.DefaultRequestHeaders.UserAgent.Add(new ProductInfoHeaderValue("Search", Updater.Version));
        return client;
    }

    private static string FileOf(ListSource source) => source.Name.ToLowerInvariant() + ".txt";

    private static string? Header(string text, string key)
    {
        var at = text.IndexOf(key, StringComparison.Ordinal);
        if (at < 0 || at > 4096) return null;
        var end = text.IndexOf('\n', at);
        return text[(at + key.Length)..(end < 0 ? text.Length : end)].Trim();
    }

    private static int Rules(FilterList list) =>
        list.Stats.NetworkRules + list.Stats.NetworkExceptions + list.Stats.CosmeticRules + list.Stats.CosmeticExceptions;

    /// The compiled list from last time, without reading any list text.
    private static FilterList? LoadSaved()
    {
        var path = Path.Combine(Folder, Blob);
        if (!File.Exists(path)) return null;
        try
        {
            using var stream = new BufferedStream(File.OpenRead(path), 1 << 16);
            return FilterList.Load(stream);
        }
        catch (Exception e)
        {
            // An older format, or a torn file: compiled again from the text.
            Log.Write($"shield lists: saved list unreadable ({e.Message})");
            return null;
        }
    }

    private static void Save(FilterList list)
    {
        var path = Path.Combine(Folder, Blob);
        var partial = path + ".part";
        using (var stream = new BufferedStream(File.Create(partial), 1 << 16)) list.Save(stream);
        File.Move(partial, path, overwrite: true);
    }

    /// Into Shield, on the UI thread, with its 200 KB stylesheet already
    /// written out here — unless Forget has run since `gen` began. Checked
    /// on the UI thread, where Forget runs, so it can't change in between.
    private static void Publish(FilterList list, int gen)
    {
        var literal = Shield.Prepare(list);
        UI.Do(() =>
        {
            if (turns.Holds(gen) && Browser.Shared is { Prefs.ShieldLists: true }) Shield.Shared.Use(list, literal);
        });
    }

    /// Turned off: back to the built-in list, and the downloads deleted.
    public static void Forget()
    {
        // Advanced before the deletion itself: a worker already past this
        // point in a download checks it after its next write and, finding
        // itself overtaken, takes back out whatever it wrote rather than
        // silently undoing what Forget is about to do.
        turns.Advance();
        Shield.Shared.Use(null, null);
        Cleanup();
    }

    /// Every file this feature owns, gone. Shared by Forget and by a worker
    /// that finds Forget ran while it was still downloading.
    private static void Cleanup()
    {
        // One file at a time: one held open (a read still under way) must
        // not keep the others on disk.
        foreach (var name in (string[])[Blob, State, .. ListSource.BuiltIn.Select(FileOf)])
        {
            foreach (var file in (string[])[name, name + ".part"])
            {
                try { File.Delete(Path.Combine(Folder, file)); }
                catch { }
            }
        }
    }

    /// "EasyList and EasyPrivacy · 125,176 rules · updated 3 hr. ago", for
    /// Settings; null before the first download.
    public static string? Said()
    {
        var state = Saved.Read(Path.Combine(Folder, State));
        if (state.Checked == DateTime.MinValue) return null;
        var names = string.Join(" and ", ListSource.BuiltIn.Select(s => s.Name));
        return state.Rules > 0
            ? $"{names} · {state.Rules:N0} rules · checked {When.Said(state.Checked)}"
            : $"{names} · checked {When.Said(state.Checked)}";
    }

    /// lists.json: when the publisher was last asked, and what each list's
    /// tag and date were, so the next ask can be "only if it changed".
    private sealed class Saved
    {
        public DateTime Checked = DateTime.MinValue;
        public int Rules;
        public readonly Dictionary<string, Entry> Lists = new(StringComparer.Ordinal);

        public sealed class Entry
        {
            public string? ETag;
            public DateTimeOffset? Modified;
            public string? Version;
            public long Bytes;
        }

        public static Saved Read(string path)
        {
            var saved = new Saved();
            try
            {
                if (!File.Exists(path)) return saved;
                using var doc = JsonDocument.Parse(File.ReadAllBytes(path));
                var root = doc.RootElement;
                if (root.TryGetProperty("checked", out var c) && c.TryGetInt64(out var ms))
                    saved.Checked = DateTimeOffset.FromUnixTimeMilliseconds(ms).UtcDateTime;
                if (root.TryGetProperty("rules", out var r) && r.TryGetInt32(out var rules)) saved.Rules = rules;
                if (root.TryGetProperty("lists", out var lists) && lists.ValueKind == JsonValueKind.Object)
                    foreach (var list in lists.EnumerateObject())
                    {
                        var entry = new Entry();
                        if (list.Value.TryGetProperty("etag", out var e) && e.ValueKind == JsonValueKind.String) entry.ETag = e.GetString();
                        if (list.Value.TryGetProperty("modified", out var m) && m.TryGetInt64(out var mod))
                            entry.Modified = DateTimeOffset.FromUnixTimeMilliseconds(mod);
                        if (list.Value.TryGetProperty("version", out var v) && v.ValueKind == JsonValueKind.String) entry.Version = v.GetString();
                        if (list.Value.TryGetProperty("bytes", out var b) && b.TryGetInt64(out var bytes)) entry.Bytes = bytes;
                        saved.Lists[list.Name] = entry;
                    }
            }
            catch { }
            return saved;
        }

        public void Write(string path)
        {
            using var buffer = new MemoryStream();
            using (var w = new Utf8JsonWriter(buffer, new JsonWriterOptions { Indented = true }))
            {
                w.WriteStartObject();
                w.WriteNumber("checked", new DateTimeOffset(Checked).ToUnixTimeMilliseconds());
                w.WriteNumber("rules", Rules);
                w.WriteStartObject("lists");
                foreach (var (name, entry) in Lists)
                {
                    w.WriteStartObject(name);
                    if (entry.ETag != null) w.WriteString("etag", entry.ETag);
                    if (entry.Modified is { } m) w.WriteNumber("modified", m.ToUnixTimeMilliseconds());
                    if (entry.Version != null) w.WriteString("version", entry.Version);
                    w.WriteNumber("bytes", entry.Bytes);
                    w.WriteEndObject();
                }
                w.WriteEndObject();
                w.WriteEndObject();
            }
            File.WriteAllBytes(path, buffer.ToArray());
        }
    }
}
