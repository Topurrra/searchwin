using System.Diagnostics;

namespace Search;

// What you have kept. Downloads work already; this is only the memory of them,
// so a file you fetched an hour ago is one click from Explorer rather than a
// hunt through a folder.

public sealed class Keep
{
    public string Name { get; set; } = "";
    public string From { get; set; } = "";
    public string Path { get; set; } = "";
    public DateTime Date { get; set; }

    public string Id => Path;
    public bool StillThere => File.Exists(Path);
}

public sealed class Loot
{
    public List<Keep> Kept { get; private set; } = [];
    public event Action? Changed;

    public Loot() => Kept = Store.Read<List<Keep>>("downloads.json") ?? [];

    public void Add(Keep keep)
    {
        Kept.RemoveAll(k => k.Path == keep.Path);
        Kept.Insert(0, keep);
        // Fifty is more than anybody scrolls back through.
        if (Kept.Count > 50) Kept.RemoveRange(50, Kept.Count - 50);
        Save();
    }

    public void Forget(Keep keep)
    {
        Kept.RemoveAll(k => k.Id == keep.Id);
        Save();
    }

    /// Only the list is emptied. Files you asked for are yours, and deleting
    /// them is Explorer's business, not a browser's.
    public void ForgetAll()
    {
        Kept = [];
        Save();
    }

    public static void Reveal(Keep keep)
    {
        try { Process.Start(new ProcessStartInfo("explorer.exe", $"/select,\"{keep.Path}\"") { UseShellExecute = true }); } catch { }
    }

    public static void Open(Keep keep)
    {
        try { Process.Start(new ProcessStartInfo(keep.Path) { UseShellExecute = true }); } catch { }
    }

    private void Save()
    {
        Changed?.Invoke();
        Store.Write("downloads.json", Kept);
    }
}
