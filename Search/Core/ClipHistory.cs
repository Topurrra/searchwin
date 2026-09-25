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
/// three seconds after the window is first activated, behind the page, and
/// asked to listen. The browser keeps its line to the engine open all
/// session, so the engine stays; if the engine goes away anyway, it is
/// started and asked again, with longer pauses between tries if it keeps
/// going.
public static class ClipHistory
{
    public const string Updated = ClipboardSource.ChangedEvent;

    private static Browser? browser;
    private static Preferences? prefs;
    /// The pause as last known: chosen in Settings, or read from the engine
    /// (`get_clipboard_paused`), which is what decides.
    private static volatile bool paused;
    private static double retry = 5;
    private static long armedAt;
    private static bool woken;

    /// The history changed: a copy kept, an entry pinned or gone. On the UI thread.
    public static event Action? Changed;

    /// Whether history is kept, and there's an engine to keep it.
    public static bool On => prefs?.ClipboardHistory == true && Engine.Available;

    /// Settings' "Pause": nothing new is kept until it's turned off again, or
    /// Search starts again — the engine keeps it for this session only. As
    /// the engine last said it.
    public static bool Paused => paused;

    /// With the browser: nothing but the setting watched. The engine waits
    /// for the window (Wake).
    public static void Start(Browser owner)
    {
        browser = owner;
        prefs = owner.Prefs;
        prefs.On(nameof(Preferences.ClipboardHistory), Apply);
    }

    /// The main window's first activation: three seconds later, behind the
    /// page, the engine is started and asked to listen. Not even a look for
    /// it before the window is up and has been shown.
    public static void Wake()
    {
        if (woken || prefs is not { } settings) return;
        woken = true;
        UI.After(3, () =>
        {
            Hook();
            if (settings.ClipboardHistory) Arm(turnedOn: false);
        });
    }

    private static bool hooked;

    private static void Hook()
    {
        if (hooked) return;
        hooked = true;
        Engine.Client.EventReceived += (name, payload) =>
        {
            if (name != Updated) return;
            // A pause (or its end) is said with the same event.
            _ = Task.Run(ReadPause);
            UI.Do(() => Changed?.Invoke());
        };
        // A new engine (the first, or one started again) knows nothing of
        // what the last was asked.
        Engine.Client.Connected += () =>
        {
            if (On) Arm(turnedOn: false);
        };
        Engine.Client.Disconnected += () => UI.Do(Lost);
    }

    private static void Apply()
    {
        Hook();
        if (On)
        {
            Arm(turnedOn: true);
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
    /// asked). Turned on in Settings, the pause that turning it off put on is
    /// lifted; on an engine newly joined, the pause is read from it and never
    /// lifted — only put back when the user chose it and the engine started
    /// afresh without it.
    private static void Arm(bool turnedOn) => _ = Task.Run(async () =>
    {
        if (!On) return;
        try
        {
            await Engine.Client.CallAsync("start_clipboard_listener");
            if (turnedOn)
            {
                await Engine.Client.CallAsync("set_clipboard_paused", new JsonObject { ["paused"] = paused });
            }
            else
            {
                var (now, tell) = ClipGuard.Rejoin(paused, IsTrue(await Engine.Client.CallAsync("get_clipboard_paused")));
                paused = now;
                if (tell is { } pause) await Engine.Client.CallAsync("set_clipboard_paused", new JsonObject { ["paused"] = pause });
            }
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
            if (On && !Engine.Client.IsConnected) Arm(turnedOn: false);
        });
    }

    /// The pause, as the engine has it now.
    private static async Task ReadPause()
    {
        if (!On) return;
        try { paused = IsTrue(await Engine.Client.CallAsync("get_clipboard_paused")); }
        catch (EngineException error) { Log.Write($"clipboard: {error.Message}"); }
    }

    private static bool IsTrue(JsonNode? node) => node is JsonValue v && v.TryGetValue<bool>(out var on) && on;

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
            // Up to 200 entries of a quarter megabyte each: read off the UI thread.
            return Last = await Task.Run(async () => ClipList.Read(await Engine.Client.CallAsync("get_clipboard_history")));
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

    /// Everything but what's pinned. Works with history off too: turning it
    /// off offers to clear what's kept.
    public static async Task<bool> Clear()
    {
        Last = [.. Last.Where(e => e.Pinned)];
        if (!On)
        {
            // Off, the engine may not have read the history this session:
            // it reads it now to clear it — paused first, so nothing new is kept.
            if (!await Call("set_clipboard_paused", new JsonObject { ["paused"] = true })) return false;
            if (!await Call("start_clipboard_listener", null)) return false;
        }
        return await Call("clear_clipboard_history", null);
    }

    /// An entry back on the clipboard itself, as it was copied (a picture as
    /// a picture). The engine doesn't keep its own write as a new entry, and a
    /// secret goes back marked so that no clipboard history keeps it.
    public static Task<bool> CopyBack(ClipEntry entry) => CopyBack(entry.Id, ClipGuard.Quiet(entry));

    /// The field's rows carry an id and whether it's a secret, not the entry.
    public static Task<bool> CopyBack(long id, bool quiet) =>
        Call("copy_clipboard_entry_to_clipboard", new JsonObject { ["id"] = id, ["quiet"] = quiet });

    // MARK: - settings

    /// What Settings › Clipboard shows. Null when the engine can't say.
    public sealed record Choices(int Days, bool Images, int ImageDays, bool Paused, IReadOnlyList<string> Exclusions);

    public static async Task<Choices?> Read()
    {
        if (!Engine.Available) return null;
        try
        {
            var days = Number(await Engine.Client.CallAsync("get_clipboard_retention_days"), 14);
            var images = IsTrue(await Engine.Client.CallAsync("get_clipboard_images_enabled"));
            var imageDays = Number(await Engine.Client.CallAsync("get_clipboard_image_retention_days"), 2);
            var apps = (await Engine.Client.CallAsync("get_clipboard_exclusions")) is JsonArray list
                ? list.OfType<JsonValue>().Select(v => v.TryGetValue<string>(out var s) ? s : "").Where(s => s.Length > 0).ToList()
                : [];
            // Paused as the engine has it, which is what decides. History off
            // pauses the engine too, so that's only asked while it's on.
            if (On) paused = IsTrue(await Engine.Client.CallAsync("get_clipboard_paused"));
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
