namespace Search;

// Spaces: separate rows of tabs, each with its own sign-ins or sharing the
// first one's. Off unless turned on in Settings › Tabs.
//
// PORT: see Sources/Search/Spaces.swift and SpaceSwipe.swift.
public sealed partial class Browser
{
    private List<Space> spaces = Space.Read();
    public List<Space> Spaces { get => spaces; set { spaces = value; Space.Sharing = [.. value.Where(s => s.SharesSignIns == true).Select(s => s.Id)]; Tell(); } }

    private Guid spaceID = Space.FirstID;
    public Guid SpaceID { get => spaceID; set => Set(ref spaceID, value); }

    public Dictionary<Guid, Parked> ParkedRows { get; } = [];
    public IEnumerable<Tab> ParkedTabs => ParkedRows.Values.SelectMany(p => p.Tabs);

    private bool makingSpace;
    public bool MakingSpace { get => makingSpace; set => Set(ref makingSpace, value); }

    public Space CurrentSpace => Spaces.FirstOrDefault(s => s.Id == SpaceID) ?? Spaces[0];

    /// A space's own downloads folder, when it has one.
    public string? SpaceDownloads => Prefs.UsesSpaces ? CurrentSpace.Downloads : null;

    private void StartSpaces()
    {
        Space.Sharing = [.. Spaces.Where(s => s.SharesSignIns == true).Select(s => s.Id)];
    }

    public void PreloadSpaces() { }
    public void LeaveSpaces() { }
    public void SwitchSpace(int index) { }
}
