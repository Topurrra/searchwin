using System.Text.Json.Serialization;

namespace Search;

// Spaces: separate sets of tabs in the one window, each with its own
// cookies and sign-ins, and a downloads folder of its own if you like.
//
// Off unless turned on in Settings › Tabs. Until then there is one space,
// the first, and nothing about it shows: its tabs are the session there
// has always been and its sites use the profile there has always been, so
// turning spaces on signs nobody out.
//
// A space's sites live in a WebView2 profile of their own, named after it;
// history, bookmarks, the passwords, settings and extensions are shared by
// every space. Switching swaps the row of tabs: the ones left behind are
// parked, their sound paused, and they sleep after half an hour as any tab
// does. Alt+1–Alt+9 switch, as Ctrl+1–Ctrl+9 do in Arc.

/// A set of tabs of its own, signed in where the others are or starting
/// afresh (see Browser.Spaces). The first one is the browser there always
/// was, and its tabs are the ones in session.json.
///
/// Written to spaces.json in the Mac's own shape — the same keys, and the
/// icon as the name of the Mac's symbol — so the list reads the same on both.
public sealed class Space
{
    public Guid Id { get; set; }
    public string Name { get; set; } = "";
    /// Which of the Mac's colours — from before spaces had icons; kept so an
    /// older list still reads.
    public int Colour { get; set; }
    /// Its icon, one of `Space.Icons`.
    public string? Icon { get; set; }
    /// Signed in wherever the first space is — the same cookies and
    /// sign-ins, only the tabs its own — rather than a profile of its own.
    /// Chosen when it is made; null, for a space from before the choice, is
    /// a profile of its own.
    public bool? SharesSignIns { get; set; }
    /// Where this space's downloads go; null for the folder in Settings.
    public string? Downloads { get; set; }

    /// The first space: the session and the profile there were before spaces.
    public static readonly Guid FirstID = new("00000000-0000-0000-0000-000000000001");
    [JsonIgnore] public bool IsFirst => Id == FirstID;

    /// The icon it shows: its own, or a house for the first and a briefcase
    /// for any other that has none yet.
    [JsonIgnore]
    public string Symbol => Icon is { } icon && Icons.Contains(icon) ? icon : IsFirst ? "house" : "briefcase";

    /// The same, as a glyph of Segoe Fluent Icons.
    [JsonIgnore] public string Glyph => GlyphOf(Symbol);

    // MARK: - the icons

    /// The icons a space can wear, grouped as work, thinking, leisure and
    /// life. Named as the Mac names them — its SF Symbols — and drawn here
    /// with the nearest of Segoe Fluent Icons, in one weight and one grey.
    public static readonly string[] Icons =
    [
        "briefcase", "building.2", "desktopcomputer", "laptopcomputer", "chevron.left.forwardslash.chevron.right", "terminal",
        "sparkles", "brain.head.profile", "lightbulb", "gamecontroller", "beach.umbrella", "cup.and.saucer",
        "music.note", "film", "paintpalette", "camera", "house", "book",
        "graduationcap", "cart", "airplane", "dumbbell", "leaf", "heart",
    ];

    public static readonly string[] IconNames =
    [
        "Work", "Office", "Desktop", "Laptop", "Code", "Terminal",
        "AI", "Thinking", "Ideas", "Games", "Leisure", "Café",
        "Music", "Film", "Art", "Photos", "Home", "Reading",
        "Studies", "Shopping", "Travel", "Sport", "Nature", "Personal",
    ];

    private static readonly string[] Glyphs =
    [
        "", "", "", "", "", "",
        "", "", "", "", "", "",
        "", "", "", "", "", "",
        "", "", "", "", "", "",
    ];

    public static string GlyphOf(string symbol)
    {
        var at = Array.IndexOf(Icons, symbol);
        return at >= 0 ? Glyphs[at] : Glyphs[0];
    }

    // MARK: - the list

    private const string File = "spaces.json";

    /// Every space, the first one first — made on the spot if there is no
    /// list yet.
    public static List<Space> Read()
    {
        var saved = Store.Read<List<Space>>(File) ?? [];
        var first = saved.FirstOrDefault(s => s.IsFirst) ?? new Space { Id = FirstID, Name = "Personal", Colour = 0 };
        return [first, .. saved.Where(s => !s.IsFirst)];
    }

    public static void Write(List<Space> spaces) => Store.Write(File, spaces);

    /// The space new tabs are made in: the one on screen.
    public static Guid Current = FirstID;
    /// The spaces signed in wherever the first one is (see SharesSignIns).
    public static HashSet<Guid> Sharing = [];

    /// The WebView2 profile a space's tabs live in. WebView2 keeps processes
    /// per profile, so every tab of a space asking for the same name is what
    /// WebKit's one store object per space was on the Mac.
    public static string Profile(Guid id) =>
        id == FirstID || Sharing.Contains(id) ? "Default" : "space-" + id.ToString("N");

    // MARK: - erasing

    /// Where WebView2 keeps a profile's cookies, storage and caches.
    private static string Folder(Guid id) => Path.Combine(Web.DataFolder, "EBWebView", "space-" + id.ToString("N"));

    /// A space's profile and everything in it, gone. What it holds —
    /// cookies, sign-ins, storage, caches — is emptied at once through a
    /// page of that profile when there is one (see Browser.DeleteSpace).
    /// The folder itself WebView2 won't let go of while this run has had the
    /// profile open, however closed its tabs, so it is written down and
    /// removed at the next launch if the tries in between don't manage.
    public static void Erase(Guid id)
    {
        if (id == FirstID) return;
        var pending = Store.Settings.Strings("spaces.erasing").ToHashSet();
        pending.Add(id.ToString().ToUpperInvariant());
        Store.Settings.Set("spaces.erasing", pending.Order());
        Sweep();
        foreach (var delay in new[] { 3.0, 15.0 }) UI.After(delay, Sweep);
    }

    /// Every profile of a deleted space that is still there, tried again.
    public static void Sweep()
    {
        var pending = Store.Settings.Strings("spaces.erasing");
        if (pending.Count == 0) return;
        Task.Run(() =>
        {
            var gone = new List<string>();
            foreach (var text in pending)
            {
                if (!Guid.TryParse(text, out var id)) { gone.Add(text); continue; }
                var folder = Folder(id);
                try
                {
                    // Gone already is as good as removed; anything else is
                    // tried again later.
                    if (Directory.Exists(folder)) Directory.Delete(folder, recursive: true);
                    gone.Add(text);
                }
                catch { }
            }
            if (gone.Count == 0) return;
            UI.Do(() =>
            {
                var now = Store.Settings.Strings("spaces.erasing").Where(t => !gone.Contains(t)).ToList();
                if (now.Count == 0) Store.Settings.Remove("spaces.erasing");
                else Store.Settings.Set("spaces.erasing", now);
            });
        });
    }

    /// What is still on disk for the spaces being erased, for the bench.
    public static List<string> Leftovers() =>
        Store.Settings.Strings("spaces.erasing")
            .Where(t => Guid.TryParse(t, out var id) && Directory.Exists(Folder(id)))
            .ToList();

    /// Every profile folder of a space WebView2 has made, for the bench.
    public static List<string> Stores()
    {
        try
        {
            var root = Path.Combine(Web.DataFolder, "EBWebView");
            if (!Directory.Exists(root)) return [];
            return Directory.GetDirectories(root, "space-*").Select(Path.GetFileName).OfType<string>().ToList();
        }
        catch { return []; }
    }
}

/// Another space's row, as it was left.
public sealed partial class Parked(List<Tab> tabs, Guid? active)
{
    public List<Tab> Tabs { get; } = tabs;
    public Guid? Active { get; set; } = active;
}
