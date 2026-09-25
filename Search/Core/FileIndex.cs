using System.Text.Json.Nodes;
using SearchKit.Engine;
using SearchKit.Field;

namespace Search;

public sealed partial class Browser
{
    partial void StartFiles() => FileIndex.Start(Prefs);
}

/// Files on this PC, for the field: the engine's index of the folders chosen
/// in Settings › Search, and nothing else. No folder, no index — and no
/// engine started for one. With folders, the engine is told about them a few
/// seconds after the window is up (never before it), its index services
/// start, and the index is built if it never was; its watcher keeps it
/// current after that.
public static class FileIndex
{
    private static readonly SemaphoreSlim telling = new(1, 1);

    public static void Start(Preferences prefs)
    {
        prefs.On(nameof(Preferences.SearchFolders), () => Apply(prefs, rebuild: true));
        prefs.On(nameof(Preferences.FileContents), () => Apply(prefs, rebuild: prefs.FileContents, dropContents: !prefs.FileContents));
        if (prefs.SearchFolders.Count > 0) UI.After(4, () => Apply(prefs, rebuild: false));
    }

    /// Where the engine keeps its index (its `file-search-index`).
    private static string Kept => Path.Combine(Engine.DataDir, "file-search-index");

    /// Tells the engine the folders as they are now. `rebuild` builds the
    /// index again (a folder added or taken away); otherwise it's built only
    /// if it never was. With no folders left the index is deleted, and with
    /// `dropContents` (Search inside files turned off) what it knew of the
    /// files' insides: nothing it no longer covers stays on disk.
    public static async void Apply(Preferences prefs, bool rebuild, bool dropContents = false)
    {
        var folders = prefs.SearchFolders;
        var contents = prefs.FileContents;
        // No folders, no engine running and nothing kept: there's nothing to
        // tell it, and nothing to delete.
        if (folders.Count == 0 && !Engine.Client.IsConnected && !Directory.Exists(Kept)) return;
        if (!Engine.Available) return;
        await telling.WaitAsync();
        try
        {
            if (dropContents && !contents && folders.Count > 0) await Clear("content");
            var options = IndexPlan.Options(folders, contents, Store.Folder);
            await Engine.Client.CallAsync("save_file_search_index_options", new JsonObject { ["options"] = options.DeepClone() });
            if (folders.Count == 0)
            {
                await Engine.Client.CallAsync("stop_file_search_index_watcher");
                await Clear("all");
                return;
            }
            await Engine.Client.CallAsync("start_search_services");
            if (rebuild)
            {
                await Build(options, contents);
                return;
            }
            // The engine's own scheduler keeps the names fresh once they're
            // built; each index is built here the first time, on its own —
            // the names being built says nothing about the contents.
            var status = IndexStatus.Read(await Engine.Client.CallAsync("get_file_search_status"));
            if (status.Names == 0 && !status.NamesBuilding) await Start("start_filename_search_index", options);
            if (contents && status.Files == 0 && !status.Building) await Start("start_content_search_index", options);
        }
        catch (EngineException error)
        {
            Log.Write($"files: {error.Message}");
        }
        catch (Exception error)
        {
            Links.Trouble(error);
        }
        finally
        {
            telling.Release();
        }
    }

    /// `clear_file_search_index`: "content", or "all" (names too).
    private static async Task Clear(string kind)
    {
        try
        {
            await Engine.Client.CallAsync("clear_file_search_index", new JsonObject { ["kind"] = kind });
            Log.Write($"files: index cleared ({kind})");
        }
        // One file still held a moment too long: the rest is gone, and the
        // next clear takes what's left.
        catch (EngineException error) when (!error.Message.Contains("went away", StringComparison.Ordinal))
        {
            Log.Write($"files: clear {kind}: {error.Message}");
        }
    }

    /// Settings › Search › Rebuild.
    public static async void Rebuild(Preferences prefs)
    {
        if (prefs.SearchFolders.Count == 0 || !Engine.Available) return;
        try
        {
            await Build(IndexPlan.Options(prefs.SearchFolders, prefs.FileContents, Store.Folder), prefs.FileContents);
        }
        catch (EngineException error)
        {
            Log.Write($"files: {error.Message}");
        }
    }

    /// Names always; what's inside the files too, when that's wanted (the
    /// engine's full build is both).
    private static Task Build(JsonObject options, bool contents) =>
        Start(contents ? "start_file_search_index" : "start_filename_search_index", options);

    private static async Task Start(string method, JsonObject options)
    {
        try
        {
            await Engine.Client.CallAsync(method, new JsonObject { ["options"] = options.DeepClone() });
        }
        // Already building: the build under way has these folders too.
        catch (EngineException error) when (error.Message.Contains("already running", StringComparison.OrdinalIgnoreCase)) { }
    }

    /// How the index stands, for Settings. Null when there's nothing to ask
    /// (no folders, no engine) or the engine couldn't say.
    public static async Task<IndexStatus?> Status(Preferences prefs)
    {
        if (prefs.SearchFolders.Count == 0 || !Engine.Available) return null;
        try
        {
            return IndexStatus.Read(await Engine.Client.CallAsync("get_file_search_status"));
        }
        catch (EngineException)
        {
            return null;
        }
    }
}
