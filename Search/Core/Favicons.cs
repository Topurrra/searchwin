using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using Microsoft.Web.WebView2.Core;

namespace Search;

// A site's own icon, for the tabs that are set to wear one.
//
// WebView2 knows which icon a page declared and hands it over as a PNG, so
// there is no guessing and no second fetch. It is kept as a small file next
// to the history: a tab brought back from yesterday's session has its icon
// before it has a page; a tab on a site never seen before shows a letter
// until the icon arrives, which is a second or so.
public sealed class Favicons
{
    public static readonly Favicons Shared = new();

    /// Called with a host and its icon whenever one arrives, so every tab on
    /// that host can put it on at once.
    public Action<string, ImageSource>? Arrived;

    private readonly Dictionary<string, ImageSource> memory = [];
    private readonly HashSet<string> busy = [];

    private static string Folder => Path.Combine(Store.Folder, "icons");
    private static string FileFor(string host) => Path.Combine(Folder, host + ".png");

    /// What is already known, and nothing fetched.
    public ImageSource? Cached(string host)
    {
        if (memory.TryGetValue(host, out var hit)) return hit;
        var file = FileFor(host);
        if (!File.Exists(file)) return null;
        var image = new BitmapImage { DecodePixelWidth = 32, UriSource = new Uri(file) };
        memory[host] = image;
        return image;
    }

    /// An icon from somewhere else — another browser's cache, at import —
    /// kept as if the site had handed it over, unless one is already here.
    public async Task Adopt(byte[] png, string host)
    {
        if (Cached(host) != null || png.Length < 60) return;
        await Keep(png, host, shy: false);
    }

    /// Asks the page for the icon it declared, and keeps it.
    public void Fetch(Tab tab)
    {
        if (tab.Core is not { } core || Address.Host(tab.Address) is not { } host || !Address.IsWeb(tab.Address)) return;
        if (!busy.Add(host)) return;
        _ = Take(core, host, tab.Shy);
    }

    private async Task Take(CoreWebView2 core, string host, bool shy)
    {
        try
        {
            if (string.IsNullOrEmpty(core.FaviconUri)) return;
            using var stream = await core.GetFaviconAsync(CoreWebView2FaviconImageFormat.Png);
            if (stream == null || stream.Size < 60) return;
            var bytes = new byte[stream.Size];
            using (var reader = new Windows.Storage.Streams.DataReader(stream.GetInputStreamAt(0)))
            {
                await reader.LoadAsync((uint)stream.Size);
                reader.ReadBytes(bytes);
            }
            await Keep(bytes, host, shy);
        }
        catch { }
        finally { busy.Remove(host); }
    }

    private async Task Keep(byte[] png, string host, bool shy)
    {
        var image = new BitmapImage { DecodePixelWidth = 32 };
        using (var memoryStream = new Windows.Storage.Streams.InMemoryRandomAccessStream())
        {
            using (var writer = new Windows.Storage.Streams.DataWriter(memoryStream.GetOutputStreamAt(0)))
            {
                writer.WriteBytes(png);
                await writer.StoreAsync();
                await writer.FlushAsync();
            }
            memoryStream.Seek(0);
            await image.SetSourceAsync(memoryStream);
        }
        memory[host] = image;
        // A private tab's icons are not written down: they would say where it
        // had been.
        if (!shy)
        {
            var file = FileFor(host);
            _ = Task.Run(() =>
            {
                Directory.CreateDirectory(Folder);
                Store.WriteAtomic(file, png);
            });
        }
        Arrived?.Invoke(host, image);
    }
}
