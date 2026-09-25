using System.Net.Sockets;
using System.Runtime.InteropServices;
using System.Runtime.InteropServices.WindowsRuntime;
using System.Security.Principal;
using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.Json.Nodes;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using Microsoft.Web.WebView2.Core;
using Windows.Graphics.Imaging;
using Windows.Storage.Streams;
using Windows.System;

namespace Search;

// A way for a script on this PC to drive the browser you already have open,
// in tabs of its own, without ever taking the window from you.
//
// Off unless switched on in Settings › General. On, the app listens on a named
// pipe only this Windows user can open (see PipeName). One JSON object per
// line in, one per line out, one request per connection. The tabs it opens
// sit at the end of your row with a flask on them, are never selected on your
// behalf, never enter the session or the history, and go when the script says
// so. `bench.ps1` at the root of the repository speaks this protocol from the
// shell.
//
// Pages a script has opened but you are not looking at are kept drawn,
// unseen, beneath the one you are: WebView2 lays out and paints a page only
// while its view is visible, and a picture of a page that isn't is a picture
// of nothing.

public sealed partial class Browser
{
    partial void StartBench()
    {
        if (Prefs.Bench) Search.Bench.Shared.Start(this);
    }
}

public sealed class Bench
{
    public static readonly Bench Shared = new();

    private Browser? browser;
    private CancellationTokenSource? stopping;
    private bool watching;

    /// The pipe's name: one per test world, so a test run's bench is as
    /// separate from the real one as everything else it keeps. The Mac uses a
    /// Unix socket in the app's folder; Windows has those too, but they refuse
    /// connections under AppData on some machines, and a named pipe is what
    /// Windows programs talk over.
    public static string PipeName => "search-bench" + (Store.World is { } w ? "-" + w : "");

    /// True while something is listening.
    public bool Running { get; private set; }

    // MARK: - starting and stopping

    public void Start(Browser browser)
    {
        if (Running) return;
        this.browser = browser;
        stopping = new CancellationTokenSource();
        Running = true;
        if (!watching) browser.On(nameof(Browser.ActiveID), Unhouse);
        watching = true;
        var token = stopping.Token;
        _ = Task.Run(() => Accept(token));
    }

    public void Stop()
    {
        if (!Running) return;
        Running = false;
        stopping?.Cancel();
        // The tabs a script left open go with it.
        if (browser is { } b)
            UI.Do(() => { foreach (var tab in b.Tabs.Where(t => t.Bench).ToList()) b.Close(tab); });
    }

    /// One pipe instance waiting at a time; each connection is answered on
    /// its own while the next one waits. CurrentUserOnly: only this Windows
    /// user can connect, and nobody else can have put up a pipe by this name
    /// first to listen in.
    private async Task Accept(CancellationToken token)
    {
        while (!token.IsCancellationRequested)
        {
            var pipe = new System.IO.Pipes.NamedPipeServerStream(
                PipeName, System.IO.Pipes.PipeDirection.InOut, System.IO.Pipes.NamedPipeServerStream.MaxAllowedServerInstances,
                System.IO.Pipes.PipeTransmissionMode.Byte,
                System.IO.Pipes.PipeOptions.Asynchronous | System.IO.Pipes.PipeOptions.CurrentUserOnly);
            try { await pipe.WaitForConnectionAsync(token); }
            catch
            {
                await pipe.DisposeAsync();
                if (token.IsCancellationRequested) break;
                Log.Write("bench: couldn't listen");
                await Task.Delay(1000, CancellationToken.None);
                continue;
            }
            _ = Task.Run(() => Serve(pipe));
        }
    }

    // MARK: - one connection

    /// Reads until a newline, hands the line to the UI thread, writes the
    /// answer, closes.
    private async Task Serve(System.IO.Pipes.NamedPipeServerStream client)
    {
        await using var _ = client;
        JsonObject answer;
        try
        {
            var line = await ReadLine(client);
            answer = line == null ? Error("request too long")
                : JsonNode.Parse(line) is JsonObject request ? await Ask(request)
                : Error("not a JSON object");
        }
        catch (JsonException) { answer = Error("not a JSON object"); }
        catch (Exception e) { answer = Error(e.Message); }
        try
        {
            var bytes = Encoding.UTF8.GetBytes(answer.ToJsonString(Writing) + "\n");
            await client.WriteAsync(bytes);
            await client.FlushAsync();
            client.WaitForPipeDrain();
        }
        catch { }
    }

    private static readonly JsonSerializerOptions Writing = new() { Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping };

    /// One line, or null for a line that never ends — which is not a request.
    private static async Task<string?> ReadLine(Stream client)
    {
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(30));
        var bytes = new List<byte>();
        var chunk = new byte[65536];
        while (true)
        {
            var count = await client.ReadAsync(chunk, timeout.Token);
            if (count <= 0) break;
            var newline = Array.IndexOf(chunk, (byte)'\n', 0, count);
            bytes.AddRange(chunk.AsSpan(0, newline >= 0 ? newline : count).ToArray());
            if (bytes.Count > 4_000_000) return null;
            if (newline >= 0) break;
        }
        return Encoding.UTF8.GetString(bytes.ToArray());
    }

    /// The request, answered on the UI thread, once.
    private Task<JsonObject> Ask(JsonObject request)
    {
        var done = new TaskCompletionSource<JsonObject>(TaskCreationOptions.RunContinuationsAsynchronously);
        UI.Do(() =>
        {
            try { Handle(request, reply => done.TrySetResult(reply)); }
            catch (Exception e) { done.TrySetResult(Error(e.Message)); }
        });
        return done.Task;
    }

    [DllImport("user32.dll")]
    private static extern IntPtr GetForegroundWindow();

    // MARK: - the commands

    private static readonly string[] Commands =
    [
        "tabs", "open", "go", "close", "wait", "sleep", "select", "text", "eval", "click", "type", "submit", "tap",
        "shot", "probe", "key", "press", "resize", "hit", "space", "strip", "column", "ui", "fish", "field",
    ];

    /// What a script may open: anything the field would take, and an
    /// extension's own pages, which a script testing one needs.
    private static Uri? Reachable(string text) =>
        Address.Url(text) ?? (Uri.TryCreate(text, UriKind.Absolute, out var url) && url.Scheme == "chrome-extension" ? url : null);

    private void Handle(JsonObject request, Action<JsonObject> given)
    {
        // One answer, and always one: a page that never replies to a script
        // would otherwise hold the bench — every later command waits behind it.
        var answered = false;
        void answer(JsonObject reply)
        {
            if (answered) return;
            answered = true;
            given(reply);
        }
        var verb = Str(request, "do") ?? "";
        var patience = verb == "wait" ? (Num(request, "seconds") ?? 30) + 5 : 25;
        UI.After(patience, () => answer(Error($"no answer within {(int)patience} s")));
        if (browser is not { } b)
        {
            answer(Error("no browser"));
            return;
        }

        switch (verb)
        {
            // One call to the engine, as the tool pages will make them. Test
            // runs only for now: its commands include ones that shred files.
            case "engine":
            {
                if (!Store.Testing) { answer(Error("engine only works on a --test run for now")); return; }
                if (Str(request, "method") is not { } method) { answer(Error("engine needs a method")); return; }
                var args = request["params"]?.DeepClone();
                _ = Task.Run(async () =>
                {
                    try
                    {
                        var result = await Engine.Client.CallAsync(method, args);
                        UI.Do(() => answer(new JsonObject { ["result"] = result }));
                    }
                    catch (Exception e)
                    {
                        UI.Do(() => answer(Error(e.Message)));
                    }
                });
                break;
            }

            case "field":
            {
                // Typed into the field as a person would, one keystroke at a
                // time with `keys`; what it offers once the engine has
                // answered; then Enter, or a row picked and pressed. Only on
                // a SEARCH_PROBE run: it opens files and starts apps.
                if (!Store.Testing) { answer(Error("field only works on a --test run — it would open things in your browser")); return; }
                _ = Field(b, request, answer);
                break;
            }

            case "clip":
            {
                // Ctrl+Shift+V's list, as a person would use it: open (where
                // the caret is now decides where Enter pastes), type to
                // narrow, down/up, enter (paste) or go (Shift+Enter), close.
                // `secret TEXT` copies the way Search copies a password, which
                // no clipboard history may keep. Only on a SEARCH_PROBE run:
                // it pastes into pages and writes the clipboard. What was
                // copied before this run began is never reported.
                if (!Store.Testing) { answer(Error("clip only works on a --test run — it pastes into your pages")); return; }
                var popup = App.Window?.Clips;
                switch (Str(request, "act"))
                {
                    case "open": if (!b.Clipping) b.ShowClipboard(); break;
                    case "close": b.HideClipboard(); break;
                    case "type": popup?.Type(Str(request, "text") ?? ""); break;
                    case "down": popup?.Step(1); break;
                    case "up": popup?.Step(-1); break;
                    case "enter": popup?.Enter(); break;
                    case "go": popup?.Enter(go: true); break;
                    case "secret": if (!QuietCopy.Copy(Str(request, "text") ?? "")) { answer(Error("the clipboard was busy")); return; } break;
                    // Everything but what's pinned — same as Settings ›
                    // Clipboard's own Clear — so a test can check the
                    // thumbnail cache actually drops with the entries.
                    case "clear": _ = ClipHistory.Clear(); break;
                    case null: break;
                    default: answer(Error("clip acts are open, close, type, down, up, enter, go, secret, clear")); return;
                }
                // The list reloads when the engine says the history changed.
                UI.After(Num(request, "wait") ?? 0.5, () =>
                {
                    var now = App.Window?.Clips;
                    var reply = new JsonObject
                    {
                        ["on"] = ClipHistory.On,
                        ["paused"] = ClipHistory.Paused,
                        ["open"] = b.Clipping,
                        ["aim"] = b.ClipAim,
                        ["kept"] = SearchKit.Field.ClipList.Since(ClipHistory.Last, RunStartedMs).Count,
                        ["announced"] = b.Announcement,
                        ["typed"] = b.Typed,
                        ["thumbnails"] = ClipPopup.CachedThumbnails,
                    };
                    if (now != null)
                    {
                        var (rows, picked) = now.Seen;
                        // Rows from before the run show as their place only.
                        reply["rows"] = new JsonArray([.. rows.Select(r => (JsonNode)(r.CapturedMs >= RunStartedMs ? r.Row : "(from before this run)"))]);
                        reply["picked"] = picked;
                        reply["drawMs"] = Math.Round(now.DrawMs, 2);
                    }
                    if (b.LastPaste is { } last) reply["last"] = new JsonObject { ["kind"] = last.Kind, ["aim"] = last.Aim };
                    answer(reply);
                });
                break;
            }

            case "tabs":
                answer(new JsonObject { ["tabs"] = new JsonArray([.. b.Tabs.Select(t => (JsonNode)Describe(t))]) });
                break;

            case "open":
            {
                if (Str(request, "url") is not { } text || Reachable(text) is not { } url) { answer(Error("open needs a url")); return; }
                var tab = b.BenchOpen(url);
                Starting(tab);
                House(tab);
                answer(Describe(tab));
                break;
            }

            case "go":
            {
                if (Find(request) is not { } tab) { answer(Missing(request)); return; }
                if (Str(request, "url") is not { } text || Reachable(text) is not { } url) { answer(Error("go needs a url")); return; }
                b.Go(tab, url);
                Starting(tab);
                answer(Describe(tab));
                break;
            }

            case "close":
            {
                if (Str(request, "id") == "all")
                {
                    var mine = b.Tabs.Concat(b.ParkedTabs).Where(t => t.Bench).ToList();
                    foreach (var tab in mine) Drop(tab);
                    answer(new JsonObject { ["closed"] = mine.Count });
                    return;
                }
                if (Find(request) is not { } one) { answer(Missing(request)); return; }
                if (!one.Bench) { answer(Error("not a bench tab — only tabs the bench opened can be closed from here")); return; }
                Drop(one);
                answer(new JsonObject { ["closed"] = 1 });
                break;
            }

            case "wait":
            {
                if (Find(request) is not { } tab) { answer(Missing(request)); return; }
                Wait(tab, DateTime.UtcNow.AddSeconds(Num(request, "seconds") ?? 20), answer);
                break;
            }

            case "sleep":
            {
                // Now rather than after half an hour, but past every other
                // check a tab has to clear — the answer says which one kept it
                // awake.
                if (Find(request) is not { } tab) { answer(Missing(request)); return; }
                _ = SleepNow(b, tab, answer);
                break;
            }

            case "select":
            {
                // Picking a tab takes the window over, which the bench never
                // does to someone using it: only on a SEARCH_PROBE run.
                if (!Store.Testing) { answer(Error("select only works on a --test run — it would take your window over")); return; }
                if (Find(request) is not { } tab) { answer(Missing(request)); return; }
                b.Select(tab);
                answer(Describe(tab));
                break;
            }

            case "fish":
            {
                // A scam warning's buttons, pressed: back, continue, real (go
                // to the real site), ok (close the quiet line). They act on
                // the tab in front, so the tab is brought there first — test
                // runs only, like select.
                if (!Store.Testing) { answer(Error("fish only works on a --test run — it takes your window over")); return; }
                if (Find(request) is not { } tab) { answer(Missing(request)); return; }
                b.Select(tab);
                switch (Str(request, "act"))
                {
                    case "back": b.LeaveScam(); break;
                    case "continue": b.ContinueToScam(); break;
                    case "real": b.OpenRealSite(); break;
                    case "ok": b.QuietCaution(); break;
                    default: answer(Error("fish needs back, continue, real or ok")); return;
                }
                Starting(tab);
                answer(Describe(tab));
                break;
            }

            case "text":
            {
                if (Find(request) is not { } tab) { answer(Missing(request)); return; }
                House(tab);
                Script(tab, "document.body ? document.body.innerText : ''", (value, error) =>
                {
                    if (error != null) { answer(Error(error)); return; }
                    var text = value?.ValueKind == JsonValueKind.String ? value.Value.GetString() ?? "" : "";
                    var cut = text.Length > 120_000;
                    if (cut) text = text[..120_000];
                    answer(new JsonObject { ["text"] = text, ["truncated"] = cut, ["url"] = tab.Address?.AbsoluteUri ?? "", ["title"] = tab.Title });
                });
                break;
            }

            case "eval":
            {
                if (Find(request) is not { } tab) { answer(Missing(request)); return; }
                if (Str(request, "js") is not { } js) { answer(Error("eval needs js")); return; }
                House(tab);
                Script(tab, js, (value, error) =>
                {
                    if (error != null) { answer(Error(error)); return; }
                    answer(new JsonObject { ["value"] = value is { } v ? JsonNode.Parse(v.GetRawText()) : null });
                });
                break;
            }

            case "tap":
            {
                // A real click on an element, delivered to the page as mouse
                // input through DevTools — trusted, as a hand's is — where
                // `click` only runs element.click() in the page, which a
                // password manager, for one, is right to ignore. `text=Sign
                // in` picks a button or link by its words. Only on a
                // SEARCH_PROBE run.
                if (!Store.Testing) { answer(Error("tap only works on a --test run — it would click in your page")); return; }
                if (Find(request) is not { } tab || Str(request, "selector") is not { } selector) { answer(Missing(request)); return; }
                House(tab);
                Script(tab, Locate(selector), async (value, error) =>
                {
                    if (value is not { ValueKind: JsonValueKind.Array } point || point.GetArrayLength() != 2 || tab.Core is not { } core)
                    {
                        answer(Error(error ?? $"nothing matches {selector}"));
                        return;
                    }
                    var x = point[0].GetDouble();
                    var y = point[1].GetDouble();
                    try
                    {
                        foreach (var type in new[] { "mousePressed", "mouseReleased" })
                        {
                            var input = new JsonObject { ["type"] = type, ["x"] = x, ["y"] = y, ["button"] = "left", ["clickCount"] = 1 };
                            await core.CallDevToolsProtocolMethodAsync("Input.dispatchMouseEvent", input.ToJsonString());
                        }
                        answer(new JsonObject { ["ok"] = true, ["at"] = new JsonArray((int)x, (int)y) });
                    }
                    catch (Exception e) { answer(Error(e.Message)); }
                });
                break;
            }

            case "click":
            case "type":
            case "submit":
            {
                if (Find(request) is not { } tab) { answer(Missing(request)); return; }
                if (Str(request, "selector") is not { } selector) { answer(Error($"{verb} needs a selector")); return; }
                House(tab);
                Script(tab, Act(verb, selector, Str(request, "text") ?? ""), (value, error) =>
                {
                    if (error != null) { answer(Error(error)); return; }
                    var said = value?.ValueKind == JsonValueKind.String ? value.Value.GetString() : "?";
                    answer(said == "ok" ? new JsonObject { ["ok"] = true } : Error(said ?? "?"));
                });
                break;
            }

            case "shot":
            {
                if (Find(request) is not { } tab) { answer(Missing(request)); return; }
                var fresh = House(tab);
                var path = Str(request, "path") ?? Path.Combine(Path.GetTempPath(), $"search-bench-{Short(tab)}.png");
                var width = Num(request, "width");
                // A view just uncovered is given a moment to paint.
                tab.WhenReady(core => UI.After(fresh ? 0.3 : 0, () => _ = Shoot(core, path, width, answer)));
                break;
            }

            case "probe":
                answer(Probe(b));
                break;

            case "key":
            {
                // Keys pressed on a tab, as real key events handed to its page
                // through DevTools. Only on a SEARCH_PROBE run: it types into
                // a page.
                if (!Store.Testing) { answer(Error("key only works on a --test run — it would type into your page")); return; }
                if (Find(request) is not { } tab || Str(request, "text") is not { } text) { answer(Missing(request)); return; }
                House(tab);
                tab.WhenReady(core => _ = Keys(core, text, answer));
                break;
            }

            case "press":
            {
                // A key pressed on the app as a whole — its own shortcuts see
                // it, as they do a real press; `key` goes straight to a page
                // instead. The code is a Windows virtual-key code. Only on a
                // SEARCH_PROBE run.
                if (!Store.Testing) { answer(Error("press only works on a --test run — it would press keys in your browser")); return; }
                if (Int(request, "code") is not { } code) { answer(Error("press needs a virtual-key code")); return; }
                var mods = (request["mods"] as JsonArray)?.Select(m => m?.GetValue<string>() ?? "").ToHashSet() ?? [];
                var taken = Shortcuts.Take((VirtualKey)code,
                    ctrl: mods.Contains("ctrl") || mods.Contains("cmd"),
                    shift: mods.Contains("shift"),
                    alt: mods.Contains("alt") || mods.Contains("opt"),
                    repeat: mods.Contains("repeat"),
                    inPage: false);
                UI.After(0.4, () => answer(new JsonObject { ["taken"] = taken, ["active"] = b.Active is { } a ? Short(a) : "" }));
                break;
            }

            case "resize":
                Resize(request, answer);
                break;

            case "hit":
                Hit(request, answer);
                break;

            case "film":
                // The Mac drew the window frame by frame; WinUI composes on
                // the GPU, where a drawing taken mid-animation shows where
                // things will end up rather than where they are.
                answer(Error("film isn't in the Windows bench"));
                break;

            case "strip":
            case "column":
                Picture(verb, request, b, answer);
                break;

            case "space":
                Space(request, b, answer);
                break;

            case "ui":
                // Open or close the app's own panels, to reproduce what a
                // person did without a person.
                if (Bool(request, "settings") is { } settings) b.Tuning = settings;
                if (Bool(request, "passwords") is { } passwords) b.Managing = passwords;
                if (Bool(request, "welcome") is { } welcome) b.Welcoming = welcome;
                if (Bool(request, "history") is { } history) b.Recalling = history;
                if (Bool(request, "downloads") is { } downloads) b.Hoarding = downloads;
                if (Bool(request, "bookmarks") is { } bookmarks) b.Bookmarking = bookmarks;
                if (Bool(request, "hidden") is { } hidden) b.Reviewing = hidden;
                if (Str(request, "look") is { } look && Enum.TryParse<Look>(look, ignoreCase: true, out var chosen)) b.Prefs.Look = chosen;
                if (Bool(request, "sidebar") is { } sidebar) b.Prefs.Sidebar = sidebar;
                if (Bool(request, "spaces") is { } spaces) b.Prefs.UsesSpaces = spaces;
                if (Bool(request, "hides") is { } hides) b.Prefs.SideHides = hides;
                if (Bool(request, "folded") is { } folded) b.Folded = folded;
                if (Bool(request, "peek") is { } peek) b.Peeking = peek;
                // Settings › Search, test runs only: `folders` is the list
                // (`;` between folders, `none` for none), `contents` Search
                // inside files.
                if (Store.Testing && Str(request, "folders") is { } folders)
                    b.Prefs.SearchFolders = folders == "none" ? [] : folders.Split(';', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
                if (Store.Testing && Bool(request, "contents") is { } contents) b.Prefs.FileContents = contents;
                answer(new JsonObject { ["ok"] = true });
                break;

            case "extensions":
            case var ext when ext.StartsWith("ext-"):
                answer(Error("extensions aren't driven from the Windows bench yet"));
                break;

            default:
                answer(new JsonObject
                {
                    ["error"] = $"unknown command “{verb}”",
                    ["commands"] = new JsonArray([.. Commands.Select(c => (JsonNode)c)]),
                });
                break;
        }
    }

    // MARK: - the field

    /// Each keystroke's time on the UI thread (the field's own work and its
    /// list redrawn), the rows once every engine source has answered, and
    /// what Enter did.
    /// When this run of Search began: clipboard entries from before it are
    /// never reported (a test world may keep what was copied last time).
    private static readonly long RunStartedMs = new DateTimeOffset(System.Diagnostics.Process.GetCurrentProcess().StartTime).ToUnixTimeMilliseconds();

    private static async Task Field(Browser b, JsonObject request, Action<JsonObject> answer)
    {
        var text = Str(request, "text") ?? "";
        // `ctrlk`: the switcher's field (Ctrl+K), typed into the same way.
        if (Bool(request, "ctrlk") == true) b.Summon();
        else
        {
            b.Editing = true;
            b.Typed = "";
        }
        var times = new JsonArray();
        var clock = new System.Diagnostics.Stopwatch();
        IEnumerable<string> steps = Bool(request, "keys") == true ? Enumerable.Range(1, text.Length).Select(n => text[..n]) : [text];
        // Into the field on screen, as a key would put it there, and taken
        // from it when WinUI says it changed; `direct` sets the text from
        // the browser's side instead, as Ctrl+L does.
        var box = Bool(request, "direct") == true ? null : App.Window?.Field;
        foreach (var step in steps)
        {
            if (box != null)
            {
                box.Key(step);
                for (var wait = 0; wait < 100 && b.Typed != step; wait++) await Task.Delay(5);
                times.Add((JsonNode)Math.Round(box.KeyMs, 3));
                continue;
            }
            clock.Restart();
            b.Typed = step;
            times.Add((JsonNode)Math.Round(clock.Elapsed.TotalMilliseconds, 3));
        }
        // Whatever WinUI has still to say about the field's text.
        await Task.Delay(30);
        clock.Restart();
        await Task.WhenAny(b.Reach.WhenSettled, Task.Delay(TimeSpan.FromSeconds(Num(request, "seconds") ?? 5)));
        var settled = clock.Elapsed.TotalMilliseconds;
        // The last rows are posted to this thread as they land.
        await Task.Delay(50);
        // What was copied before this run began is never reported.
        var before = (await ClipHistory.List()).Where(e => e.CapturedMs < RunStartedMs).Select(e => e.Id.ToString(System.Globalization.CultureInfo.InvariantCulture)).ToHashSet();
        var reply = new JsonObject
        {
            ["keystrokes"] = times,
            ["settledMs"] = Math.Round(settled, 1),
            ["rows"] = new JsonArray([.. b.Offers.Select((o, i) => (o, i))
                .Where(x => x.o.Row is not { Action: SearchKit.Field.RowAction.CopyClip } clip || !before.Contains(clip.Target))
                .Select(x => (JsonNode)Offered(x.o, x.i))]),
            ["picked"] = b.Picked,
            ["ending"] = b.Ending,
            ["typed"] = b.Typed,
        };
        static JsonObject Offered(Suggestion o, int i) => new()
            {
                ["i"] = i,
                ["kind"] = o.Kind.ToString(),
                ["title"] = o.Key,
                ["detail"] = o.Title,
                ["group"] = o.Row?.Origin.ToString(),
                ["action"] = o.Row?.Action.ToString(),
                ["top"] = o.Row?.Group == SearchKit.Field.Group.TopHit,
            };
        var pick = Int(request, "pick");
        if (pick != null || Bool(request, "enter") == true)
        {
            if (pick is { } n) b.Picked = n;
            var tabs = b.Tabs.Count;
            b.Submit();
            await Task.Delay(TimeSpan.FromSeconds(Num(request, "after") ?? 0.8));
            reply["after"] = new JsonObject
            {
                ["said"] = b.Announcement,
                ["newTabs"] = b.Tabs.Count - tabs,
                ["active"] = b.Active?.Address?.AbsoluteUri,
                ["typed"] = b.Typed,
                ["refusals"] = b.Refusals,
            };
        }
        answer(reply);
    }

    // MARK: - tabs

    private Tab? Find(JsonObject request)
    {
        if (browser is not { } b || Str(request, "id")?.ToLowerInvariant() is not { Length: > 0 } reference) return null;
        return b.Tabs.Concat(b.ParkedTabs).FirstOrDefault(t => t.Id.ToString().StartsWith(reference, StringComparison.OrdinalIgnoreCase));
    }

    private static JsonObject Missing(JsonObject request) => Error($"no tab “{Str(request, "id") ?? ""}” — see tabs");

    /// Closed wherever it is: in the row on screen, or parked with another
    /// space's.
    private void Drop(Tab tab)
    {
        if (browser is not { } b) return;
        if (b.Tabs.Contains(tab)) { b.Close(tab); return; }
        foreach (var row in b.ParkedRows.Values)
            if (row.Tabs.Remove(tab)) tab.Close();
    }

    private JsonObject Describe(Tab tab) => new()
    {
        ["id"] = Short(tab),
        ["url"] = tab.Address?.AbsoluteUri ?? "",
        ["title"] = tab.Title,
        ["name"] = tab.Name ?? "",
        ["loading"] = tab.Loading || starting.ContainsKey(tab.Id),
        ["hollow"] = Hollow(tab),
        ["view"] = tab.Core?.Source ?? "",
        ["bench"] = tab.Bench,
        ["active"] = tab.Id == browser?.ActiveID,
        ["asleep"] = tab.Asleep,
        ["dozing"] = tab.Dozing,
        ["trouble"] = tab.Failure?.Kind.ToString(),
        ["fish"] = Fish(tab),
        ["shield"] = ShieldOf(tab),
    };

    /// FishCatcher's verdict on the tab's page, for scripts testing it: the
    /// level, the score, the reasons in words, and how long the address
    /// check took.
    private static JsonObject? Fish(Tab tab)
    {
        if (tab.Fish is not { } verdict) return null;
        return new JsonObject
        {
            ["level"] = verdict.Level.ToString().ToLowerInvariant(),
            ["score"] = verdict.Score,
            ["reasons"] = new JsonArray([.. verdict.Reasons.Select(r => (JsonNode)r)]),
            ["keys"] = new JsonArray([.. verdict.Signals.Select(s => (JsonNode)s.Key)]),
            ["realSite"] = verdict.RealSite,
            ["warns"] = verdict.Warns,
            ["caution"] = tab.Caution?.Text,
            ["ms"] = Math.Round(tab.FishMs, 3),
        };
    }

    /// Shields on the tab's page: requests refused since it started loading,
    /// how many crossed over to be decided and the UI-thread time that took,
    /// whether its site is paused, and how many rules the list in force has
    /// (0: the built-in 44 domains).
    private static JsonObject ShieldOf(Tab tab) => new()
    {
        ["blocked"] = tab.Blocked,
        ["paused"] = Shield.Shared.IsPaused(Curtain.Host(tab.Address)),
        ["seen"] = tab.ShieldSeen,
        ["ms"] = Math.Round(tab.ShieldMs, 3),
        ["rules"] = Shield.Shared.List is { } list ? list.Stats.NetworkRules + list.Stats.NetworkExceptions + list.Stats.CosmeticRules + list.Stats.CosmeticExceptions : 0,
    };

    /// True when the view holds nothing — never loaded, or emptied — while
    /// the tab still names a page. The white page, in other words.
    private static bool Hollow(Tab tab)
    {
        if (tab.Built == null || tab.Core is not { } core) return tab.Address != null;
        var there = core.Source;
        return (string.IsNullOrEmpty(there) || there == "about:blank") && tab.Pending == null && tab.Address != null;
    }

    public static string Short(Tab tab) => tab.Id.ToString("N")[..8];

    /// Tabs sent somewhere whose engine hasn't said it started yet. WebView2
    /// starts a page a moment after it is asked to, and a `wait` in that
    /// moment would otherwise find it not loading and answer at once.
    private readonly Dictionary<Guid, DateTime> starting = [];

    private void Starting(Tab tab)
    {
        // Stopped before the engine heard of it (a scam warning): nothing
        // is going to start.
        if (tab.Failure != null) return;
        starting[tab.Id] = DateTime.UtcNow;
        void Watch(object? sender, System.ComponentModel.PropertyChangedEventArgs e)
        {
            if (e.PropertyName is not (nameof(Tab.Loading) or nameof(Tab.Failure))) return;
            starting.Remove(tab.Id);
            tab.PropertyChanged -= Watch;
        }
        tab.PropertyChanged += Watch;
    }

    /// Once the page has stopped loading, or the time is up.
    private void Wait(Tab tab, DateTime limit, Action<JsonObject> answer)
    {
        if (starting.TryGetValue(tab.Id, out var since) && (DateTime.UtcNow - since).TotalSeconds > 15) starting.Remove(tab.Id);
        if (!tab.Loading && !starting.ContainsKey(tab.Id) && tab.Address != null)
        {
            // A beat for the document's own scripts to settle.
            UI.After(0.25, () =>
            {
                var out_ = Describe(tab);
                if (tab.Failure is { } failure) out_["failure"] = failure.Headline;
                answer(out_);
            });
            return;
        }
        if (DateTime.UtcNow >= limit)
        {
            var out_ = Describe(tab);
            out_["timeout"] = true;
            answer(out_);
            return;
        }
        UI.After(0.1, () => Wait(tab, limit, answer));
    }

    private static async Task SleepNow(Browser b, Tab tab, Action<JsonObject> answer)
    {
        var said = await b.Sleep(tab);
        answer(new JsonObject { ["said"] = said, ["asleep"] = tab.Asleep, ["dozing"] = tab.Dozing });
    }

    // MARK: - the page, unseen

    /// A page nobody is looking at has to be drawn to be laid out and
    /// pictured at all. A bench tab's view is kept visible beneath the page
    /// on screen, at no opacity and taking no clicks; picking its tab puts it
    /// back as any other page. Answers whether it had been out of sight.
    private bool House(Tab tab)
    {
        if (!tab.Bench || tab.Id == browser?.ActiveID) return false;
        var view = tab.Web;
        var hidden = view.Visibility != Visibility.Visible || view.Opacity > 0;
        // Once it has started: starting, the tab puts its view the way it
        // should be, which for a tab not on screen is out of sight.
        tab.WhenReady(_ =>
        {
            if (tab.Id == browser?.ActiveID || tab.Built != view) return;
            view.Opacity = 0;
            view.IsHitTestVisible = false;
            Canvas.SetZIndex(view, -1);
            view.Visibility = Visibility.Visible;
        });
        return hidden;
    }

    /// A bench tab you went to is a page like any other again.
    private void Unhouse()
    {
        if (browser?.Active is not { Bench: true, Built: { } view }) return;
        view.Opacity = 1;
        view.IsHitTestVisible = true;
    }

    /// Runs a script once the page has started, and hands back what it
    /// returned, or what it threw.
    private static void Script(Tab tab, string js, Action<JsonElement?, string?> done)
    {
        tab.WhenReady(async core =>
        {
            try
            {
                var result = await core.ExecuteScriptWithResultAsync(js);
                if (!result.Succeeded)
                {
                    done(null, result.Exception?.Message ?? "the script threw");
                    return;
                }
                var json = result.ResultAsJson;
                if (string.IsNullOrEmpty(json)) { done(null, null); return; }
                using var doc = JsonDocument.Parse(json);
                done(doc.RootElement.Clone(), null);
            }
            catch (Exception e) { done(null, e.Message); }
        });
    }

    private static async Task Shoot(CoreWebView2 core, string path, double? width, Action<JsonObject> answer)
    {
        try
        {
            using var picture = new InMemoryRandomAccessStream();
            await core.CapturePreviewAsync(CoreWebView2CapturePreviewImageFormat.Png, picture);
            picture.Seek(0);
            var decoder = await BitmapDecoder.CreateAsync(picture);
            uint w = decoder.PixelWidth, h = decoder.PixelHeight;
            IRandomAccessStream written = picture;
            using var scaled = new InMemoryRandomAccessStream();
            if (width is { } wanted && wanted > 0 && (uint)wanted != w && w > 0)
            {
                var encoder = await BitmapEncoder.CreateForTranscodingAsync(scaled, decoder);
                h = (uint)Math.Max(1, Math.Round(h * wanted / w));
                w = (uint)wanted;
                encoder.BitmapTransform.ScaledWidth = w;
                encoder.BitmapTransform.ScaledHeight = h;
                encoder.BitmapTransform.InterpolationMode = BitmapInterpolationMode.Fant;
                await encoder.FlushAsync();
                written = scaled;
            }
            await Save(written, path);
            answer(new JsonObject { ["path"] = path, ["width"] = w, ["height"] = h });
        }
        catch (Exception e) { answer(Error(e.Message)); }
    }

    private static async Task Save(IRandomAccessStream stream, string path)
    {
        stream.Seek(0);
        var folder = Path.GetDirectoryName(Path.GetFullPath(path));
        if (!string.IsNullOrEmpty(folder)) Directory.CreateDirectory(folder);
        await using var file = File.Create(path);
        await stream.AsStreamForRead().CopyToAsync(file);
    }

    /// Each character pressed and let go, as a keyboard would.
    private static async Task Keys(CoreWebView2 core, string text, Action<JsonObject> answer)
    {
        try
        {
            foreach (var character in text.EnumerateRunes())
            {
                var chars = character.ToString();
                var letter = chars.Length == 1 && char.IsAsciiLetterOrDigit(chars[0]);
                var code = letter ? (char.IsDigit(chars[0]) ? "Digit" : "Key") + char.ToUpperInvariant(chars[0]) : "";
                var vk = letter ? (int)char.ToUpperInvariant(chars[0]) : chars == " " ? 32 : 0;
                var down = new JsonObject { ["type"] = "keyDown", ["text"] = chars, ["key"] = chars, ["code"] = code, ["windowsVirtualKeyCode"] = vk };
                var up = new JsonObject { ["type"] = "keyUp", ["key"] = chars, ["code"] = code, ["windowsVirtualKeyCode"] = vk };
                await core.CallDevToolsProtocolMethodAsync("Input.dispatchKeyEvent", down.ToJsonString());
                await core.CallDevToolsProtocolMethodAsync("Input.dispatchKeyEvent", up.ToJsonString());
            }
            answer(new JsonObject { ["typed"] = text });
        }
        catch (Exception e) { answer(Error(e.Message)); }
    }

    // MARK: - the window

    /// The state of the window itself, for the bug that is not in a page:
    /// which panels are up, whether a dialog has the app, and the window.
    private static JsonObject Probe(Browser b)
    {
        var modal = "";
        if (App.Root?.XamlRoot is { } xamlRoot)
            foreach (var popup in VisualTreeHelper.GetOpenPopupsForXamlRoot(xamlRoot))
                if (popup.Child is ContentDialog dialog) modal = $"ContentDialog “{dialog.Title}”";
        var out_ = new JsonObject
        {
            ["settings"] = b.Tuning,
            ["welcome"] = b.Welcoming,
            ["passwords"] = b.Managing,
            ["history"] = b.Recalling,
            ["downloads"] = b.Hoarding,
            ["bookmarks"] = b.Bookmarking,
            ["hidden"] = b.Reviewing,
            ["field"] = b.Editing,
            ["suggesting"] = b.Suggesting != null,
            ["offering"] = b.Offering != null,
            ["modal"] = modal,
            ["look"] = b.Prefs.Look.Raw(),
            ["appearance"] = Palette.Dark ? "dark" : "light",
            ["sidebar"] = b.Prefs.Sidebar,
            ["folded"] = b.Folded,
            ["peeking"] = b.Peeking,
            ["sideHides"] = b.Prefs.SideHides,
            ["spaces"] = b.Prefs.UsesSpaces,
            ["space"] = b.CurrentSpace.Name,
            ["makingSpace"] = b.MakingSpace,
            ["tabs"] = b.Tabs.Count,
            ["inspector"] = new JsonArray([.. b.Tabs.Select(t => t.Core).OfType<CoreWebView2>().Select(c => (JsonNode)c.Settings.AreDevToolsEnabled)]),
        };
        if (App.Window is { } window)
        {
            var handle = WinRT.Interop.WindowNative.GetWindowHandle(window);
            var app = window.AppWindow;
            out_["key"] = GetForegroundWindow() == handle ? $"MainWindow “{window.Title}”" : "";
            out_["windows"] = new JsonArray(new JsonObject
            {
                ["kind"] = "MainWindow",
                ["title"] = window.Title,
                ["visible"] = app.IsVisible,
                ["frame"] = new JsonArray(app.Position.X, app.Position.Y, app.Size.Width, app.Size.Height),
                ["presenter"] = app.Presenter.Kind.ToString(),
                ["maximized"] = App.Presenter?.State == Microsoft.UI.Windowing.OverlappedPresenterState.Maximized,
            });
        }
        return out_;
    }

    private static double Scale => App.Root?.XamlRoot?.RasterizationScale ?? 1;

    /// The window taken to another size in steps, a frame apart, the way a
    /// hand drags its corner. It moves the window, so only on a SEARCH_PROBE
    /// run. Sizes are the window's own points, as everything else here is.
    private static void Resize(JsonObject request, Action<JsonObject> answer)
    {
        if (!Store.Testing) { answer(Error("resize only works on a --test run — it would move your window")); return; }
        if (App.Window is not { } window || Num(request, "width") is not { } width || Num(request, "height") is not { } height)
        {
            answer(Error("resize needs a width and a height"));
            return;
        }
        var steps = Math.Max(1, Int(request, "steps") ?? 12);
        var app = window.AppWindow;
        var from = app.Size;
        var scale = Scale;
        void Step(int n)
        {
            var t = (double)n / steps;
            var w = from.Width + (width * scale - from.Width) * t;
            var h = from.Height + (height * scale - from.Height) * t;
            app.Resize(new Windows.Graphics.SizeInt32((int)Math.Round(w), (int)Math.Round(h)));
            if (n < steps) UI.After(0.016, () => Step(n + 1));
            else UI.After(0.3, () => answer(new JsonObject
            {
                ["size"] = new JsonArray((int)Math.Round(app.Size.Width / scale), (int)Math.Round(app.Size.Height / scale)),
            }));
        }
        Step(1);
    }

    /// What a press at a point of the window lands on: the element on top
    /// there and the ones it sits in. Only looked at.
    private static void Hit(JsonObject request, Action<JsonObject> answer)
    {
        if (App.Root is not { } root || Num(request, "x") is not { } x || Num(request, "y") is not { } y)
        {
            answer(Error("hit needs an x and a y"));
            return;
        }
        if (Bool(request, "double") == true || Bool(request, "middle") == true)
        {
            answer(Error("hit … double|middle isn't in the Windows bench"));
            return;
        }
        var under = VisualTreeHelper.FindElementsInHostCoordinates(new Windows.Foundation.Point(x, y), root).ToList();
        string Name(UIElement e) => e is FrameworkElement { Name.Length: > 0 } f ? $"{e.GetType().Name} {f.Name}" : e.GetType().Name;
        answer(new JsonObject
        {
            ["view"] = under.Count > 0 ? Name(under[0]) : "",
            ["under"] = new JsonArray([.. under.Take(8).Select(e => (JsonNode)Name(e))]),
            ["titleBar"] = y <= Metrics.Strip,
        });
    }

    /// The row of tabs across the top, or the column, drawn to a PNG as it is
    /// on screen now.
    private static async void Picture(string verb, JsonObject request, Browser b, Action<JsonObject> answer)
    {
        if (Str(request, "path") is not { } path) { answer(Error($"{verb} needs a path")); return; }
        FrameworkElement? element = verb == "strip"
            ? App.Root?.Children.OfType<TabBar>().FirstOrDefault()
            : App.Root?.Children.OfType<SideBar>().FirstOrDefault();
        if (element == null || element.Visibility != Visibility.Visible || element.ActualWidth <= 0)
        {
            answer(Error(verb == "strip" ? "the row isn't showing — ui sidebar off" : "the column isn't showing — ui sidebar on"));
            return;
        }
        try
        {
            var bitmap = new RenderTargetBitmap();
            await bitmap.RenderAsync(element);
            var pixels = await bitmap.GetPixelsAsync();
            using var stream = new InMemoryRandomAccessStream();
            var encoder = await BitmapEncoder.CreateAsync(BitmapEncoder.PngEncoderId, stream);
            var dpi = 96 * Scale;
            encoder.SetPixelData(BitmapPixelFormat.Bgra8, BitmapAlphaMode.Premultiplied,
                (uint)bitmap.PixelWidth, (uint)bitmap.PixelHeight, dpi, dpi, pixels.ToArray());
            await encoder.FlushAsync();
            await Save(stream, path);
            answer(new JsonObject { ["saved"] = path });
        }
        catch (Exception e) { answer(Error(e.Message)); }
    }

    // MARK: - spaces

    /// The spaces, and switching between them, for a test of what a space
    /// keeps apart. Test runs only: it moves your tabs about.
    private static void Space(JsonObject request, Browser b, Action<JsonObject> answer)
    {
        if (!Store.Testing) { answer(Error("space only works on a --test run")); return; }
        var swipe = SpaceSwipe.Shared;
        swipe.Start(b);
        switch (Str(request, "action") ?? "")
        {
            case "new": b.AddSpace(Str(request, "name") ?? "Test", sharesSignIns: Bool(request, "fresh") != true); break;
            case "go": b.SwitchSpace((Int(request, "index") ?? 1) - 1); break;
            case "delete": b.DeleteSpace(b.SpaceID); break;
            case "swipe":
            case "hold":
            {
                // Two fingers sideways over the column, as the swipe reads
                // them; `hold` leaves them down.
                var dx = Num(request, "dx") ?? -120;
                swipe.Began();
                for (var i = 0; i < 12; i++) swipe.Moved(dx / 12, 0);
                if (Str(request, "action") == "swipe") swipe.Ended();
                break;
            }
            case "release": swipe.Ended(); break;
            case "move":
                if (Int(request, "index") is { } index) b.MoveSpace(b.SpaceID, index - 1);
                break;
        }
        // And the profiles on disk a moment later — what a deleted space
        // should have taken with it.
        UI.After(0.5, () => answer(new JsonObject
        {
            ["on"] = b.Prefs.UsesSpaces,
            ["current"] = b.CurrentSpace.Name,
            ["spaces"] = new JsonArray([.. b.Spaces.Select(s => (JsonNode)new JsonObject
            {
                ["name"] = s.Name,
                ["id"] = s.Id.ToString().ToUpperInvariant(),
                ["icon"] = s.Symbol,
                ["downloads"] = s.Downloads ?? "",
                ["shared"] = s.SharesSignIns == true,
            })]),
            ["parked"] = new JsonArray([.. b.ParkedRows.Select(p => (JsonNode)new JsonObject { [p.Key.ToString().ToUpperInvariant()] = p.Value.Tabs.Count })]),
            ["tabs"] = b.Tabs.Count,
            ["making"] = b.MakingSpace,
            ["swipe"] = b.SwipeTravel,
            ["pages"] = new JsonArray([.. b.Tabs.Concat(b.ParkedTabs).Where(t => t.Built != null).Select(t => (JsonNode)t.Profile)]),
            ["stores"] = new JsonArray([.. Search.Space.Stores().Select(s => (JsonNode)s)]),
            ["erasing"] = new JsonArray([.. Search.Space.Leftovers().Select(s => (JsonNode)s)]),
        }));
    }

    // MARK: - reading a request

    private static JsonObject Error(string message) => new() { ["error"] = message };

    private static string? Str(JsonObject o, string key) =>
        o[key] is JsonValue v && v.TryGetValue<string>(out var s) ? s : null;

    private static double? Num(JsonObject o, string key) =>
        o[key] is JsonValue v && v.TryGetValue<double>(out var d) ? d : null;

    private static int? Int(JsonObject o, string key) => Num(o, key) is { } d ? (int)Math.Round(d) : null;

    private static bool? Bool(JsonObject o, string key) =>
        o[key] is JsonValue v && v.TryGetValue<bool>(out var b) ? b : null;

    // MARK: - page-side helpers

    /// Where an element's middle is, in the page's own points, scrolled into
    /// view first. A selector, or `text=…` for a button or link by its words.
    private static string Locate(string selector) => $$"""
        (function () {
          var s = {{Bridge.Literal(selector)}}, el = null;
          if (s.indexOf('text=') === 0) {
            var want = s.slice(5).trim().toLowerCase();
            el = Array.prototype.find.call(document.querySelectorAll('button, a, [role=button], input[type=submit]'), function (e) {
              return ((e.innerText || e.value || '').trim().toLowerCase()) === want;
            }) || null;
          } else {
            el = document.querySelector(s);
          }
          if (!el) return null;
          el.scrollIntoView({ block: 'center', inline: 'nearest' });
          var r = el.getBoundingClientRect();
          return [r.left + r.width / 2, r.top + r.height / 2];
        })()
        """;

    /// Click, type into, or submit the element a selector names. Typing goes
    /// through the field's own setter and fires the events a keystroke
    /// would, the same as the password filler, so frameworks notice.
    private static string Act(string verb, string selector, string text)
    {
        var sel = Bridge.Literal(selector);
        var txt = Bridge.Literal(text);
        return $$"""
        (function () {
          var el = document.querySelector({{sel}});
          if (!el) return 'nothing matches ' + {{sel}};
          if (el.scrollIntoView) el.scrollIntoView({ block: 'center', inline: 'nearest' });
          var verb = '{{verb}}';
          if (verb === 'click') { el.focus && el.focus(); el.click(); return 'ok'; }
          if (verb === 'submit') {
            var form = el.tagName === 'FORM' ? el : el.form || el.closest('form');
            if (!form) return 'no form around ' + {{sel}};
            if (form.requestSubmit) form.requestSubmit(); else form.submit();
            return 'ok';
          }
          el.focus && el.focus();
          var value = {{txt}};
          if (el.isContentEditable) {
            el.textContent = value;
            el.dispatchEvent(new InputEvent('input', { bubbles: true, data: value, inputType: 'insertText' }));
            return 'ok';
          }
          var proto = el.tagName === 'TEXTAREA' ? window.HTMLTextAreaElement.prototype : window.HTMLInputElement.prototype;
          var setter = Object.getOwnPropertyDescriptor(proto, 'value');
          if (setter && setter.set) setter.set.call(el, value); else el.value = value;
          el.dispatchEvent(new Event('input', { bubbles: true }));
          el.dispatchEvent(new Event('change', { bubbles: true }));
          return 'ok';
        })();
        """;
    }
}
