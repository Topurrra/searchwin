using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;

namespace Search;

// Reading what another browser on this PC already holds: its bookmarks, the
// places it has been, and the icons it had for them.
//
// Every Chromium browser — Chrome, Edge, Brave, Vivaldi, Opera, Arc — keeps
// these the same way, in a folder per profile: a JSON file of bookmarks, and
// SQLite files for history and icons.
//
// Passwords are not read from here. They come over the way each browser
// offers to hand them over itself — its own export to a CSV, which the
// passwords panel takes in (see Vault.Take).
//
// Every database is copied before it is read. The browser it belongs to is
// usually running, and reading its live file underneath it is how you get a
// lock error, or worse, its attention. Nothing read here leaves the machine.
public static class Chromium
{
    /// A browser, by the folder its profiles live in.
    public sealed record Source(string Name, string Root, string? Export = null)
    {
        public string Id => Name;

        /// Every profile, not only the default one: the folders holding a
        /// "Bookmarks" or a "History". Opera keeps its one profile in the root
        /// itself; the rest keep "Default" and "Profile N" beneath it.
        public List<string> Profiles
        {
            get
            {
                var found = new List<string>();
                if (!Directory.Exists(Root)) return found;
                if (IsProfile(Root)) found.Add(Root);
                try
                {
                    // One level is as deep as a profile goes; going further is
                    // a walk through the cache.
                    foreach (var dir in Directory.EnumerateDirectories(Root))
                        if (IsProfile(dir)) found.Add(dir);
                }
                catch { }
                return found;
            }
        }

        private static bool IsProfile(string dir) =>
            File.Exists(Path.Combine(dir, "Bookmarks")) || File.Exists(Path.Combine(dir, "History"));
    }

    private static string Local => Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
    private static string Roaming => Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData);

    public static List<Source> Known()
    {
        var known = new List<Source>
        {
            new("Chrome", Path.Combine(Local, @"Google\Chrome\User Data"), "chrome://password-manager/settings"),
            new("Edge", Path.Combine(Local, @"Microsoft\Edge\User Data"), "edge://wallet/passwords"),
            new("Brave", Path.Combine(Local, @"BraveSoftware\Brave-Browser\User Data"), "brave://password-manager/settings"),
            new("Vivaldi", Path.Combine(Local, @"Vivaldi\User Data")),
            new("Opera", Path.Combine(Roaming, @"Opera Software\Opera Stable")),
            new("Opera GX", Path.Combine(Roaming, @"Opera Software\Opera GX Stable")),
            new("Chromium", Path.Combine(Local, @"Chromium\User Data")),
        };
        // Arc comes from the Store, and lives inside its package's folder.
        try
        {
            var packages = Path.Combine(Local, "Packages");
            if (Directory.Exists(packages))
                foreach (var dir in Directory.EnumerateDirectories(packages, "TheBrowserCompany.Arc*"))
                    known.Insert(2, new Source("Arc", Path.Combine(dir, @"LocalCache\Local\Arc\User Data")));
        }
        catch { }
        return known;
    }

    /// Only the browsers actually on this PC, with something to read.
    public static List<Source> Installed() => Known().Where(s => s.Profiles.Count > 0).ToList();

    /// Where to go in that browser for the file the passwords panel takes.
    public static string ExportHint(Source source) =>
        source.Export is { } page
            ? $"{source.Name} exports its passwords as a CSV from {page}"
            : $"{source.Name} exports its passwords as a CSV from its password settings";

    // MARK: - what they kept

    /// The other browser's bookmarks: the bar first, then anything filed
    /// elsewhere, folders and all. Chromium keeps them as one JSON file.
    public static List<Bookmark> Bookmarks(Source source)
    {
        var @out = new List<Bookmark>();
        foreach (var profile in source.Profiles)
        {
            var file = Path.Combine(profile, "Bookmarks");
            if (!File.Exists(file)) continue;
            try
            {
                using var doc = JsonDocument.Parse(ReadShared(file));
                if (!doc.RootElement.TryGetProperty("roots", out var roots)) continue;
                if (roots.TryGetProperty("bookmark_bar", out var bar)) @out.AddRange(Nodes(bar));
                foreach (var key in new[] { "other", "synced" })
                {
                    if (!roots.TryGetProperty(key, out var more)) continue;
                    var kids = Nodes(more);
                    if (kids.Count > 0) @out.Add(Bookmark.Folder(key == "other" ? "Other" : "Mobile", kids));
                }
            }
            catch { }
        }
        return @out;
    }

    private static List<Bookmark> Nodes(JsonElement parent)
    {
        var @out = new List<Bookmark>();
        if (!parent.TryGetProperty("children", out var children) || children.ValueKind != JsonValueKind.Array) return @out;
        foreach (var entry in children.EnumerateArray())
        {
            var name = entry.TryGetProperty("name", out var n) ? n.GetString() ?? "" : "";
            var type = entry.TryGetProperty("type", out var t) ? t.GetString() : null;
            if (type == "folder") @out.Add(Bookmark.Folder(name, Nodes(entry)));
            else if (type == "url" && entry.TryGetProperty("url", out var u)
                && Uri.TryCreate(u.GetString(), UriKind.Absolute, out var url) && Address.IsWeb(url))
                @out.Add(Bookmark.Site(name, url));
        }
        return @out;
    }

    /// The other browser's icons for the given pages, host by host: the
    /// largest bitmap it kept for the page itself, or failing that for the
    /// site's front door. Read from a copy of its "Favicons" file.
    public static Dictionary<string, byte[]> Icons(Source source, IEnumerable<Uri> urls, int limit = 400)
    {
        var @out = new Dictionary<string, byte[]>();
        var wanted = new List<(string host, Uri url)>();
        var seen = new HashSet<string>();
        foreach (var url in urls)
        {
            if (Address.Host(url) is not { } host || !seen.Add(host)) continue;
            wanted.Add((host, url));
            if (wanted.Count >= limit) break;
        }
        if (wanted.Count == 0) return @out;

        foreach (var profile in source.Profiles)
        {
            var file = Path.Combine(profile, "Favicons");
            if (!File.Exists(file)) continue;
            using var db = Sqlite.OpenCopy(file);
            if (db == null) continue;
            using var statement = db.Prepare("""
                SELECT b.image_data FROM icon_mapping m
                JOIN favicon_bitmaps b ON b.icon_id = m.icon_id
                WHERE m.page_url = ? AND b.width BETWEEN 16 AND 256
                ORDER BY b.width DESC LIMIT 1
                """);
            if (statement == null) continue;
            foreach (var (host, url) in wanted)
            {
                if (@out.ContainsKey(host)) continue;
                foreach (var door in new[] { url.AbsoluteUri, $"{url.Scheme}://{url.Host}/" })
                {
                    statement.Reset();
                    statement.Bind(1, door);
                    if (!statement.Step() || statement.Blob(0) is not { Length: > 60 } png) continue;
                    @out[host] = png;
                    break;
                }
            }
        }
        return @out;
    }

    // MARK: - where they have been

    public sealed record Place(Uri Url, string Title, int Count, DateTime Last);

    /// The other browser's history — what it takes to finish an address on
    /// the first day. A copy of the file, read once.
    public static List<Place> Places(Source source, int limit = 3000)
    {
        var @out = new List<Place>();
        foreach (var profile in source.Profiles)
        {
            var file = Path.Combine(profile, "History");
            if (!File.Exists(file)) continue;
            using var db = Sqlite.OpenCopy(file);
            if (db == null) continue;
            using var statement = db.Prepare($"""
                SELECT url, title, visit_count, last_visit_time FROM urls
                WHERE hidden = 0 AND visit_count > 0
                ORDER BY last_visit_time DESC LIMIT {limit}
                """);
            if (statement == null) continue;
            while (statement.Step())
            {
                if (!Uri.TryCreate(statement.Text(0), UriKind.Absolute, out var url) || !Address.IsWeb(url)) continue;
                var count = (int)statement.Int64(2);
                // Microseconds since 1601, Chromium's idea of a date — which
                // is Windows' own, in tenths.
                var stamp = statement.Int64(3);
                var last = stamp > 0 && stamp < DateTime.MaxValue.ToFileTimeUtc() / 10 ? DateTime.FromFileTimeUtc(stamp * 10) : DateTime.UtcNow;
                @out.Add(new Place(url, statement.Text(1), Math.Max(1, count), last));
            }
        }
        return @out.OrderByDescending(p => p.Last).Take(limit).ToList();
    }

    /// A file the other browser may have open, read without getting in its way.
    private static byte[] ReadShared(string file)
    {
        using var stream = new FileStream(file, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete);
        using var memory = new MemoryStream();
        stream.CopyTo(memory);
        return memory.ToArray();
    }

    // MARK: - the files

    /// SQLite, as Windows itself ships it (winsqlite3.dll, there since
    /// Windows 10) — only what reading a copied file needs.
    private sealed class Sqlite : IDisposable
    {
        private IntPtr db;
        private readonly string temp;

        private Sqlite(IntPtr db, string temp)
        {
            this.db = db;
            this.temp = temp;
        }

        /// A copy, next to nothing the other browser is watching.
        public static Sqlite? OpenCopy(string file)
        {
            var temp = Path.Combine(Path.GetTempPath(), $"office-import-{Guid.NewGuid():N}.db");
            try
            {
                // Only the main file. A WAL-mode database opens read-only from
                // its main file alone; copying the -wal too would need a -shm
                // we can't make beside it, and the read would fail instead.
                File.WriteAllBytes(temp, ReadShared(file));
            }
            catch
            {
                Remove(temp);
                return null;
            }
            if (sqlite3_open_v2(Utf8(temp), out var db, ReadOnly, IntPtr.Zero) != Ok)
            {
                if (db != IntPtr.Zero) sqlite3_close(db);
                Remove(temp);
                return null;
            }
            return new Sqlite(db, temp);
        }

        public Statement? Prepare(string sql) =>
            sqlite3_prepare_v2(db, Utf8(sql), -1, out var statement, IntPtr.Zero) == Ok && statement != IntPtr.Zero
                ? new Statement(statement)
                : null;

        public void Dispose()
        {
            if (db != IntPtr.Zero) sqlite3_close(db);
            db = IntPtr.Zero;
            Remove(temp);
        }

        private static void Remove(string temp)
        {
            foreach (var name in new[] { temp, temp + "-wal", temp + "-shm", temp + "-journal" })
                try { File.Delete(name); } catch { }
        }
    }

    private sealed class Statement(IntPtr handle) : IDisposable
    {
        public bool Step() => sqlite3_step(handle) == Row;
        public void Reset() => sqlite3_reset(handle);
        public void Bind(int index, string text)
        {
            var bytes = Encoding.UTF8.GetBytes(text);
            sqlite3_bind_text(handle, index, bytes, bytes.Length, Transient);
        }

        public string Text(int column)
        {
            var at = sqlite3_column_text(handle, column);
            return at == IntPtr.Zero ? "" : Marshal.PtrToStringUTF8(at, sqlite3_column_bytes(handle, column)) ?? "";
        }

        public long Int64(int column) => sqlite3_column_int64(handle, column);

        public byte[]? Blob(int column)
        {
            var at = sqlite3_column_blob(handle, column);
            var count = sqlite3_column_bytes(handle, column);
            if (at == IntPtr.Zero || count <= 0) return null;
            var bytes = new byte[count];
            Marshal.Copy(at, bytes, 0, count);
            return bytes;
        }

        public void Dispose() => sqlite3_finalize(handle);
    }

    private static byte[] Utf8(string text) => Encoding.UTF8.GetBytes(text + "\0");

    private const int Ok = 0, Row = 100, ReadOnly = 0x1;
    private static readonly IntPtr Transient = new(-1);

    [DllImport("winsqlite3.dll")] private static extern int sqlite3_open_v2(byte[] filename, out IntPtr db, int flags, IntPtr vfs);
    [DllImport("winsqlite3.dll")] private static extern int sqlite3_close(IntPtr db);
    [DllImport("winsqlite3.dll")] private static extern int sqlite3_prepare_v2(IntPtr db, byte[] sql, int bytes, out IntPtr statement, IntPtr tail);
    [DllImport("winsqlite3.dll")] private static extern int sqlite3_step(IntPtr statement);
    [DllImport("winsqlite3.dll")] private static extern int sqlite3_reset(IntPtr statement);
    [DllImport("winsqlite3.dll")] private static extern int sqlite3_finalize(IntPtr statement);
    [DllImport("winsqlite3.dll")] private static extern int sqlite3_bind_text(IntPtr statement, int index, byte[] text, int bytes, IntPtr destructor);
    [DllImport("winsqlite3.dll")] private static extern IntPtr sqlite3_column_text(IntPtr statement, int column);
    [DllImport("winsqlite3.dll")] private static extern IntPtr sqlite3_column_blob(IntPtr statement, int column);
    [DllImport("winsqlite3.dll")] private static extern int sqlite3_column_bytes(IntPtr statement, int column);
    [DllImport("winsqlite3.dll")] private static extern long sqlite3_column_int64(IntPtr statement, int column);
}
