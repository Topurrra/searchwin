namespace Search;

// Everything there is to set, in one observable place.
//
// Each of these is a line in the settings file and nothing more; the object
// exists so that a panel can bind to them and the rest of the window can
// redraw when one changes. Defaults are chosen so that a browser nobody has
// configured behaves the way it always did.

/// What a tab wears beside its title, and what a pinned one is reduced to: a
/// letter, or the site's own icon.
public enum Glyph { Letters, Icons }

/// Light, dark, or Windows' own — the one choice that colours everything.
public enum Look { Light, Dark, System }

public static class Choices
{
    public static string Title(this Glyph g) => g == Glyph.Icons ? "Site icons" : "Letters";
    public static string Title(this Look l) => l switch { Look.Light => "Light", Look.Dark => "Dark", _ => "System" };
    public static string Raw(this Glyph g) => g == Glyph.Icons ? "icons" : "letters";
    public static string Raw(this Look l) => l.ToString().ToLowerInvariant();
}

public sealed partial class Preferences : Model
{
    private readonly Defaults store = Store.Settings;

    public Preferences()
    {
        bench = store.Bool("bench");
        look = store.String("look") switch { "light" => Look.Light, "dark" => Look.Dark, _ => Look.System };
        sidebar = store.OptionalBool("sidebar") ?? false;
        sideHides = store.Bool("sidebar.hides");
        sideWidth = Math.Clamp(store.Double("sidebar.width", Metrics.Side), Metrics.SideMin, Metrics.SideMax);
        glyph = store.String("glyph") == "icons" ? Glyph.Icons : Glyph.Letters;
        sleepsTabs = store.OptionalBool("tabs.sleep") ?? true;
        shielded = store.OptionalBool("shield") ?? true;
        shieldLists = store.OptionalBool("shield.lists") ?? true;
        tidiesLinks = store.OptionalBool("shield.links") ?? true;
        rejectsCookies = store.OptionalBool("shield.cookies") ?? true;
        warnsOfScams = store.OptionalBool("fishcatcher") ?? true;
        scamFeed = store.OptionalBool("fishcatcher.feed") ?? true;
        // WebView2 does passkeys through Windows Hello out of the box, so a
        // build here can always offer them.
        passkeys = store.OptionalBool("passkeys") ?? true;
        var downloadsSet = store.String("downloads");
        downloads = Store.Testing
            ? Directory.CreateDirectory(Store.File("Downloads")).FullName
            : downloadsSet ?? KnownFolders.Downloads;
        asksWhereToSave = store.Bool("downloads.ask");
        savesPasswords = store.OptionalBool("passwords.save") ?? true;
        fillsPasswords = store.OptionalBool("passwords.fill") ?? true;
        // Anyone who already has a session was here before the welcome
        // existed; they are not asked to sit through it.
        welcomed = store.Bool("welcomed") || store.Has("glyph");
        spelling = store.OptionalBool("spelling") ?? true;
        usesSpaces = store.Bool("spaces");
        searchFolders = store.Strings("files.folders");
        fileContents = store.OptionalBool("files.contents") ?? true;
        appsInField = store.OptionalBool("field.apps") ?? true;
    }

    private bool bench;
    /// A local socket a script can drive the browser through, in tabs of its
    /// own. Off unless asked for.
    public bool Bench { get => bench; set { if (Set(ref bench, value)) store.Set("bench", value); } }

    private Look look;
    public Look Look { get => look; set { if (Set(ref look, value)) store.Set("look", value.Raw()); } }

    private bool sidebar;
    /// Titles down the left instead of across the top.
    public bool Sidebar { get => sidebar; set { if (Set(ref sidebar, value)) store.Set("sidebar", value); } }

    private bool sideHides;
    /// The column folded away whenever the pointer isn't at the left edge,
    /// rather than only after Ctrl+S (see Fold). Off unless asked for.
    public bool SideHides { get => sideHides; set { if (Set(ref sideHides, value)) store.Set("sidebar.hides", value); } }

    private double sideWidth;
    /// How wide the column is. Pulled by its edge, and remembered.
    public double SideWidth { get => sideWidth; set { if (Set(ref sideWidth, value)) store.Set("sidebar.width", value); } }

    private Glyph glyph;
    public Glyph Glyph { get => glyph; set { if (Set(ref glyph, value)) store.Set("glyph", value.Raw()); } }

    private bool sleepsTabs;
    /// Tabs nobody has looked at for half an hour give their page back and
    /// keep where they were. On unless turned off.
    public bool SleepsTabs { get => sleepsTabs; set { if (Set(ref sleepsTabs, value)) store.Set("tabs.sleep", value); } }

    private bool shielded;
    /// The ad blocker. On unless turned off; there is nothing else to it.
    public bool Shielded { get => shielded; set { if (Set(ref shielded, value)) store.Set("shield", value); } }

    private bool shieldLists;
    /// EasyList and EasyPrivacy in place of the short built-in list. They are
    /// a download — the one network call the blocker makes, once a day, from
    /// easylist.to — but a full, current list is worth more than a 44-domain
    /// fallback, so this is on unless turned off (superseding Decisions D28).
    public bool ShieldLists { get => shieldLists; set { if (Set(ref shieldLists, value)) store.Set("shield.lists", value); } }

    private bool tidiesLinks;
    /// Tracking tags taken off addresses (utm_…, click ids), redirect
    /// wrappers unwrapped, AMP pages swapped for the real one. On unless
    /// turned off.
    public bool TidiesLinks { get => tidiesLinks; set { if (Set(ref tidiesLinks, value)) store.Set("shield.links", value); } }

    private bool rejectsCookies;
    /// Cookie banners answered "no" where a page lets that be said, and
    /// hidden where it doesn't. On unless turned off.
    public bool RejectsCookies { get => rejectsCookies; set { if (Set(ref rejectsCookies, value)) store.Set("shield.cookies", value); } }

    private bool warnsOfScams;
    /// FishCatcher: a warning before a scam or phishing site loads. On
    /// unless turned off; it reads addresses on this computer and asks
    /// nobody about them.
    public bool WarnsOfScams { get => warnsOfScams; set { if (Set(ref warnsOfScams, value)) store.Set("fishcatcher", value); } }

    private bool scamFeed;
    /// FishCatcher's daily list of newly reported scam sites. It is a
    /// download — the one network call FishCatcher makes, once a day, from
    /// the registry — verified by its signature before anything in it is
    /// used, and it is on unless turned off.
    public bool ScamFeed { get => scamFeed; set { if (Set(ref scamFeed, value)) store.Set("fishcatcher.feed", value); } }

    private bool passkeys;
    /// Whether sites may ask for a passkey here. Off sends them to the
    /// password instead.
    public bool Passkeys { get => passkeys; set { if (Set(ref passkeys, value)) store.Set("passkeys", value); } }
    public bool PasskeysPossible => true;

    private string downloads;
    public string Downloads { get => downloads; set { if (Set(ref downloads, value)) store.Set("downloads", value); } }

    private bool asksWhereToSave;
    public bool AsksWhereToSave { get => asksWhereToSave; set { if (Set(ref asksWhereToSave, value)) store.Set("downloads.ask", value); } }

    private bool savesPasswords;
    /// Offer to keep a password the first time a site sees it.
    public bool SavesPasswords { get => savesPasswords; set { if (Set(ref savesPasswords, value)) store.Set("passwords.save", value); } }

    private bool fillsPasswords;
    /// The accounts kept for a site hang from its sign-in box.
    public bool FillsPasswords { get => fillsPasswords; set { if (Set(ref fillsPasswords, value)) store.Set("passwords.fill", value); } }

    private bool welcomed;
    /// The first launch has been walked through. Until then the welcome
    /// stands over the window.
    public bool Welcomed { get => welcomed; set { if (Set(ref welcomed, value)) store.Set("welcomed", value); } }

    private bool spelling;
    /// Misspelt words underlined in a page's text fields, with the fix on a
    /// right-click — Windows' own spelling checker. On unless turned off.
    public bool Spelling { get => spelling; set { if (Set(ref spelling, value)) store.Set("spelling", value); } }

    private bool usesSpaces;
    /// Separate sets of tabs, each with its own sign-ins (see Spaces).
    /// Off unless asked for.
    public bool UsesSpaces { get => usesSpaces; set { if (Set(ref usesSpaces, value)) store.Set("spaces", value); } }

    private IReadOnlyList<string> searchFolders;
    /// The folders whose files the field finds (Settings › Search). None
    /// until some are chosen: nothing on the PC is indexed before that. A
    /// new list on every change, so the engine's threads can read it as is.
    public IReadOnlyList<string> SearchFolders
    {
        get => searchFolders;
        set
        {
            if (searchFolders.SequenceEqual(value, StringComparer.OrdinalIgnoreCase)) return;
            searchFolders = [.. value];
            store.Set("files.folders", searchFolders);
            Tell();
        }
    }

    private bool fileContents;
    /// Those files found by what's inside them too, not only by name. On
    /// unless turned off.
    public bool FileContents { get => fileContents; set { if (Set(ref fileContents, value)) store.Set("files.contents", value); } }

    private bool appsInField;
    /// Installed apps among the field's suggestions. On unless turned off.
    public bool AppsInField { get => appsInField; set { if (Set(ref appsInField, value)) store.Set("field.apps", value); } }
}

public static class KnownFolders
{
    private static readonly Guid DownloadsId = new("374DE290-123F-4565-9164-39C4925E467B");

    [System.Runtime.InteropServices.DllImport("shell32.dll", CharSet = System.Runtime.InteropServices.CharSet.Unicode)]
    private static extern int SHGetKnownFolderPath(in Guid id, uint flags, IntPtr token, out string path);

    /// The Downloads folder where the person keeps it, which is not always
    /// under their profile.
    public static string Downloads
    {
        get
        {
            try
            {
                if (SHGetKnownFolderPath(DownloadsId, 0, IntPtr.Zero, out var path) == 0) return path;
            }
            catch { }
            return Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), "Downloads");
        }
    }
}
