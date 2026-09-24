using System.Text.Json.Nodes;

namespace SearchKit.Commands;

/// `!yt cats` → a YouTube search for cats. `{q}` in the address is where the
/// words go, escaped.
public sealed record Bang(string Trigger, string Name, string Address, bool BuiltIn = false)
{
    public Uri? For(string query)
    {
        var text = Address.Replace("{q}", Uri.EscapeDataString(query.Trim()), StringComparison.Ordinal);
        return Uri.TryCreate(text, UriKind.Absolute, out var url) && (url.Scheme == "https" || url.Scheme == "http")
            ? url
            : null;
    }

    /// With no words, a bang goes to the site itself.
    public Uri? Home()
    {
        if (!Uri.TryCreate(Address.Replace("{q}", "", StringComparison.Ordinal), UriKind.Absolute, out var url)) return null;
        return new Uri(url.GetLeftPart(UriPartial.Authority) + "/");
    }
}

/// The built-in bangs and your own. Yours are kept in a small JSON file and
/// win over a built-in with the same trigger.
public sealed class Bangs
{
    public static readonly IReadOnlyList<Bang> Starter =
    [
        new("g", "Google", "https://www.google.com/search?q={q}", true),
        new("ddg", "DuckDuckGo", "https://duckduckgo.com/?q={q}", true),
        new("b", "Bing", "https://www.bing.com/search?q={q}", true),
        new("br", "Brave Search", "https://search.brave.com/search?q={q}", true),
        new("yt", "YouTube", "https://www.youtube.com/results?search_query={q}", true),
        new("w", "Wikipedia", "https://en.wikipedia.org/w/index.php?search={q}", true),
        new("gh", "GitHub", "https://github.com/search?q={q}", true),
        new("so", "Stack Overflow", "https://stackoverflow.com/search?q={q}", true),
        new("r", "Reddit", "https://www.reddit.com/search/?q={q}", true),
        new("maps", "Google Maps", "https://www.google.com/maps/search/{q}", true),
        new("tr", "Google Translate", "https://translate.google.com/?sl=auto&text={q}", true),
        new("a", "Amazon", "https://www.amazon.com/s?k={q}", true),
        new("imdb", "IMDb", "https://www.imdb.com/find/?q={q}", true),
        new("mdn", "MDN Web Docs", "https://developer.mozilla.org/en-US/search?q={q}", true),
        new("npm", "npm", "https://www.npmjs.com/search?q={q}", true),
        new("crates", "crates.io", "https://crates.io/search?q={q}", true),
        new("nuget", "NuGet", "https://www.nuget.org/packages?q={q}", true),
        new("x", "X", "https://x.com/search?q={q}", true),
        new("wa", "Wolfram|Alpha", "https://www.wolframalpha.com/input?i={q}", true),
        new("hn", "Hacker News", "https://hn.algolia.com/?q={q}", true),
    ];

    private readonly string? file;
    private readonly Dictionary<string, Bang> byTrigger = new(StringComparer.OrdinalIgnoreCase);
    private readonly List<Bang> yours = [];

    /// `file` is where yours are kept; null keeps them in memory only.
    public Bangs(string? file = null)
    {
        this.file = file;
        foreach (var bang in Starter) byTrigger[bang.Trigger] = bang;
        foreach (var bang in Load()) Put(bang);
    }

    public IReadOnlyList<Bang> Yours => yours;

    public IEnumerable<Bang> All => byTrigger.Values.OrderBy(b => b.Trigger, StringComparer.OrdinalIgnoreCase);

    public Bang? Find(string trigger) => byTrigger.GetValueOrDefault(trigger);

    /// Adds or replaces one of yours, and keeps it.
    public void Add(string trigger, string name, string address)
    {
        trigger = trigger.Trim().TrimStart('!');
        if (trigger.Length == 0 || trigger.Any(char.IsWhiteSpace))
            throw new ArgumentException("A bang's trigger is one word.");
        if (!address.Contains("{q}", StringComparison.Ordinal))
            throw new ArgumentException("A bang's address needs {q} where the words go.");
        var bang = new Bang(trigger, name.Trim().Length > 0 ? name.Trim() : trigger, address.Trim());
        if (bang.For("test") is null) throw new ArgumentException("A bang's address must be a web address.");
        yours.RemoveAll(b => string.Equals(b.Trigger, trigger, StringComparison.OrdinalIgnoreCase));
        Put(bang);
        Save();
    }

    /// Removes one of yours; a built-in it replaced comes back.
    public bool Remove(string trigger)
    {
        trigger = trigger.Trim().TrimStart('!');
        if (yours.RemoveAll(b => string.Equals(b.Trigger, trigger, StringComparison.OrdinalIgnoreCase)) == 0) return false;
        byTrigger.Remove(trigger);
        if (Starter.FirstOrDefault(b => string.Equals(b.Trigger, trigger, StringComparison.OrdinalIgnoreCase)) is { } builtIn)
            byTrigger[trigger] = builtIn;
        Save();
        return true;
    }

    private void Put(Bang bang)
    {
        yours.Add(bang);
        byTrigger[bang.Trigger] = bang;
    }

    private IEnumerable<Bang> Load()
    {
        if (file == null || !File.Exists(file)) yield break;
        JsonArray? list;
        try { list = JsonNode.Parse(File.ReadAllText(file)) as JsonArray; }
        catch { yield break; }
        foreach (var node in list ?? [])
        {
            if (node is not JsonObject entry) continue;
            var trigger = entry["trigger"]?.GetValue<string>();
            var address = entry["address"]?.GetValue<string>();
            if (string.IsNullOrWhiteSpace(trigger) || string.IsNullOrWhiteSpace(address)) continue;
            yield return new Bang(trigger, entry["name"]?.GetValue<string>() ?? trigger, address);
        }
    }

    private void Save()
    {
        if (file == null) return;
        var list = new JsonArray([.. yours.Select(b => (JsonNode)new JsonObject
        {
            ["trigger"] = b.Trigger,
            ["name"] = b.Name,
            ["address"] = b.Address,
        })]);
        Directory.CreateDirectory(Path.GetDirectoryName(Path.GetFullPath(file))!);
        var temp = file + ".tmp";
        File.WriteAllText(temp, list.ToJsonString(new System.Text.Json.JsonSerializerOptions { WriteIndented = true }));
        File.Move(temp, file, overwrite: true);
    }
}
