using System.Security.Cryptography;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using Microsoft.Web.WebView2.Core;
using Windows.System;

namespace Search;

// Chrome extensions, without Chrome.
//
// On the Mac the engine is WebKit's, and most of Extensions.swift is the
// browser's half of a long contract with it — tabs, windows, popups,
// permissions — plus the APIs WebKit doesn't have, filled in by hand. Here
// the engine is Chromium, and runs Chrome extensions itself, per WebView2
// profile. What is left for the browser is the list: what is installed, from
// where, on or off, pinned or not — kept here, and put into every profile a
// tab is using the moment that profile first starts. Private tabs see none.
//
// The ids are Chrome's. A store extension is unpacked with the public key it
// was signed with written into its manifest, which is how Chromium names an
// unpacked extension after its key; one loaded from a folder is given a key
// of its own, once, so its id — and everything it keeps under its
// chrome-extension:// origin — stays the same through every reload.

/// One installed extension, as the list in Settings shows it.
public sealed class Installed
{
    /// The Chrome Web Store id, or for one loaded from a folder, the id its
    /// key gives it.
    public string Id { get; set; } = "";
    public string Name { get; set; } = "";
    public string Version { get; set; } = "";
    public bool Enabled { get; set; } = true;
    public bool FromStore { get; set; }
    /// The permissions it was installed with, so an update that asks for more
    /// is asked about rather than slipped through.
    public List<string> Permissions { get; set; } = [];
    /// Kept in the row beside the puzzle button rather than only behind it.
    public bool? Pinned { get; set; }
    /// For one loaded from a folder: where that folder is, so Reload can
    /// bring the author's latest edits in.
    public string? Source { get; set; }
    /// Where this version is unpacked. WebView2 drops an extension whose
    /// files change under it, so each version gets a folder of its own and a
    /// folder is never written to once the engine has it.
    public string Folder { get; set; } = "";
    /// For one loaded from a folder that had no key: the one it was given,
    /// written into each copy so the id never moves.
    public string? Key { get; set; }
    /// What the engine calls it, in the unlikely case that isn't Id.
    public string? Engine { get; set; }
    /// Which folder each profile last loaded it from, so a profile starting
    /// up is only given what has changed since.
    public Dictionary<string, string>? Profiles { get; set; }

    public string EngineId => Engine ?? Id;
}

public sealed class Extensions : Model
{
    public static readonly Extensions Shared = new();

    public static string Folder => Path.Combine(Store.Folder, "Extensions");
    private const string ListName = "Extensions\\installed.json";
    /// Extensions removed, by the engine's id, so a profile that wasn't
    /// running then lets them go when it next starts.
    private const string GoneKey = "extensions.gone";

    private List<Installed> installed;
    public IReadOnlyList<Installed> Installed => installed;

    private string? busy;
    /// The store extension on its way in.
    public string? Busy { get => busy; private set => Set(ref busy, value); }

    /// Why an extension that should be running isn't, by id.
    public Dictionary<string, string> Failed { get; } = [];

    private Browser? browser;
    /// The profiles in use this run, by name. A profile only exists once a
    /// tab using it has started; each is brought up to date then.
    private readonly Dictionary<string, CoreWebView2Profile> profiles = [];
    /// One change to the engine's extensions at a time: WebView2 answers
    /// each asynchronously, and two at once on one profile race.
    private readonly SemaphoreSlim gate = new(1, 1);

    /// An extension's button pressed, from a keyboard shortcut: whoever draws
    /// the buttons opens its popup.
    public event Action<string>? PopupWanted;

    private Extensions()
    {
        installed = Store.Read<List<Installed>>(ListName) ?? [];
    }

    private void Save()
    {
        Store.Write(ListName, installed);
        Tell(nameof(Installed));
    }

    public Installed? Find(string id) => installed.FirstOrDefault(i => i.Id == id);

    // MARK: - starting

    public void Start(Browser browser)
    {
        this.browser = browser;
        var keep = installed.Select(i => i.Folder).ToList();
        var ids = installed.Select(i => i.Id).ToHashSet();
        _ = Task.Run(() => Tidy(keep, ids));
        UI.After(8, CheckForUpdates);
    }

    /// What earlier runs left behind: half-made installs, and versions that
    /// have since been replaced.
    private static void Tidy(List<string> folders, HashSet<string> ids)
    {
        try
        {
            if (!Directory.Exists(Folder)) return;
            var keep = folders.Where(f => f.Length > 0).Select(Path.GetFullPath).ToHashSet(StringComparer.OrdinalIgnoreCase);
            foreach (var dir in Directory.GetDirectories(Folder))
            {
                var name = Path.GetFileName(dir);
                if (name.StartsWith(".staging-") || !ids.Contains(name)) { TryDelete(dir); continue; }
                foreach (var version in Directory.GetDirectories(dir))
                    if (!keep.Contains(Path.GetFullPath(version))) TryDelete(version);
            }
        }
        catch { }
    }

    /// A page's engine has started. Its profile, the first time it is seen
    /// this run, is given every extension that is on and relieved of any
    /// that are gone. Private tabs keep nothing and see no extensions.
    public void Attach(Tab tab, CoreWebView2 core)
    {
        if (tab.Shy) return;
        try { _ = Adopt(core.Profile); }
        catch { }
    }

    private readonly Dictionary<string, Task> syncing = [];

    /// A profile, brought up to date the first time it is seen this run —
    /// awaited by a popup, which must not load before its extension is there.
    public Task Adopt(CoreWebView2Profile profile)
    {
        if (profile.IsInPrivateModeEnabled) return Task.CompletedTask;
        var name = profile.ProfileName;
        if (profiles.ContainsKey(name) && syncing.TryGetValue(name, out var known)) return known;
        profiles[name] = profile;
        return syncing[name] = Sync(name, profile);
    }

    private async Task Sync(string name, CoreWebView2Profile profile)
    {
        await gate.WaitAsync();
        try
        {
            IReadOnlyList<CoreWebView2BrowserExtension> have;
            try { have = await profile.GetBrowserExtensionsAsync(); }
            catch
            {
                profiles.Remove(name);
                syncing.Remove(name);
                return;
            }
            foreach (var item in installed.ToList())
                await Put(name, profile, item, have.FirstOrDefault(e => e.Id == item.EngineId));
            // Anything removed while this profile wasn't running goes now.
            var gone = Store.Settings.Strings(GoneKey).ToHashSet();
            var known = installed.Select(i => i.EngineId).ToHashSet();
            foreach (var stray in have.Where(e => gone.Contains(e.Id) && !known.Contains(e.Id)))
                try { await stray.RemoveAsync(); } catch { }
        }
        finally { gate.Release(); }
    }

    /// One extension, into one profile, as the list says it should be.
    private async Task Put(string name, CoreWebView2Profile profile, Installed item, CoreWebView2BrowserExtension? present)
    {
        try
        {
            if (!item.Enabled)
            {
                if (present is { IsEnabled: true }) await present.EnableAsync(false);
                return;
            }
            var loadedFrom = item.Profiles?.GetValueOrDefault(name);
            if (present == null || !SamePath(loadedFrom, item.Folder))
            {
                if (!Directory.Exists(item.Folder)) throw new DirectoryNotFoundException("Its files are missing");
                // Added again over itself is how the engine takes a new
                // version: the same id, so what it keeps is kept.
                var added = await profile.AddBrowserExtensionAsync(item.Folder);
                if (added.Id != item.Id) item.Engine = added.Id;
                (item.Profiles ??= [])[name] = item.Folder;
                if (!added.IsEnabled) await added.EnableAsync(true);
                Store.Write(ListName, installed);
            }
            else if (!present.IsEnabled)
            {
                await present.EnableAsync(true);
            }
            if (Failed.Remove(item.Id)) Tell(nameof(Installed));
        }
        catch (Exception e)
        {
            Failed[item.Id] = e.Message;
            Tell(nameof(Installed));
            Log.Write($"extensions: {item.Id} in {name}: {e.Message}");
        }
    }

    private static bool SamePath(string? a, string? b) =>
        a != null && b != null && string.Equals(Path.GetFullPath(a).TrimEnd('\\'), Path.GetFullPath(b).TrimEnd('\\'), StringComparison.OrdinalIgnoreCase);

    /// One extension, into every profile running now. The others catch up
    /// when they start.
    private async Task Apply(Installed item)
    {
        foreach (var (name, profile) in profiles.ToList())
        {
            await gate.WaitAsync();
            try
            {
                IReadOnlyList<CoreWebView2BrowserExtension> have;
                try { have = await profile.GetBrowserExtensionsAsync(); }
                catch
                {
                    profiles.Remove(name);
                    syncing.Remove(name);
                    continue;
                }
                await Put(name, profile, item, have.FirstOrDefault(e => e.Id == item.EngineId));
            }
            finally { gate.Release(); }
        }
    }

    private async Task Unapply(string engineId)
    {
        foreach (var (name, profile) in profiles.ToList())
        {
            await gate.WaitAsync();
            try
            {
                var have = await profile.GetBrowserExtensionsAsync();
                foreach (var e in have.Where(e => e.Id == engineId)) await e.RemoveAsync();
            }
            catch
            {
                profiles.Remove(name);
                syncing.Remove(name);
            }
            finally { gate.Release(); }
        }
    }

    // MARK: - installing

    /// A store link or an id, from the field in Settings, the bar that shows
    /// on a store page, or the store page's own button.
    public async void Install(string text)
    {
        if (Crx.Id(text) is not { } id)
        {
            browser?.Announce(Crx.Refused.NotAnID.Message);
            return;
        }
        if (Find(id) != null)
        {
            browser?.Announce("Already installed");
            return;
        }
        if (Busy != null) return;
        Busy = id;
        try
        {
            var crx = await Crx.Fetch(id);
            var staged = Path.Combine(Folder, ".staging-" + id);
            await Task.Run(() =>
            {
                var (zip, key) = Crx.VerifiedZip(crx, id);
                Crx.Unpack(zip, staged);
                Prepare(staged, Convert.ToBase64String(key));
            });
            await Admit(staged, id, fromStore: true, source: null, key: null);
        }
        catch (Exception e)
        {
            browser?.Announce(e.Message);
        }
        finally { Busy = null; }
    }

    /// An unpacked extension from disk — a developer's own, or one exported
    /// from another browser. Copied in, so moving the original breaks nothing.
    public async void InstallFolder(string source)
    {
        if (!File.Exists(Path.Combine(source, "manifest.json")))
        {
            browser?.Announce("That folder has no manifest.json");
            return;
        }
        // Its own key if the manifest has one — the id it has in Chrome too —
        // else one made for it now.
        var own = ExtensionManifest.Read(source)?.Key;
        string? made = null;
        string id;
        try { id = Crx.IdOfKey(Convert.FromBase64String(own ?? "")); }
        catch { own = null; id = ""; }
        if (own == null)
        {
            using var rsa = RSA.Create(2048);
            var key = rsa.ExportSubjectPublicKeyInfo();
            made = Convert.ToBase64String(key);
            id = Crx.IdOfKey(key);
        }
        if (Find(id) is { } already)
        {
            if (already.Source != null) Reload(id);
            else browser?.Announce("Already installed");
            return;
        }
        var staged = Path.Combine(Folder, ".staging-" + id);
        try
        {
            await Task.Run(() =>
            {
                TryDelete(staged);
                Copy(source, staged);
                Prepare(staged, made);
            });
        }
        catch
        {
            TryDelete(staged);
            browser?.Announce("Couldn't copy the extension");
            return;
        }
        try { await Admit(staged, id, fromStore: false, source: source, key: made); }
        catch (Exception e) { browser?.Announce(e.Message); }
    }

    /// Reads what was unpacked, asks, and — on yes — moves it into place and
    /// starts it. On no, nothing is left behind.
    private async Task Admit(string staged, string id, bool fromStore, string? source, string? key)
    {
        if (ExtensionManifest.Read(staged) is not { } manifest)
        {
            TryDelete(staged);
            throw new Crx.Refused("The extension's manifest.json can't be read");
        }
        var name = manifest.Name.Length > 0 ? manifest.Name : id;
        if (!await AskToInstall(name, Describe(manifest), manifest.IconFile(64)))
        {
            TryDelete(staged);
            return;
        }
        var target = VersionFolder(id, manifest.Version);
        await Task.Run(() => Directory.Move(staged, target));
        var item = new Installed
        {
            Id = id,
            Name = name,
            Version = manifest.Version,
            Enabled = true,
            FromStore = fromStore,
            Permissions = manifest.AllPermissions,
            Source = source,
            Folder = target,
            Key = key,
        };
        installed.RemoveAll(i => i.Id == id);
        installed.Add(item);
        Store.Settings.Set(GoneKey, Store.Settings.Strings(GoneKey).Where(g => g != id));
        Failed.Remove(id);
        Save();
        await Apply(item);
        browser?.Announce(Failed.ContainsKey(id) ? $"{name} is installed, but couldn't start" : $"{name} is installed");
        await OfferNewTabPage(item, manifest);
    }

    /// Takes the extension up again — the way Chrome's reload button does in
    /// developer mode. One loaded from a folder is copied in afresh from that
    /// folder first, so what its author just saved is what runs.
    public async void Reload(string id)
    {
        if (Find(id) is not { } item) return;
        var old = item.Folder;
        if (item.Source is { } source)
        {
            if (!File.Exists(Path.Combine(source, "manifest.json")))
            {
                browser?.Announce($"The folder {item.Name} was loaded from is gone");
                return;
            }
            var staged = Path.Combine(Folder, ".staging-" + id);
            try
            {
                var manifest = await Task.Run(() =>
                {
                    TryDelete(staged);
                    Copy(source, staged);
                    Prepare(staged, item.Key);
                    return ExtensionManifest.Read(staged) ?? throw new IOException();
                });
                var target = VersionFolder(id, manifest.Version);
                await Task.Run(() => Directory.Move(staged, target));
                item.Folder = target;
                item.Name = manifest.Name.Length > 0 ? manifest.Name : item.Name;
                item.Version = manifest.Version;
                item.Permissions = manifest.AllPermissions;
                manifests.Remove(old);
            }
            catch
            {
                TryDelete(staged);
                browser?.Announce($"Couldn't copy {item.Name} again");
                return;
            }
        }
        // Every profile loads it again, from the same folder or the new one.
        item.Profiles = null;
        Failed.Remove(id);
        Save();
        if (!item.Enabled) return;
        await Apply(item);
        if (!SamePath(old, item.Folder)) TryDelete(old);
        browser?.Announce(Failed.ContainsKey(id) ? $"{item.Name} couldn't start — see Settings › Extensions" : $"{item.Name} reloaded");
    }

    public async void Remove(string id)
    {
        if (Find(id) is not { } item) return;
        installed.Remove(item);
        Failed.Remove(id);
        Store.Settings.Set(GoneKey, Store.Settings.Strings(GoneKey).Append(item.EngineId).Distinct());
        Store.Settings.Remove("extensions.newtab." + id);
        Save();
        await Unapply(item.EngineId);
        _ = Task.Run(() => TryDelete(Path.Combine(Folder, id)));
    }

    public async void SetEnabled(string id, bool on)
    {
        if (Find(id) is not { } item || item.Enabled == on) return;
        item.Enabled = on;
        if (!on) Failed.Remove(id);
        Save();
        await Apply(item);
    }

    public void SetPinned(string id, bool on)
    {
        if (Find(id) is not { } item) return;
        item.Pinned = on;
        Save();
    }

    public void OpenOptions(string id)
    {
        if (Find(id) is { } item && Page(item, Manifest(item)?.OptionsPage) is { } url) browser?.Open(url, foreground: true);
    }

    /// Where a page of an extension's own is: chrome-extension://<id>/<path>.
    public static Uri? Page(Installed item, string? path)
    {
        if (string.IsNullOrWhiteSpace(path)) return null;
        return Uri.TryCreate(new Uri($"chrome-extension://{item.EngineId}/"), path.TrimStart('/'), out var url) ? url : null;
    }

    public Uri? PopupUrl(Installed item) => Page(item, Manifest(item)?.Popup);

    // MARK: - the buttons

    /// One per extension that is on, in install order — in Chrome's puzzle
    /// menu every extension has a place, button or not.
    public List<Installed> Buttons => installed.Where(i => i.Enabled).ToList();

    /// The button pressed, from a shortcut: its popup, if it has one.
    public void Press(string id) => PopupWanted?.Invoke(id);

    // MARK: - reading an extension

    private readonly Dictionary<string, ExtensionManifest?> manifests = new(StringComparer.OrdinalIgnoreCase);

    public ExtensionManifest? Manifest(Installed item)
    {
        if (!manifests.TryGetValue(item.Folder, out var found))
            manifests[item.Folder] = found = ExtensionManifest.Read(item.Folder);
        return found;
    }

    /// An icon from the extension's own folder, for its button and its row.
    private readonly Dictionary<string, BitmapImage> icons = [];

    public ImageSource? Icon(Installed item, int size)
    {
        if (Manifest(item)?.IconFile(size * 2) is not { } file) return null;
        var key = $"{file}|{size}";
        if (icons.TryGetValue(key, out var known)) return known;
        try
        {
            var image = new BitmapImage { DecodePixelWidth = size * 2, UriSource = new Uri(file) };
            icons[key] = image;
            return image;
        }
        catch { return null; }
    }

    // MARK: - new tab pages

    /// The page an extension asks to show in new tabs, from the one added
    /// last — once you've said yes to it. Null while nobody asks, or you said
    /// no.
    public Uri? NewTabPage
    {
        get
        {
            foreach (var item in Enumerable.Reverse(installed))
            {
                if (!item.Enabled || Page(item, Manifest(item)?.NewTab) is not { } url) continue;
                return Store.Settings.Bool("extensions.newtab." + item.Id) ? url : null;
            }
            return null;
        }
    }

    public bool HasNewTabPage(Installed item) => Manifest(item)?.NewTab != null;
    public bool ShowsInNewTabs(Installed item) => Store.Settings.Bool("extensions.newtab." + item.Id);

    public void SetShowsInNewTabs(Installed item, bool on)
    {
        Store.Settings.Set("extensions.newtab." + item.Id, on);
        Tell(nameof(Installed));
    }

    /// Chrome asks the first time an extension's page takes the place of the
    /// new tab — an extension that did it quietly could be anything. So does
    /// Search, as soon as it is added.
    private async Task OfferNewTabPage(Installed item, ExtensionManifest manifest)
    {
        if (manifest.NewTab == null || Store.Settings.Has("extensions.newtab." + item.Id)) return;
        var yes = await Ask($"Show “{item.Name}” in new tabs?",
            "It asked to replace the new tab page. You can change this later in Settings › Extensions.",
            manifest.IconFile(64), "Keep It", "Don't Allow");
        Store.Settings.Set("extensions.newtab." + item.Id, yes);
        Tell(nameof(Installed));
    }

    /// A password manager extension that asked, Chrome's way, to do the saving
    /// itself — its name, for Settings to say so. What an extension sets
    /// through chrome.privacy stays inside the engine; WebView2 has no way to
    /// read it back, so this is never known here.
    public string? PasswordSavingTakenBy => null;

    // MARK: - updates

    /// Once a day, the store is asked whether anything installed from it has
    /// a newer version; if so it is fetched, checked and swapped in. One that
    /// asks for more than it was installed with is asked about first.
    public void CheckForUpdates()
    {
        const string key = "extensions.checked";
        var now = DateTimeOffset.UtcNow.ToUnixTimeSeconds();
        if (now - Store.Settings.Double(key) < 60 * 60 * 20) return;
        Store.Settings.Set(key, (double)now);
        foreach (var item in installed.Where(i => i.FromStore).ToList()) _ = Update(item);
    }

    private static readonly Regex UpdateVersion = new(@"<updatecheck[^>]*\sversion=""([^""]+)""", RegexOptions.Compiled);

    private async Task Update(Installed item)
    {
        var staged = Path.Combine(Folder, ".staging-" + item.Id);
        try
        {
            var xml = await Crx.Http.GetStringAsync(Crx.UpdateCheckUrl(item.Id, item.Version));
            if (!xml.Contains("status=\"ok\"") || UpdateVersion.Match(xml) is not { Success: true } match) return;
            if (match.Groups[1].Value == item.Version) return;
            var crx = await Crx.Fetch(item.Id);
            var manifest = await Task.Run(() =>
            {
                var (zip, key) = Crx.VerifiedZip(crx, item.Id);
                Crx.Unpack(zip, staged);
                Prepare(staged, Convert.ToBase64String(key));
                return ExtensionManifest.Read(staged) ?? throw Crx.Refused.Unpack;
            });
            var wants = manifest.AllPermissions;
            if (!wants.All(item.Permissions.Contains)
                && !await AskToInstall($"An update to {item.Name}", Describe(manifest), manifest.IconFile(64)))
            {
                TryDelete(staged);
                return;
            }
            if (Find(item.Id) != item)
            {
                TryDelete(staged);
                return;
            }
            var target = VersionFolder(item.Id, manifest.Version);
            await Task.Run(() => Directory.Move(staged, target));
            var old = item.Folder;
            item.Folder = target;
            item.Version = manifest.Version;
            item.Permissions = wants;
            Save();
            if (item.Enabled) await Apply(item);
            if (!SamePath(old, target)) TryDelete(old);
        }
        catch (Exception e)
        {
            TryDelete(staged);
            Log.Write($"extensions: update of {item.Id} failed: {e.Message}");
        }
    }

    // MARK: - files

    /// A folder for this version of this extension, never one the engine
    /// already has: the same version again gets a folder beside it.
    private static string VersionFolder(string id, string version)
    {
        var safe = string.Concat((version.Length > 0 ? version : "0").Select(c => char.IsLetterOrDigit(c) || c is '.' or '-' ? c : '_'));
        var target = Path.Combine(Folder, id, safe);
        if (Directory.Exists(target)) TryDelete(target);
        if (Directory.Exists(target)) target += "-" + DateTime.UtcNow.Ticks;
        Directory.CreateDirectory(Path.GetDirectoryName(target)!);
        return target;
    }

    /// Made ready for the engine: the key that gives it its id written into
    /// its manifest, and Chrome's own _metadata folder — the store's content
    /// hashes, which Chrome keeps for itself — taken out, since WebView2
    /// refuses to load anything with a name starting with "_" but _locales.
    private static void Prepare(string folder, string? key)
    {
        var metadata = Path.Combine(folder, "_metadata");
        if (Directory.Exists(metadata)) Directory.Delete(metadata, recursive: true);
        if (key == null) return;
        var file = Path.Combine(folder, "manifest.json");
        if (JsonNode.Parse(File.ReadAllText(file), documentOptions: ExtensionManifest.Lenient) is not JsonObject json) return;
        if (json["key"] is JsonValue) return;
        json["key"] = key;
        File.WriteAllText(file, json.ToJsonString(new JsonSerializerOptions { WriteIndented = true }));
    }

    private static void Copy(string from, string to)
    {
        Directory.CreateDirectory(to);
        foreach (var dir in Directory.GetDirectories(from, "*", SearchOption.AllDirectories))
        {
            var relative = Path.GetRelativePath(from, dir);
            // A git checkout's history is no part of the extension.
            if (relative.Split(Path.DirectorySeparatorChar).Contains(".git")) continue;
            Directory.CreateDirectory(Path.Combine(to, relative));
        }
        foreach (var file in Directory.GetFiles(from, "*", SearchOption.AllDirectories))
        {
            var relative = Path.GetRelativePath(from, file);
            if (relative.Split(Path.DirectorySeparatorChar).Contains(".git")) continue;
            File.Copy(file, Path.Combine(to, relative), overwrite: true);
        }
    }

    private static void TryDelete(string folder)
    {
        try { if (Directory.Exists(folder)) Directory.Delete(folder, recursive: true); } catch { }
    }

    // MARK: - asking

    /// What an extension wants, in words.
    public static List<string> Describe(ExtensionManifest manifest)
    {
        var output = new List<string>();
        var hosts = manifest.HostPatterns;
        if (hosts.Any(p => p is "<all_urls>" or "*://*/*" or "http://*/*" or "https://*/*"))
            output.Add("Read and change everything on every website");
        else if (hosts.Count > 0)
        {
            var names = hosts.Select(p => Regex.Match(p, @"^[^:]+://([^/]*)").Groups[1].Value).Where(h => h.Length > 0).Distinct().ToList();
            if (names.Count > 0)
                output.Add("Read and change what's on " + string.Join(", ", names.Take(4)) + (names.Count > 4 ? $" and {names.Count - 4} more" : ""));
        }
        (string, string)[] words =
        [
            ("tabs", "See your open tabs and their addresses"), ("cookies", "Read and change cookies"),
            ("webNavigation", "See where you go"), ("webRequest", "See the requests pages make"),
            ("declarativeNetRequest", "Block or change requests pages make"), ("clipboardWrite", "Write to the clipboard"),
            ("nativeMessaging", "Talk to apps on this computer"), ("scripting", "Run scripts in pages"),
            ("userScripts", "Run scripts you add to it on websites"), ("history", "Read and change your history"),
            ("bookmarks", "Read and change your bookmarks"), ("downloads", "Manage your downloads"),
            ("privacy", "Change your privacy settings"), ("browsingData", "Clear your browsing data"),
            ("management", "See your other extensions"), ("notifications", "Show notifications"),
        ];
        var asked = manifest.ApiPermissions;
        foreach (var (permission, sentence) in words)
            if (asked.Contains(permission)) output.Add(sentence);
        return output;
    }

    private Task<bool> AskToInstall(string name, List<string> wants, string? icon) => Ask(
        name.StartsWith("An update to ") ? $"{name} asks for more" : $"Add “{name}” to Search?",
        wants.Count == 0 ? "It doesn't ask for anything special." : "It will be able to:\n• " + string.Join("\n• ", wants),
        icon, name.StartsWith("An update to ") ? "Update" : "Add Extension", "Cancel");

    private Task<bool>? question;

    /// One question at a time, over the window. A second waits for the first
    /// to be answered — WinUI shows one dialog at a time anyway, and an
    /// extension's update can ask when nobody is looking.
    public Task<bool> Ask(string title, string detail, string? icon, string yes, string no)
    {
        var before = question;
        var task = AskAfter(before, title, detail, icon, yes, no);
        question = task;
        return task;
    }

    private static async Task<bool> AskAfter(Task<bool>? before, string title, string detail, string? icon, string yes, string no)
    {
        if (before != null) try { await before; } catch { }
        if (App.Root?.XamlRoot is not { } root) return false;
        var words = new StackPanel { Spacing = 12, Orientation = Orientation.Horizontal };
        if (icon != null)
        {
            try
            {
                words.Children.Add(new Image
                {
                    Source = new BitmapImage { DecodePixelWidth = 96, UriSource = new Uri(icon) },
                    Width = 48,
                    Height = 48,
                    VerticalAlignment = VerticalAlignment.Top,
                });
            }
            catch { }
        }
        var text = Kit.Text(detail, 13, Palette.Ink);
        text.TextWrapping = TextWrapping.Wrap;
        text.TextTrimming = TextTrimming.None;
        text.MaxWidth = 340;
        words.Children.Add(text);
        var dialog = new ContentDialog
        {
            XamlRoot = root,
            Title = title,
            Content = words,
            PrimaryButtonText = yes,
            CloseButtonText = no,
            DefaultButton = ContentDialogButton.Primary,
            RequestedTheme = Palette.Dark ? ElementTheme.Dark : ElementTheme.Light,
        };
        try { return await dialog.ShowAsync() == ContentDialogResult.Primary; }
        catch { return false; }
    }

    // MARK: - what the engine doesn't hand over

    /// An extension's OAuth sign-in coming back (https://<id>.chromiumapp.org).
    /// On the Mac the browser runs chrome.identity.launchWebAuthFlow itself
    /// and catches the answer here. In WebView2 that API belongs to the
    /// engine, and there is no way for the browser to hand an answer back
    /// into an extension, so nothing is caught: a navigation there is left
    /// to the engine.
    public bool Intercept(Uri url) => false;

    /// A shortcut an extension registered in its manifest's "commands". Its
    /// own button's (_execute_action) opens its popup, as in Chrome. Any other
    /// is the extension's to hear through chrome.commands.onCommand, and
    /// WebView2 has no way for the browser to send it one — so those keys are
    /// left to go on to the page.
    public bool Take(VirtualKey key, bool ctrl, bool shift, bool alt)
    {
        foreach (var item in installed)
        {
            if (!item.Enabled || Manifest(item) is not { } manifest) continue;
            foreach (var (name, chord) in manifest.Commands)
            {
                if (!name.StartsWith("_execute_") || Chord.Parse(chord) is not { } wanted) continue;
                if (wanted.Key != key || wanted.Ctrl != ctrl || wanted.Shift != shift || wanted.Alt != alt) continue;
                Press(item.Id);
                return true;
            }
        }
        return false;
    }

    /// What extensions add to a page's right-click menu. WebView2 hands the
    /// browser its own menu to rearrange, but not the items extensions made
    /// with chrome.contextMenus — nor any way to tell an extension one was
    /// chosen — so there is nothing to add here.
    public void AddMenuItems(Tab tab, CoreWebView2 core, CoreWebView2ContextMenuRequestedEventArgs e) { }
}

/// A key and its modifiers, as a manifest writes them: "Ctrl+Shift+Y".
public readonly record struct Chord(VirtualKey Key, bool Ctrl, bool Shift, bool Alt)
{
    public static Chord? Parse(string text)
    {
        bool ctrl = false, shift = false, alt = false;
        VirtualKey? key = null;
        foreach (var raw in text.Split('+', StringSplitOptions.TrimEntries | StringSplitOptions.RemoveEmptyEntries))
        {
            switch (raw.ToLowerInvariant())
            {
                // Command is the Mac's name for Ctrl; a manifest that only
                // gave the Mac's gets Chrome's translation.
                case "ctrl" or "command": ctrl = true; break;
                case "macctrl": return null;
                case "shift": shift = true; break;
                case "alt": alt = true; break;
                default: key = KeyOf(raw); if (key == null) return null; break;
            }
        }
        return key is { } k && (ctrl || alt) ? new Chord(k, ctrl, shift, alt) : null;
    }

    private static VirtualKey? KeyOf(string name)
    {
        if (name.Length == 1)
        {
            var c = char.ToUpperInvariant(name[0]);
            if (c is >= 'A' and <= 'Z' or >= '0' and <= '9') return (VirtualKey)c;
            return null;
        }
        if (name.Length is 2 or 3 && (name[0] is 'F' or 'f') && int.TryParse(name[1..], out var f) && f is >= 1 and <= 12)
            return VirtualKey.F1 + (f - 1);
        return name.ToLowerInvariant() switch
        {
            "comma" => (VirtualKey)188,
            "period" => (VirtualKey)190,
            "home" => VirtualKey.Home,
            "end" => VirtualKey.End,
            "pageup" => VirtualKey.PageUp,
            "pagedown" => VirtualKey.PageDown,
            "space" => VirtualKey.Space,
            "insert" => VirtualKey.Insert,
            "delete" => VirtualKey.Delete,
            "up" => VirtualKey.Up,
            "down" => VirtualKey.Down,
            "left" => VirtualKey.Left,
            "right" => VirtualKey.Right,
            _ => null,
        };
    }
}

/// An extension's manifest.json, read for what the browser needs of it.
public sealed class ExtensionManifest
{
    /// Chrome reads manifests with comments and trailing commas allowed.
    public static readonly JsonDocumentOptions Lenient = new() { CommentHandling = JsonCommentHandling.Skip, AllowTrailingCommas = true };

    private readonly JsonObject json;
    private readonly string folder;

    private ExtensionManifest(JsonObject json, string folder)
    {
        this.json = json;
        this.folder = folder;
    }

    public static ExtensionManifest? Read(string folder)
    {
        try
        {
            var file = Path.Combine(folder, "manifest.json");
            if (!File.Exists(file)) return null;
            return JsonNode.Parse(File.ReadAllText(file), documentOptions: Lenient) is JsonObject json ? new ExtensionManifest(json, folder) : null;
        }
        catch { return null; }
    }

    private static string? Str(JsonNode? node) => node is JsonValue v && v.TryGetValue<string>(out var s) ? s : null;

    private JsonObject? Action => (json["action"] ?? json["browser_action"] ?? json["page_action"]) as JsonObject;

    public string Name => Localize(Str(json["name"]) ?? "");
    public string Version => Str(json["version"]) ?? "?";
    public string? Key => Str(json["key"]);
    public bool HasAction => Action != null;
    public string? Title => Str(Action?["default_title"]) is { } t ? Localize(t) : null;
    public string? Popup => Str(Action?["default_popup"]) is { Length: > 0 } p ? p : null;
    public string? OptionsPage => Str((json["options_ui"] as JsonObject)?["page"]) ?? Str(json["options_page"]);
    public string? NewTab => Str((json["chrome_url_overrides"] as JsonObject)?["newtab"]);

    private IEnumerable<string> Strings(string key) =>
        json[key] is JsonArray list ? list.Select(Str).OfType<string>() : [];

    /// The APIs it asks for, without the sites (which Manifest V2 listed in
    /// the same place).
    public HashSet<string> ApiPermissions => Strings("permissions").Where(p => !p.Contains("://") && p != "<all_urls>").ToHashSet();

    /// The sites it can reach: host permissions, and where its content
    /// scripts run.
    public List<string> HostPatterns
    {
        get
        {
            var patterns = Strings("host_permissions").Concat(Strings("permissions").Where(p => p.Contains("://") || p == "<all_urls>")).ToList();
            if (json["content_scripts"] is JsonArray scripts)
                foreach (var script in scripts.OfType<JsonObject>())
                    if (script["matches"] is JsonArray matches) patterns.AddRange(matches.Select(Str).OfType<string>());
            return patterns.Distinct().ToList();
        }
    }

    public List<string> AllPermissions => ApiPermissions.Concat(HostPatterns).Distinct().OrderBy(p => p, StringComparer.Ordinal).ToList();

    /// Its shortcuts: each command's name, and the chord suggested for
    /// Windows, else the default.
    public IEnumerable<(string Name, string Chord)> Commands
    {
        get
        {
            if (json["commands"] is not JsonObject commands) yield break;
            foreach (var (name, value) in commands)
            {
                var suggested = (value as JsonObject)?["suggested_key"];
                var chord = suggested is JsonObject keys ? Str(keys["windows"]) ?? Str(keys["default"]) : Str(suggested);
                if (chord != null) yield return (name, chord);
            }
        }
    }

    /// The file of the icon nearest `pixels`, at least that big if there is
    /// one: the button's own, else the extension's.
    public string? IconFile(int pixels)
    {
        var sets = new List<Dictionary<int, string>>();
        foreach (var node in new[] { Action?["default_icon"], json["icons"] })
        {
            if (Str(node) is { } single) sets.Add(new() { [pixels] = single });
            else if (node is JsonObject sizes)
            {
                var set = new Dictionary<int, string>();
                foreach (var (size, path) in sizes)
                    if (int.TryParse(size, out var n) && Str(path) is { } p) set[n] = p;
                if (set.Count > 0) sets.Add(set);
            }
        }
        foreach (var set in sets)
        {
            var best = set.Where(p => p.Key >= pixels).OrderBy(p => p.Key).Select(p => p.Value).FirstOrDefault()
                ?? set.OrderByDescending(p => p.Key).First().Value;
            var file = Path.GetFullPath(Path.Combine(folder, best.TrimStart('/').Replace('/', Path.DirectorySeparatorChar)));
            if (file.StartsWith(Path.GetFullPath(folder), StringComparison.OrdinalIgnoreCase) && File.Exists(file)) return file;
        }
        return null;
    }

    /// "__MSG_name__", in the language Windows is in, else the extension's
    /// own default.
    private string Localize(string text)
    {
        var match = Regex.Match(text, "^__MSG_(\\w+)__$");
        if (!match.Success) return text;
        var key = match.Groups[1].Value;
        var culture = System.Globalization.CultureInfo.CurrentUICulture;
        var tried = new[] { culture.Name.Replace('-', '_'), culture.TwoLetterISOLanguageName, Str(json["default_locale"]) };
        foreach (var locale in tried.OfType<string>().Where(l => l.Length > 0).Distinct())
        {
            try
            {
                var file = Path.Combine(folder, "_locales", locale, "messages.json");
                if (!File.Exists(file) || JsonNode.Parse(File.ReadAllText(file), documentOptions: Lenient) is not JsonObject messages) continue;
                foreach (var (name, entry) in messages)
                    if (string.Equals(name, key, StringComparison.OrdinalIgnoreCase) && Str((entry as JsonObject)?["message"]) is { } found)
                        return found;
            }
            catch { }
        }
        return text;
    }
}

// Where the extensions hook into the browser's life (see Browser.Features).
public sealed partial class Browser
{
    partial void StartExtensions()
    {
        Extensions.Shared.Start(this);
        StartStore();
    }

    partial void AttachExtensions(Tab tab, CoreWebView2 core) => Extensions.Shared.Attach(tab, core);
}
