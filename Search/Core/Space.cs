namespace Search;

/// A set of tabs of its own, signed in where the others are or starting
/// afresh (see Browser.Spaces). The first one is the browser there always
/// was, and its tabs are the ones in session.json.
public sealed class Space
{
    public Guid Id { get; set; }
    public string Name { get; set; } = "";
    public int Colour { get; set; }
    public string? Icon { get; set; }
    /// Signed in wherever the first space is, rather than afresh.
    public bool? SharesSignIns { get; set; }
    /// A downloads folder of its own, when it has one.
    public string? Downloads { get; set; }

    public static readonly Guid FirstID = new("00000000-0000-0000-0000-000000000001");
    public bool IsFirst => Id == FirstID;

    public static List<Space> Read()
    {
        var list = Store.Read<List<Space>>("spaces.json");
        return list is { Count: > 0 } ? list : [new Space { Id = FirstID, Name = "Home", Colour = 0, Icon = "house" }];
    }

    public static void Write(List<Space> spaces) => Store.Write("spaces.json", spaces);

    /// The space on screen, for tabs that are made without being told.
    public static Guid Current = FirstID;
    /// The spaces signed in where the first one is: their tabs use its profile.
    public static HashSet<Guid> Sharing = [];

    /// The WebView2 profile a space's tabs live in.
    public static string Profile(Guid id) =>
        id == FirstID || Sharing.Contains(id) ? "Default" : "space-" + id.ToString("N");
}

/// Another space's row, as it was left.
public sealed class Parked(List<Tab> tabs, Guid? active)
{
    public List<Tab> Tabs { get; } = tabs;
    public Guid? Active { get; set; } = active;
}
