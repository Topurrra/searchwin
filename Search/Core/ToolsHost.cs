using System.Diagnostics;
using System.Text.Json.Nodes;
using Microsoft.UI.Xaml.Controls;
using Microsoft.Web.WebView2.Core;
using SearchKit.Engine;

namespace Search;

/// Workspace's tools, as pages in tabs.
///
/// The pages are a static site (Tools/, built to `tools` beside Search.exe)
/// that every tab can reach as https://tools.search/ — type `search://tools`,
/// or `search://tools/<id>` for one tool. What they used Tauri for arrives
/// here as messages: `host:…` calls the browser answers itself (dialogs,
/// files, paths), and everything else goes on to the engine by the name the
/// page used. The engine's events go back to every tool page.
///
/// Only a page actually loaded from tools.search is answered, and a web page
/// can't send a tab there: only Search's own navigations, and back, forward
/// and reload, can.
public static class ToolsHost
{
    public const string Host = "tools.search";
    private const string FilesHost = "files.search";

    public static bool IsTools(Uri? url) =>
        url is { IsAbsoluteUri: true } && url.Scheme == "https" && url.Host == Host;

    private static bool IsTools(string? url) => Uri.TryCreate(url, UriKind.Absolute, out var u) && IsTools(u);

    /// `search://tools` and `search://tools/<id>` → the page.
    /// `search://play?path=<encoded>` → the media player, for one file —
    /// never a network share or a device path outside the chosen folders.
    public static Uri? Resolve(string typed) => SearchKit.Web.ToolAddress.Resolve(typed, path => Refused(path) != null);

    /// What the field shows for a tool page, or the player.
    public static string Pretty(Uri url) => SearchKit.Web.ToolAddress.Pretty(url);

    /// The folders chosen in Settings › Search: the only place a tool page
    /// may reach a network share.
    private static IReadOnlyList<string> Chosen => App.Window?.Browser.Prefs.SearchFolders ?? [];

    private static string? Refused(string path) => SearchKit.Web.ToolGate.PathRefused(path, Chosen);

    /// The built pages: beside Search.exe, or (a test run out of the repo)
    /// Tools/dist.
    private static readonly string? Folder = FindFolder();

    private static string? FindFolder()
    {
        var beside = Path.Combine(AppContext.BaseDirectory, "tools");
        if (File.Exists(Path.Combine(beside, "index.html"))) return beside;
        if (!Store.Testing) return null;
        for (var dir = new DirectoryInfo(AppContext.BaseDirectory); dir != null; dir = dir.Parent)
        {
            var built = Path.Combine(dir.FullName, "Tools", "dist");
            if (File.Exists(Path.Combine(built, "index.html"))) return built;
        }
        return null;
    }

    /// The tools the pages list (their build's catalog.json), for the field.
    public static IReadOnlyList<SearchKit.Commands.ToolEntry> Catalog()
    {
        try
        {
            return Folder is { } folder && Path.Combine(folder, "catalog.json") is var file && File.Exists(file)
                ? SearchKit.Commands.ToolCatalog.Parse(File.ReadAllText(file))
                : [];
        }
        catch (IOException) { return []; }
        catch (UnauthorizedAccessException) { return []; }
    }

    /// Every tab: the pages are reachable, and files a tool page asks to show
    /// are served to it.
    public static void Prepare(CoreWebView2 core)
    {
        if (Folder is { } folder)
            core.SetVirtualHostNameToFolderMapping(Host, folder, CoreWebView2HostResourceAccessKind.DenyCors);
        core.AddWebResourceRequestedFilter($"https://{FilesHost}/*", CoreWebView2WebResourceContext.All);
        core.WebResourceRequested += (_, e) =>
        {
            if (!e.Request.Uri.StartsWith($"https://{FilesHost}/", StringComparison.OrdinalIgnoreCase)) return;
            ServeFile(core, e);
        };
    }

    /// May this navigation happen? Anything but tools.search, yes. Into
    /// tools.search: when Search itself asked, from a tool page, or going
    /// back, forward or reloading. `document` is the page the tab really
    /// has (see Tab.Document), not the address it shows.
    public static bool MayOpen(Uri? document, CoreWebView2NavigationStartingEventArgs e, bool ours) =>
        SearchKit.Web.ToolGate.MayNavigate(IsTools(e.Uri), IsTools(document), ours,
            e.NavigationKind == CoreWebView2NavigationKind.NewDocument);

    /// May a page's new window (window.open, a target=_blank link) open
    /// this? Never a tool page.
    public static bool MayOpenWindow(string? url) => SearchKit.Web.ToolGate.MayOpenWindow(IsTools(url));

    // MARK: - files a tool page shows

    private static async void ServeFile(CoreWebView2 core, CoreWebView2WebResourceRequestedEventArgs e)
    {
        if (!IsTools(core.Source))
        {
            e.Response = core.Environment.CreateWebResourceResponse(null, 403, "Forbidden", "");
            return;
        }
        // fetch() with a Range the browser doesn't consider simple (a suffix,
        // "the last N bytes") asks first.
        if (e.Request.Method == "OPTIONS")
        {
            e.Response = core.Environment.CreateWebResourceResponse(null, 204, "No Content",
                $"{ToolsOnly}\r\nAccess-Control-Allow-Methods: GET, HEAD\r\nAccess-Control-Allow-Headers: Range\r\nAccess-Control-Max-Age: 600");
            return;
        }
        var query = new Uri(e.Request.Uri).Query.TrimStart('?').Split('&')
            .Select(pair => pair.Split('=', 2))
            .FirstOrDefault(pair => pair[0] == "path");
        var path = query is { Length: 2 } ? Uri.UnescapeDataString(query[1]) : null;
        // Whether it's there is the file system's to say, off this thread:
        // a missing file is the 404 below, when opening it fails.
        if (string.IsNullOrEmpty(path) || Refused(path) != null)
        {
            e.Response = core.Environment.CreateWebResourceResponse(null, path == null ? 404 : 403, path == null ? "Not Found" : "Forbidden", ToolsOnly);
            return;
        }
        var range = e.Request.Headers.Contains("Range") ? e.Request.Headers.GetHeader("Range") : null;
        using var deferral = e.GetDeferral();
        try
        {
            var file = await Windows.Storage.StorageFile.GetFileFromPathAsync(path);
            var size = (await file.GetBasicPropertiesAsync()).Size;
            var type = string.IsNullOrEmpty(file.ContentType) ? "application/octet-stream" : file.ContentType;
            if (SearchKit.Web.ByteRange.Parse(range, size) is var (start, end))
            {
                // A video seeking asks for a piece: answer with at most one
                // chunk of it, and the player asks for the next.
                using var input = await file.OpenReadAsync();
                input.Seek(start);
                var buffer = new Windows.Storage.Streams.Buffer((uint)(end - start + 1));
                await input.ReadAsync(buffer, buffer.Capacity, Windows.Storage.Streams.InputStreamOptions.None);
                var piece = new Windows.Storage.Streams.InMemoryRandomAccessStream();
                await piece.WriteAsync(buffer);
                piece.Seek(0);
                e.Response = core.Environment.CreateWebResourceResponse(piece, 206, "Partial Content",
                    $"Content-Type: {type}\r\nContent-Range: bytes {start}-{start + buffer.Length - 1}/{size}\r\n" +
                    $"Content-Length: {buffer.Length}\r\nAccept-Ranges: bytes\r\nCache-Control: no-store\r\n{ToolsOnly}");
                return;
            }
            var stream = await file.OpenReadAsync();
            e.Response = core.Environment.CreateWebResourceResponse(stream, 200, "OK",
                $"Content-Type: {type}\r\nContent-Length: {size}\r\nAccept-Ranges: bytes\r\nCache-Control: no-store\r\n{ToolsOnly}");
        }
        catch (Exception error) when (error is FileNotFoundException or DirectoryNotFoundException or ArgumentException)
        {
            e.Response = core.Environment.CreateWebResourceResponse(null, 404, "Not Found", ToolsOnly);
        }
        catch
        {
            e.Response = core.Environment.CreateWebResourceResponse(null, 500, "Unreadable", ToolsOnly);
        }
    }

    /// Tool pages (and only they) may read these with fetch(), not just show
    /// them in an <img> or <video>.
    private const string ToolsOnly =
        "Access-Control-Allow-Origin: https://" + Host + "\r\nAccess-Control-Expose-Headers: Content-Range, Content-Length, Accept-Ranges";

    // MARK: - the bridge

    private static bool listening;

    /// A message from a tab. True when it was a tool page's, and handled.
    public static bool Take(Tab tab, CoreWebView2 core, CoreWebView2WebMessageReceivedEventArgs e)
    {
        if (!IsTools(e.Source)) return false;
        JsonObject? message;
        try { message = JsonNode.Parse(e.WebMessageAsJson) as JsonObject; }
        catch { return false; }
        if (message?["kind"]?.GetValue<string>() != "invoke") return false;

        if (!listening)
        {
            listening = true;
            Engine.Client.EventReceived += (name, payload) => UI.Do(() => Broadcast(name, payload));
        }

        var id = message["id"]?.DeepClone();
        var cmd = message["cmd"]?.GetValue<string>() ?? "";
        var args = message["args"] as JsonObject ?? [];
        // Files dropped on the page arrive as File objects beside the
        // message; where they are is only the browser's to read.
        if (cmd == "host:drop.paths")
            args = new JsonObject { ["paths"] = new JsonArray([.. Dropped(e).Select(path => (JsonNode)path)]) };
        // A network share or a device path, anywhere a path goes: refused
        // unless it's inside a folder chosen in Settings › Search.
        var refused = SearchKit.Web.ToolGate.ArgsRefused(args, Chosen);
        _ = Answer(core, id, cmd, args, refused);
        return true;
    }

    private static List<string> Dropped(CoreWebView2WebMessageReceivedEventArgs e)
    {
        var paths = new List<string>();
        try
        {
            if (e.AdditionalObjects is { } objects)
                foreach (var item in objects)
                    if (item != null && WinRT.CastExtensions.As<CoreWebView2File>(item).Path is { Length: > 0 } path)
                        paths.Add(path);
        }
        catch (Exception error) when (error is InvalidCastException or System.Runtime.InteropServices.COMException)
        {
            Log.Write($"tools: dropped files: {error.Message}");
        }
        return paths;
    }

    private static async Task Answer(CoreWebView2 core, JsonNode? id, string cmd, JsonObject args, string? refused)
    {
        JsonObject reply;
        try
        {
            // Some engine commands are the browser's alone (the clipboard listener, its pause).
            if (SearchKit.Web.ToolCalls.Refused(cmd) is { } why) throw new InvalidOperationException(why);
            if (refused != null) throw new InvalidOperationException(refused);
            var value = cmd.StartsWith("host:")
                ? await Local(cmd[5..], args)
                : await Engine.Client.CallAsync(cmd, args);
            reply = new JsonObject { ["kind"] = "reply", ["id"] = id, ["ok"] = true, ["value"] = value };
        }
        catch (EngineException error)
        {
            reply = new JsonObject { ["kind"] = "reply", ["id"] = id, ["ok"] = false, ["error"] = error.Detail ?? error.Message };
        }
        catch (Exception error)
        {
            reply = new JsonObject { ["kind"] = "reply", ["id"] = id, ["ok"] = false, ["error"] = error.Message };
        }
        var json = reply.ToJsonString();
        UI.Do(() =>
        {
            try { core.PostWebMessageAsJson(json); } catch { }
        });
    }

    /// An event from the browser itself (a pack came or went, the player's
    /// progress) to every open tool page. On the UI thread.
    public static void Tell(string name, JsonNode? payload) => Broadcast(name, payload);

    /// An event to every open tool page.
    private static void Broadcast(string name, JsonNode? payload)
    {
        if (App.Window?.Browser is not { } browser) return;
        var json = new JsonObject { ["kind"] = "event", ["event"] = name, ["payload"] = payload?.DeepClone() }.ToJsonString();
        foreach (var tab in browser.Tabs)
        {
            // The page the tab really has, not the address it shows.
            if (tab.Core is not { } core || !IsTools(core.Source)) continue;
            try { core.PostWebMessageAsJson(json); } catch { }
        }
    }

    // MARK: - what the browser answers itself

    private static string Text(JsonObject args, string key) => args[key]?.GetValue<string>() ?? "";

    private static async Task<JsonNode?> Local(string what, JsonObject args)
    {
        switch (what)
        {
            case "path":
                return Text(args, "which") switch
                {
                    "appData" => Engine.DataDir,
                    "temp" => Path.GetTempPath(),
                    "documents" => Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments),
                    "desktop" => Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory),
                    "pictures" => Environment.GetFolderPath(Environment.SpecialFolder.MyPictures),
                    "downloads" => Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), "Downloads"),
                    _ => Environment.GetFolderPath(Environment.SpecialFolder.UserProfile),
                };
            case "app.version":
                return Updater.Version;
            case "emit":
                UI.Do(() => Broadcast(Text(args, "event"), args["payload"]));
                return null;
            case "notify":
                return null;
            case "clipboard.secret":
                // A password, a key, a decrypted text: onto the clipboard
                // marked so no clipboard history keeps it — Windows', the
                // cloud's, or Search's own.
                var secret = Text(args, "text");
                var copied = false;
                await OnUi(() =>
                {
                    copied = QuietCopy.Copy(secret);
                    return Task.FromResult<JsonNode?>(null);
                });
                if (!copied) throw new InvalidOperationException("the clipboard was busy");
                return null;
            case "fs.readText":
                return await File.ReadAllTextAsync(Text(args, "path"));
            case "fs.writeText":
                await File.WriteAllTextAsync(Text(args, "path"), Text(args, "contents"));
                return null;
            case "fs.write":
                await File.WriteAllBytesAsync(Text(args, "path"), Convert.FromBase64String(Text(args, "base64")));
                return null;
            case "opener.url":
                if (Uri.TryCreate(Text(args, "url"), UriKind.Absolute, out var url) && Address.IsWeb(url))
                    UI.Do(() => App.Window?.Browser.Visit(url, apart: true));
                return null;
            case "opener.path":
                Process.Start(new ProcessStartInfo(Text(args, "path")) { UseShellExecute = true })?.Dispose();
                return null;
            case "opener.reveal":
                Process.Start("explorer.exe", $"/select,\"{Text(args, "path")}\"")?.Dispose();
                return null;
            case "drop.paths":
                return args["paths"]?.DeepClone();
            case "open.file":
                // As the field opens it: never run from a tool page.
                var local = Text(args, "path");
                UI.Do(() => App.Window?.Browser.OpenLocal(local));
                return null;
            case "dialog.open":
                return await OnUi(() => PickOpen(args));
            case "dialog.save":
                return await OnUi(() => PickSave(args));
            case "dialog.confirm":
                return await OnUi(async () => (JsonNode?)await Confirm(args, withCancel: true));
            case "dialog.message":
                return await OnUi(async () => { await Confirm(args, withCancel: false); return (JsonNode?)null; });
            case "packs.open":
                // A page may show Settings › Packs; installing is a click there.
                UI.Do(Packs.Show);
                return null;
            case "play.prepare":
                return await Player.Prepare(args);
            case "play.cancel":
                Player.Cancel(Text(args, "path"));
                return null;
            default:
                throw new InvalidOperationException($"the browser doesn't do {what}");
        }
    }

    private static Task<JsonNode?> OnUi(Func<Task<JsonNode?>> work)
    {
        var done = new TaskCompletionSource<JsonNode?>();
        UI.Do(async () =>
        {
            try { done.SetResult(await work()); }
            catch (Exception error) { done.SetException(error); }
        });
        return done.Task;
    }

    private static IEnumerable<string> Extensions(JsonObject args) =>
        (args["filters"] as JsonArray ?? [])
            .SelectMany(filter => filter?["extensions"] as JsonArray ?? [])
            .Select(ext => ext?.GetValue<string>() ?? "")
            .Where(ext => ext.Length > 0 && ext != "*")
            .Select(ext => "." + ext.TrimStart('.'))
            .Distinct();

    private static async Task<JsonNode?> PickOpen(JsonObject args)
    {
        var multiple = args["multiple"]?.GetValue<bool>() == true;
        if (args["directory"]?.GetValue<bool>() == true)
        {
            if (await Pick.Folder() is not { } folder) return null;
            return multiple ? new JsonArray(folder) : folder;
        }
        if (multiple)
        {
            var files = await Pick.Files(Extensions(args));
            return files.Count == 0 ? null : new JsonArray([.. files.Select(f => (JsonNode)f)]);
        }
        return await Pick.File(Extensions(args));
    }

    private static async Task<JsonNode?> PickSave(JsonObject args)
    {
        var suggested = Text(args, "defaultPath");
        var types = Extensions(args).ToList();
        if (types.Count == 0 && Path.GetExtension(suggested) is { Length: > 1 } ext) types.Add(ext);
        if (types.Count == 0) types.Add(".txt");
        return await Pick.Save(types, Path.GetFileName(suggested));
    }

    private static async Task<bool> Confirm(JsonObject args, bool withCancel)
    {
        if (App.Root?.XamlRoot is not { } root) return false;
        var dialog = new ContentDialog
        {
            XamlRoot = root,
            RequestedTheme = App.Root.RequestedTheme,
            Title = args["title"]?.GetValue<string>() ?? "Search",
            Content = new TextBlock { Text = Text(args, "message"), TextWrapping = Microsoft.UI.Xaml.TextWrapping.Wrap, MaxWidth = 360 },
            PrimaryButtonText = args["okLabel"]?.GetValue<string>() ?? "OK",
            DefaultButton = ContentDialogButton.Primary,
        };
        if (withCancel) dialog.CloseButtonText = args["cancelLabel"]?.GetValue<string>() ?? "Cancel";
        try { return await dialog.ShowAsync() == ContentDialogResult.Primary; }
        catch { return false; }
    }
}
