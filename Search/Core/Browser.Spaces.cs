namespace Search;

// Spaces: separate rows of tabs, each with its own sign-ins or sharing the
// first one's. Off unless turned on in Settings › Tabs (see Space for the
// whole idea).
public sealed partial class Browser
{
    private List<Space> spaces = Space.Read();
    /// Every space, the first one first; their order is Alt+1–Alt+9's.
    public List<Space> Spaces
    {
        get => spaces;
        set
        {
            spaces = value;
            Space.Sharing = [.. value.Where(s => s.SharesSignIns == true).Select(s => s.Id)];
            Tell();
            Tell(nameof(CurrentSpace));
        }
    }

    private Guid spaceID = Space.FirstID;
    /// The space on screen.
    public Guid SpaceID { get => spaceID; set { if (Set(ref spaceID, value)) Tell(nameof(CurrentSpace)); } }

    /// Every other space's row, as it was left.
    public Dictionary<Guid, Parked> ParkedRows { get; } = [];
    /// Every tab of the spaces not on screen, for the sleep timer.
    public IEnumerable<Tab> ParkedTabs => ParkedRows.Values.SelectMany(p => p.Tabs);

    private bool makingSpace;
    /// The card for a new space stands in for the rows (see NewSpaceCard).
    public bool MakingSpace { get => makingSpace; set => Set(ref makingSpace, value); }

    private int spaceStep = 1;
    /// Which way the last change of space went: 1 to the next, -1 back. The
    /// dot turns over that way.
    public int SpaceStep { get => spaceStep; set => Set(ref spaceStep, value); }

    private double swipeTravel;
    /// How far two fingers have come sideways over the column (see SpaceSwipe).
    public double SwipeTravel { get => swipeTravel; set => Set(ref swipeTravel, value); }

    public Space CurrentSpace => Spaces.FirstOrDefault(s => s.Id == SpaceID) ?? Spaces[0];

    /// A space's own downloads folder, when it has one.
    public string? SpaceDownloads => Prefs.UsesSpaces ? CurrentSpace.Downloads : null;

    private void StartSpaces()
    {
        // What a deleted space left behind, if WebView2 wouldn't let it go then.
        Space.Sweep();
        Space.Sharing = [.. Spaces.Where(s => s.SharesSignIns == true).Select(s => s.Id)];
        // The space you were in, when there are spaces.
        if (Prefs.UsesSpaces && Guid.TryParse(Store.Settings.String("space.current"), out var last) && Spaces.Any(s => s.Id == last))
        {
            spaceID = last;
            Space.Current = last;
        }
        // The other rows once this one is up: the session restored next is
        // this space's, and the first frame never waits on the others.
        UI.Soon(() => { if (Prefs.UsesSpaces) PreloadSpaces(); });
    }

    /// Alt+1–Alt+9, and the menu on the space's dot.
    public void SwitchSpace(Guid id)
    {
        if (!Prefs.UsesSpaces) return;
        Enter(id);
    }

    public void SwitchSpace(int index)
    {
        if (index < 0 || index >= Spaces.Count) return;
        SwitchSpace(Spaces[index].Id);
    }

    private int IndexOfSpace(Guid id) => Spaces.FindIndex(s => s.Id == id);

    private void Enter(Guid id)
    {
        var to = IndexOfSpace(id);
        if (id == SpaceID || to < 0) return;
        // Which way the icon at the foot turns over: the way the spaces lie.
        if (!MakingSpace) SpaceStep = to > Math.Max(0, IndexOfSpace(SpaceID)) ? 1 : -1;
        CancelTabEdit();
        if (Floating != null) Land();
        WriteSession(now: true);

        // The row on screen is parked as it is. Its sound stops: a space you
        // left is not one you are listening to.
        foreach (var tab in Tabs)
        {
            if (tab.Core is { } core) _ = core.ExecuteScriptAsync(Hush);
            tab.Show(false);
        }
        ParkedRows[SpaceID] = new Parked(Tabs, ActiveID);

        SpaceID = id;
        Space.Current = id;
        Store.Settings.Set("space.current", id.ToString().ToUpperInvariant());
        if (ParkedRows.Remove(id, out var back) && back.Tabs.Count > 0)
        {
            ShowRow(back.Tabs, back.Active);
        }
        else
        {
            ShowRow([], null);
            RestoreSession();
        }
        Summoning = false;
        Editing = Active?.IsBlank ?? true;
        Typed = "";
        AskFocus();
        Announce(CurrentSpace.Name);
    }

    /// Every sound and picture on a page, paused.
    private const string Hush = "document.querySelectorAll('video, audio').forEach(function (m) { try { m.pause(); } catch (e) {} });";

    /// A space's row as its session left it, made without touching the one
    /// on screen: tabs with an address and no page yet, which cost next to
    /// nothing until one is looked at.
    private Parked LoadRow(Guid space)
    {
        var saved = Session.Read(space);
        var row = new List<Tab>();
        foreach (var entry in saved.Tabs)
        {
            if (!Uri.TryCreate(entry.Url, UriKind.Absolute, out var url)) continue;
            var tab = new Tab(profile: Space.Profile(space));
            Prepare(tab);
            tab.Restore(url, entry.Title, entry.Name);
            tab.Pin = entry.Pin;
            row.Add(tab);
        }
        var active = saved.Active >= 0 && saved.Active < row.Count ? row[saved.Active].Id : row.FirstOrDefault()?.Id;
        return new Parked(row, active);
    }

    /// Another space's row put on screen in place of this one — empty, for
    /// one that restores its own. The tab it was on opens as any tab you go
    /// to does: woken if it was brought back from last time, reloaded if its
    /// page died while it was parked.
    private void ShowRow(List<Tab> row, Guid? active)
    {
        Tabs = row;
        TellTabs();
        var target = row.FirstOrDefault(t => t.Id == active) ?? row.FirstOrDefault();
        var woke = target?.Wake() ?? false;
        ActiveID = target?.Id;
        target?.Touch();
        if (target != null && !woke) target.Revive();
    }

    /// Every other space's row, made ahead of time, so the one you go to is
    /// there the moment you do.
    public void PreloadSpaces()
    {
        foreach (var space in Spaces)
            if (space.Id != SpaceID && !ParkedRows.ContainsKey(space.Id))
                ParkedRows[space.Id] = LoadRow(space.Id);
    }

    /// The icon a new space gets unless told: the first no space wears yet.
    public string FreeIcon
    {
        get
        {
            var used = Spaces.Select(s => s.Symbol).ToHashSet();
            return Space.Icons.FirstOrDefault(i => !used.Contains(i)) ?? "briefcase";
        }
    }

    /// A new space, empty, and on screen — signed in where the others are,
    /// or starting afresh with its own cookies and sign-ins.
    public void AddSpace(string name, string? icon = null, bool sharesSignIns = true)
    {
        MakingSpace = false;
        var made = new Space { Id = Guid.NewGuid(), Name = name, Colour = 0, Icon = icon ?? FreeIcon, SharesSignIns = sharesSignIns };
        Spaces = [.. Spaces, made];
        Space.Write(Spaces);
        SwitchSpace(made.Id);
    }

    /// Moved to another place among the spaces. Alt+1–Alt+9 follow the order.
    public void MoveSpace(Guid id, int index)
    {
        var from = IndexOfSpace(id);
        if (from < 0 || index < 0 || index >= Spaces.Count || from == index) return;
        var list = Spaces.ToList();
        var moving = list[from];
        list.RemoveAt(from);
        list.Insert(index, moving);
        Spaces = list;
        Space.Write(Spaces);
    }

    /// "New Space…": the card in the column when the column is there to
    /// hold it, a question otherwise.
    public void AskForSpace()
    {
        if (Prefs.Sidebar && (!Folded || Peeking)) MakingSpace = true;
        else SpaceAsk.NewSpace((name, shared) => AddSpace(name, sharesSignIns: shared));
    }

    public void RenameSpace(Guid id, string name)
    {
        if (Spaces.FirstOrDefault(s => s.Id == id) is not { } space || name.Trim().Length == 0) return;
        space.Name = name.Trim();
        Spaces = [.. Spaces];
        Space.Write(Spaces);
    }

    public void SetSpaceIcon(Guid id, string icon)
    {
        if (Spaces.FirstOrDefault(s => s.Id == id) is not { } space) return;
        space.Icon = icon;
        Spaces = [.. Spaces];
        Space.Write(Spaces);
    }

    public void SetSpaceDownloads(Guid id, string? folder)
    {
        if (Spaces.FirstOrDefault(s => s.Id == id) is not { } space) return;
        space.Downloads = folder;
        Spaces = [.. Spaces];
        Space.Write(Spaces);
    }

    /// A space, its tabs, and its cookies and sign-ins, gone. The first one
    /// stays: it is where everything was before there were spaces.
    public void DeleteSpace(Guid id)
    {
        var at = IndexOfSpace(id);
        if (id == Space.FirstID || at < 0) return;
        if (SpaceID == id) Enter(Space.FirstID);
        var row = ParkedRows.Remove(id, out var parked) ? parked.Tabs : [];
        // A space signed in with the others has nothing of its own to erase:
        // its cookies are theirs.
        var shared = Spaces[at].SharesSignIns == true;
        Spaces = [.. Spaces.Where(s => s.Id != id)];
        Space.Write(Spaces);
        Session.Erase(id);
        if (shared)
        {
            foreach (var tab in row) tab.Close();
            return;
        }
        _ = Erase(id, row);
    }

    /// Emptied through one of its own pages while there still is one — the
    /// only handle WebView2 gives on a profile — then its tabs closed and
    /// its folder swept away.
    private static async Task Erase(Guid id, List<Tab> row)
    {
        if (row.Select(t => t.Core).OfType<Microsoft.Web.WebView2.Core.CoreWebView2>().FirstOrDefault() is { } core)
        {
            try { await core.Profile.ClearBrowsingDataAsync(); } catch { }
        }
        foreach (var tab in row) tab.Close();
        Space.Erase(id);
    }

    /// Spaces turned off: back to the first one. The others are kept, in
    /// case they are turned on again.
    public void LeaveSpaces()
    {
        MakingSpace = false;
        Enter(Space.FirstID);
        foreach (var row in ParkedRows.Values)
            foreach (var tab in row.Tabs) tab.Close();
        ParkedRows.Clear();
    }
}
