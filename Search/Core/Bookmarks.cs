namespace Search;

// Bookmarks: folders and sites, kept in a small file.
//
// Shown as the app's own kind of list rather than a system menu, on purpose:
// a menu can't be dragged into, and can't be asked a second thing by
// right-clicking it. A folder you actually keep things in wants both.

public sealed class Bookmark
{
    public Guid Id { get; set; } = Guid.NewGuid();
    public string Title { get; set; } = "";
    /// Null for a folder.
    public string? Url { get; set; }
    public List<Bookmark>? Children { get; set; }

    public bool IsFolder => Url == null;

    public string? Host => Url != null && Uri.TryCreate(Url, UriKind.Absolute, out var u) ? Address.Host(u) : null;

    public static Bookmark Site(string title, Uri url) =>
        new() { Title = title.Length == 0 ? Address.Pretty(url) : title, Url = url.AbsoluteUri };

    public static Bookmark Folder(string title, List<Bookmark> children) =>
        new() { Title = title, Url = null, Children = children };
}

public sealed class Bookmarks
{
    public List<Bookmark> Roots { get; private set; } = [];
    public event Action? Changed;

    public Bookmarks() => Load();

    public bool IsEmpty => Roots.Count == 0;

    /// How many sites, folders opened.
    public int Count => CountOf(Roots);

    public static int CountOf(IEnumerable<Bookmark> nodes) =>
        nodes.Sum(n => n.IsFolder ? CountOf(n.Children ?? []) : 1);

    /// Every site in the list, in order, folders opened.
    public static IEnumerable<Uri> Urls(IEnumerable<Bookmark> nodes)
    {
        foreach (var node in nodes)
        {
            if (node.IsFolder) { foreach (var u in Urls(node.Children ?? [])) yield return u; }
            else if (Uri.TryCreate(node.Url, UriKind.Absolute, out var url)) yield return url;
        }
    }

    /// Every folder in the tree, each with how deep it sits — for "move to
    /// folder" lists, where a folder three deep should look like it.
    public static IEnumerable<(Bookmark node, int depth)> Folders(IEnumerable<Bookmark> nodes, int depth = 0)
    {
        foreach (var node in nodes.Where(n => n.IsFolder))
        {
            yield return (node, depth);
            foreach (var inner in Folders(node.Children ?? [], depth + 1)) yield return inner;
        }
    }

    // MARK: - changing

    /// The page, at the end of the list. Nothing is asked: the title is the
    /// page's, and filing it into a folder is a drag or a right-click away.
    public void Add(Uri url, string title)
    {
        if (Contains(url)) return;
        Roots.Add(Bookmark.Site(title, url));
        Save();
    }

    public bool Contains(Uri url)
    {
        bool Walk(IEnumerable<Bookmark> nodes) => nodes.Any(n => n.Url == url.AbsoluteUri || Walk(n.Children ?? []));
        return Walk(Roots);
    }

    public void Remove(Guid id)
    {
        Roots = Prune(id, Roots);
        Save();
    }

    private static List<Bookmark> Prune(Guid id, List<Bookmark> nodes)
    {
        var kept = new List<Bookmark>();
        foreach (var node in nodes)
        {
            if (node.Id == id) continue;
            if (node.Children != null) node.Children = Prune(id, node.Children);
            kept.Add(node);
        }
        return kept;
    }

    /// Takes a bookmark or a whole folder out of wherever it currently sits
    /// and puts it at the end of another folder's children — or back at the
    /// top level when `folder` is null. Moving a folder into its own children
    /// is refused rather than allowed to erase it by looping it inside itself.
    public void Move(Guid id, Guid? folder)
    {
        if (id == folder) return;
        var node = Find(id, Roots);
        if (node == null) return;
        if (folder is { } f)
        {
            if (Holds(f, node)) return;
            if (Find(f, Roots) is not { IsFolder: true } target) return;
            Detach(id, Roots);
            (target.Children ??= []).Add(node);
        }
        else
        {
            Detach(id, Roots);
            Roots.Add(node);
        }
        Save();
    }

    public static Bookmark? Find(Guid id, IEnumerable<Bookmark> nodes)
    {
        foreach (var node in nodes)
        {
            if (node.Id == id) return node;
            if (node.Children != null && Find(id, node.Children) is { } found) return found;
        }
        return null;
    }

    private static Bookmark? Detach(Guid id, List<Bookmark> nodes)
    {
        for (var i = 0; i < nodes.Count; i++)
        {
            if (nodes[i].Id == id)
            {
                var found = nodes[i];
                nodes.RemoveAt(i);
                return found;
            }
            if (nodes[i].Children is { } kids && Detach(id, kids) is { } inner) return inner;
        }
        return null;
    }

    /// `id` is `node` itself, or somewhere inside it — also used by the
    /// outline to keep a folder out of its own "move to" list.
    public static bool Holds(Guid id, Bookmark node) =>
        node.Id == id || (node.Children ?? []).Any(c => Holds(id, c));

    /// Another browser's, kept apart in a folder of that browser's name unless
    /// there was nothing here yet.
    public void Take(List<Bookmark> nodes, string name)
    {
        if (nodes.Count == 0) return;
        if (Roots.Count == 0) Roots = nodes;
        else
        {
            Roots.RemoveAll(n => n.IsFolder && n.Title == name);
            Roots.Add(Bookmark.Folder(name, nodes));
        }
        Save();
    }

    // MARK: - for extensions

    /// A page or a folder filed under `parent`, or at the top level for null
    /// or a folder that isn't there. What chrome.bookmarks.create does.
    public Bookmark Insert(Bookmark node, Guid? parent)
    {
        if (parent is { } p && Find(p, Roots) is { IsFolder: true } folder) (folder.Children ??= []).Add(node);
        else Roots.Add(node);
        Save();
        return node;
    }

    /// A new title or address for one that is kept. chrome.bookmarks.update.
    public void Update(Guid id, string? title, string? url)
    {
        if (Find(id, Roots) is not { } node) return;
        if (title != null) node.Title = title;
        if (url != null && !node.IsFolder) node.Url = url;
        Save();
    }

    // MARK: - the file

    private const string FileName = "bookmarks.json";

    private void Load() => Roots = Store.Read<List<Bookmark>>(FileName) ?? [];

    private void Save()
    {
        Changed?.Invoke();
        Store.Write(FileName, Roots);
    }
}
