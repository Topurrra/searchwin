using System.Collections.Concurrent;
using System.Text.Json;
using System.Text.Json.Nodes;

namespace Search;

// Where everything this browser keeps is kept.
//
// One place, and one rule: a run started for testing never touches the folder
// or the settings of the browser somebody is actually using. Sharing them once
// cost a person their pinned tabs, which is not a mistake worth being able to
// make twice.
public static class Store
{
    /// A run is a test run if it says so (SEARCH_PROBE), or if it is being run
    /// straight out of the build folder rather than from a published copy.
    public static bool Testing
    {
        get
        {
            if (Environment.GetEnvironmentVariable("SEARCH_PROBE") != null) return true;
            var exe = Environment.ProcessPath ?? "";
            return exe.Contains(@"\bin\Debug\", StringComparison.OrdinalIgnoreCase);
        }
    }

    /// Which test world a test run lives in: "test" for SEARCH_PROBE=1 or a
    /// debug build, SEARCH_PROBE=<name> for a world of its own. Null for the
    /// browser somebody is using.
    public static readonly string? World = ComputeWorld();

    private static string? ComputeWorld()
    {
        if (!Testing) return null;
        var asked = new string((Environment.GetEnvironmentVariable("SEARCH_PROBE") ?? "").ToLowerInvariant()
            .Where(c => (c < 128 && char.IsLetterOrDigit(c)) || c == '-').ToArray());
        return asked is "" or "1" or "test" ? "test" : asked;
    }

    /// %LOCALAPPDATA%\Search — history, bookmarks, the session, what is
    /// hidden on each site. Local rather than roaming: nothing here is meant
    /// to follow you to another machine, and WebView2's own profile beside it
    /// is far too big to roam.
    public static readonly string Folder = MakeFolder();

    private static string MakeFolder()
    {
        var local = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
        var home = Path.Combine(local, World is { } w ? $"Search ({w})" : "Search");
        Directory.CreateDirectory(home);
        return home;
    }

    public static string File(string name) => Path.Combine(Folder, name);

    /// A file that didn't decode is set aside rather than overwritten the next
    /// time something is saved over it — nobody wants to lose bookmarks,
    /// history or a session to a bad read with no trace of what was there.
    public static void Quarantine(string file)
    {
        try
        {
            if (!System.IO.File.Exists(file)) return;
            var stamp = DateTimeOffset.UtcNow.ToUnixTimeSeconds();
            var aside = Path.Combine(Path.GetDirectoryName(file)!,
                $"{Path.GetFileNameWithoutExtension(file)}.unreadable-{stamp}.json");
            System.IO.File.Move(file, aside);
        }
        catch { }
    }

    /// Written whole, then moved into place, so a crash halfway through a
    /// write leaves yesterday's file rather than half of today's.
    public static void WriteAtomic(string file, byte[] data)
    {
        try
        {
            Directory.CreateDirectory(Path.GetDirectoryName(file)!);
            var temp = file + ".tmp";
            System.IO.File.WriteAllBytes(temp, data);
            System.IO.File.Move(temp, file, overwrite: true);
        }
        catch { }
    }

    /// How each kept file is read and written, generated at build time (see
    /// Json). A type missing from that list is a mistake in this app, not in
    /// the file, and says so loudly rather than quietly losing data.
    private static System.Text.Json.Serialization.Metadata.JsonTypeInfo<T> Shape<T>() =>
        (System.Text.Json.Serialization.Metadata.JsonTypeInfo<T>?)Search.Json.Default.GetTypeInfo(typeof(T))
            ?? throw new InvalidOperationException($"{typeof(T)} is not in Json.cs");

    public static T? Read<T>(string name) where T : class
    {
        var file = File(name);
        if (!System.IO.File.Exists(file)) return null;
        byte[] bytes;
        try { bytes = System.IO.File.ReadAllBytes(file); }
        catch { return null; }
        try { return JsonSerializer.Deserialize(bytes, Shape<T>()); }
        // Only a file that really doesn't parse is set aside. Anything else —
        // a file still being written, the app's own mistake — leaves it
        // exactly where it is.
        catch (JsonException)
        {
            Quarantine(file);
            return null;
        }
    }

    public static void Write<T>(string name, T value, bool now = false)
    {
        var bytes = JsonSerializer.SerializeToUtf8Bytes(value, Shape<T>());
        var file = File(name);
        if (now) WriteAtomic(file, bytes);
        else Task.Run(() => WriteAtomic(file, bytes));
    }

    /// The app's own settings — what UserDefaults is on the Mac. One small
    /// JSON file of keys and values, written a moment after a change.
    public static readonly Defaults Settings = new(File("settings.json"));
}

/// Keys and values, the way UserDefaults keeps them: typed reads with a
/// fallback, writes coalesced so a slider dragged across its range writes the
/// file once.
public sealed class Defaults
{
    private readonly string file;
    private readonly ConcurrentDictionary<string, JsonNode?> values = new();
    private int pending;

    public Defaults(string file)
    {
        this.file = file;
        try
        {
            if (System.IO.File.Exists(file) && JsonNode.Parse(System.IO.File.ReadAllText(file)) is JsonObject obj)
                foreach (var (key, value) in obj) values[key] = value?.DeepClone();
        }
        catch { Store.Quarantine(file); }
    }

    public bool Has(string key) => values.ContainsKey(key);
    public IEnumerable<string> Keys => values.Keys;

    public bool Bool(string key, bool fallback = false) =>
        values.TryGetValue(key, out var v) && v is JsonValue j && j.TryGetValue<bool>(out var b) ? b : fallback;

    public bool? OptionalBool(string key) =>
        values.TryGetValue(key, out var v) && v is JsonValue j && j.TryGetValue<bool>(out var b) ? b : null;

    public double Double(string key, double fallback = 0) =>
        values.TryGetValue(key, out var v) && v is JsonValue j && j.TryGetValue<double>(out var d) ? d : fallback;

    public double? OptionalDouble(string key) =>
        values.TryGetValue(key, out var v) && v is JsonValue j && j.TryGetValue<double>(out var d) ? d : null;

    public string? String(string key) =>
        values.TryGetValue(key, out var v) && v is JsonValue j && j.TryGetValue<string>(out var s) ? s : null;

    public List<string> Strings(string key) =>
        values.TryGetValue(key, out var v) && v is JsonArray a
            ? a.Select(x => x?.GetValue<string>() ?? "").Where(s => s.Length > 0).ToList()
            : [];

    public void Set(string key, bool value) => Put(key, JsonValue.Create(value));
    public void Set(string key, double value) => Put(key, JsonValue.Create(value));
    public void Set(string key, string? value) => Put(key, value == null ? null : JsonValue.Create(value));
    public void Set(string key, IEnumerable<string> list) => Put(key, new JsonArray(list.Select(s => (JsonNode?)JsonValue.Create(s)).ToArray()));

    public void Remove(string key)
    {
        if (values.TryRemove(key, out _)) Save();
    }

    private void Put(string key, JsonNode? value)
    {
        if (value == null) { Remove(key); return; }
        values[key] = value;
        Save();
    }

    private void Save()
    {
        if (Interlocked.Exchange(ref pending, 1) == 1) return;
        _ = Task.Delay(400).ContinueWith(_ => Flush());
    }

    /// Straight to disk — for quitting, when there is no later.
    public void Flush()
    {
        Interlocked.Exchange(ref pending, 0);
        var obj = new JsonObject();
        foreach (var (key, value) in values.OrderBy(p => p.Key)) obj[key] = value?.DeepClone();
        lock (this) Store.WriteAtomic(file, System.Text.Encoding.UTF8.GetBytes(obj.ToJsonString(new JsonSerializerOptions { WriteIndented = true })));
    }
}
