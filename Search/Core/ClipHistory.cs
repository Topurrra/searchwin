using System.Text.Json.Nodes;
using SearchKit.Engine;
using SearchKit.Field;

namespace Search;

public sealed partial class Browser
{
    partial void StartClipboard() => ClipHistory.Start(this);
}

/// Clipboard history, on unless turned off: the engine's listener keeps what
/// you copy (text, and pictures for a couple of days), encrypted in its own
/// folder, flags secrets and forgets them within minutes, and never keeps
/// what a password manager copies. Ctrl+Shift+V lists it (ClipPopup), and so
/// does `clip:` in the field.
///
/// Nothing of it happens before the first window: the engine is started
/// three seconds after it, behind the page, and asked to listen. The browser
/// keeps its line to the engine open all session, so the engine stays; if
/// the engine goes away anyway, it is started and asked again, with longer
/// pauses between tries if it keeps going.
public static class ClipHistory
{
    public const string Updated = ClipboardSource.ChangedEvent;

    private static Browser? browser;
    private static Preferences? prefs;
    private static bool paused;
    private static double retry = 5;
    private static long armedAt;

    /// The history changed: a copy kept, an entry pinned or gone. On the UI thread.
    public static event Action? Changed;

    /// Whether history is kept, and there's an engine to keep it.
    public static bool On => prefs?.ClipboardHistory == true && Engine.Available;

    /// Settings' "Pause": nothing new is kept until it's turned off again, or
    /// Search starts again — the engine keeps it for this session only.
    public static bool Paused => paused;

    public static void Start(Browser owner)
    {
        browser = owner;
        prefs = owner.Prefs;
        prefs.On(nameof(Preferences.ClipboardHistory), Apply);
        // Not even a look for the engine before the window is up.
        UI.After(3, () =>
        {
            Hook();
            if (prefs.ClipboardHistory) Arm();
        });
    }

    private static bool hooked;

    private static void Hook()
    {
        if (hooked) return;
        hooked = true;
        Engine.Client.EventReceived += (name, _) =>
        {
            if (name == Updated) UI.Do(() => Changed?.Invoke());
        };
        // A new engine (the first, or one started again) knows nothing of
        // what the last was asked.
        Engine.Client.Connected += () =>
        {
            if (On) Arm();
        };
        Engine.Client.Disconnected += () => UI.Do(Lost);
    }

    private static void Apply()
    {
        Hook();
        if (On)
        {
            Arm();
            return;
        }
        // Off: the listener stops keeping anything straight away, and isn't
        // started again by the next engine.
        paused = false;
        Last = [];
        if (Engine.Client.IsConnected) Send("set_clipboard_paused", new JsonObject { ["paused"] = true });
        UI.Do(() => Changed?.Invoke());
    }

    /// The listener, started (the engine does that once however often it's
    /// asked), and told whether it's paused.
    private static void Arm() => _ = Task.Run(async () =>
    {
        if (!On) return;
        try
        {
            await Engine.Client.CallAsync("start_clipboard_listener");
            await Engine.Client.CallAsync("set_clipboard_paused", new JsonObject { ["paused"] = paused });
            Interlocked.Exchange(ref armedAt, Environment.TickCount64);
            // A history file that couldn't be read was put aside and started
            // afresh; that's said once.
            if (await Engine.Client.CallAsync("take_clipboard_recovery_notice") is JsonValue notice
                && notice.TryGetValue<string>(out var said) && said.Length > 0)
                UI.Do(() => browser?.Announce("Clipboard history couldn't be read, so it started afresh"));
        }
        catch (EngineException error)
        {
            Log.Write($"clipboard: {error.Message}");
        }
    });

    /// The engine went away. Asked again after a pause — longer each time
    /// it goes, unless it had been fine for a while.
    private static void Lost()
    {
        if (!On) return;
        if (Environment.TickCount64 - Interlocked.Read(ref armedAt) > 120_000) retry = 5;
        var wait = retry;
        retry = Math.Min(retry * 2, 300);
        Log.Write($"clipboard: the engine went away; asking again in {wait} s");
        UI.After(wait, () =>
        {
            if (On && !Engine.Client.IsConnected) Arm();
        });
    }

    // MARK: - the history

    /// The history as last read, so the list can be drawn the moment it's
    /// asked for and brought up to date a moment later.
    public static List<ClipEntry> Last { get; private set; } = [];

    /// Everything kept, secrets included (a paste needs their text). Empty
    /// when history is off or the engine couldn't say.
    public static async Task<List<ClipEntry>> List()
    {
        if (!On) return [];
        try
        {
            return Last = ClipList.Read(await Engine.Client.CallAsync("get_clipboard_history"));
        }
        catch (EngineException error)
        {
            Log.Write($"clipboard: {error.Message}");
            return [];
        }
    }

    public static void Pin(long id, bool pinned) =>
        Send("pin_clipboard_entry", new JsonObject { ["id"] = id, ["pinned"] = pinned });

    public static void Delete(long id) => Send("delete_clipboard_entry", new JsonObject { ["id"] = id });

    /// Everything but what's pinned.
    public static void Clear() => Send("clear_clipboard_history", null);

    /// An entry back on the clipboard itself, as it was copied (a picture as
    /// a picture). The engine doesn't keep its own write as a new entry.
    public static Task<bool> CopyBack(long id) => Call("copy_clipboard_entry_to_clipboard", new JsonObject { ["id"] = id });

    // MARK: - settings

    /// What Settings › Clipboard shows. Null when the engine can't say.
    public sealed record Choices(int Days, bool Images, int ImageDays, bool Paused, IReadOnlyList<string> Exclusions);

    public static async Task<Choices?> Read()
    {
        if (!Engine.Available) return null;
        try
        {
            var days = Number(await Engine.Client.CallAsync("get_clipboard_retention_days"), 14);
            var images = await Engine.Client.CallAsync("get_clipboard_images_enabled") is JsonValue i && i.TryGetValue<bool>(out var on) && on;
            var imageDays = Number(await Engine.Client.CallAsync("get_clipboard_image_retention_days"), 2);
            var apps = (await Engine.Client.CallAsync("get_clipboard_exclusions")) is JsonArray list
                ? list.OfType<JsonValue>().Select(v => v.TryGetValue<string>(out var s) ? s : "").Where(s => s.Length > 0).ToList()
                : [];
            return new Choices(days, images, imageDays, paused, apps);
        }
        catch (EngineException error)
        {
            Log.Write($"clipboard: {error.Message}");
            return null;
        }
    }

    public static Task<bool> KeepFor(int days) => Call("set_clipboard_retention_days", new JsonObject { ["days"] = days });

    public static Task<bool> KeepPictures(bool on) => Call("set_clipboard_images_enabled", new JsonObject { ["enabled"] = on });

    public static Task<bool> Pause(bool on)
    {
        paused = on;
        return On ? Call("set_clipboard_paused", new JsonObject { ["paused"] = on }) : Task.FromResult(true);
    }

    /// Apps whose copies are never kept, by process name (`keepassxc`).
    public static Task<bool> Exclude(IEnumerable<string> apps)
    {
        var list = new JsonArray();
        foreach (var app in apps) list.Add((JsonNode)app);
        return Call("set_clipboard_exclusions", new JsonObject { ["apps"] = list });
    }

    public static Task<bool> ExcludePasswordManagers() => Call("reset_clipboard_exclusions_to_defaults", null);

    private static int Number(JsonNode? node, int fallback) =>
        node is JsonValue v && v.TryGetValue<int>(out var n) ? n : fallback;

    private static void Send(string method, JsonObject? args) => _ = Call(method, args);

    private static async Task<bool> Call(string method, JsonObject? args)
    {
        try
        {
            await Task.Run(() => Engine.Client.CallAsync(method, args));
            return true;
        }
        catch (EngineException error)
        {
            Log.Write($"clipboard: {method}: {error.Message}");
            return false;
        }
    }
}
