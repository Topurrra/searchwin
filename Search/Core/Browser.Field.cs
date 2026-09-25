using System.Text.Json.Nodes;
using SearchKit.Engine;
using SearchKit.Field;

namespace Search;

// The field's reach past the browser: files by name and by what's inside
// them, apps, what you copied (`clip:`), and instant answers (`23*47`,
// `10 km to mi`, `sha256 hello`),
// asked of the engine as you type (SearchKit.Field) and shown below the
// field's own rows, which are decided at once, the way they always were.
// What the engine finds arrives a moment later and never moves a row you
// are about to press (see FieldMix and FieldBoard).
public sealed partial class Browser
{
    private FieldModel? field;
    /// The browser's own rows for what's typed: places, the search row,
    /// commands, bangs, open pages.
    private List<Suggestion> mine = [];
    private List<FieldSlot> slots = [];
    private IReadOnlyList<FieldRow> board = [];
    private bool reaching;

    /// The engine's side of the field. Made on the first keystroke, never at
    /// launch; the engine itself starts only when a question needs it.
    public FieldModel Reach => field ??= MakeField();

    private FieldModel MakeField()
    {
        var calls = new EngineCalls(Engine.Client);
        // Files only from folders chosen in Settings › Search, and only while
        // they still are: an index that hasn't caught up with a folder taken
        // away never shows it.
        bool files(FieldQuery _) => Prefs.SearchFolders.Count > 0;
        bool chosen(FieldRow row) => IndexPlan.Covers(Prefs.SearchFolders, row.Target);
        // Hashes and colours are worked out here; sums, units and encodings
        // need the engine.
        List<IEngineSource> sources = [new GatedSource(new AnswerSource(calls), q => Engine.Available || Answers.Plan(q) == null)];
        // No engine beside Search: nothing is ever asked of one, so no
        // keystroke pays for looking.
        if (Engine.Available)
            sources.InsertRange(0,
            [
                new GatedSource(new FileNameSource(calls), files, chosen),
                new GatedSource(new FileContentSource(calls), q => files(q) && Prefs.FileContents, chosen),
                new GatedSource(new AppSource(calls), _ => Prefs.AppsInField),
                // `clip:` and `clipboard` list it; words find a couple of
                // matches among the rest. Secrets only ever by their kind.
                new GatedSource(new ClipboardSource(calls, inField: true), _ => ClipHistory.On),
            ]);
        var model = new FieldModel(
            local: [],
            engine: sources,
            new FieldOptions
            {
                Bangs = Commands.Bangs,
                IsAddress = t => Address.Url(t) != null,
                Post = act => UI.Queue.TryEnqueue(() => act()),
            });
        model.Changed += () =>
        {
            if (!reaching) Recompose();
        };
        return model;
    }

    /// The keystroke, to the engine's side. Its rows come back through Changed.
    private void Ask(string text)
    {
        reaching = true;
        try { Reach.Type(text); }
        finally { reaching = false; }
    }

    /// The browser's own rows, with whatever the engine has found so far.
    private void Show(List<Suggestion> list)
    {
        mine = list;
        Recompose();
    }

    private void Recompose()
    {
        var was = Picked is { } p && p < slots.Count ? slots[p] : (FieldSlot?)null;
        var key = was is { IsLocal: false } s && s.Board < board.Count ? board[s.Board].Key : null;
        board = Reach.Board.Rows;
        slots = FieldMix.Compose(mine.Count, board);
        var offers = new List<Suggestion>(slots.Count);
        foreach (var slot in slots) offers.Add(slot.IsLocal ? mine[slot.Local] : Found(board[slot.Board]));
        // The row that was picked is still the one picked, wherever it is now.
        picked = FieldMix.Find(slots, board, was, key);
        Offers = offers;
        FetchIcons();
    }

    private static Suggestion Found(FieldRow row) =>
        new(row.Title, row.Detail, Suggestion.Nowhere, SuggestionKind.Found) { Row = row };

    /// The board keeps a picked row where it is: its own, or one of ours
    /// below its top hit.
    private void HoldPick()
    {
        if (field == null) return;
        field.Board.Pick(OnBoard(Picked));
    }

    /// The pointer over a row (null: off it). The row under it stays put
    /// while late rows land, the browser's own rows included.
    public void Hover(int? index)
    {
        if (field == null) return;
        field.Board.Hold(OnBoard(index));
    }

    /// The pointer over the list or off it: while it's over, a reserved top
    /// hit that never comes stays as an empty row rather than pulling every
    /// row below it up under the pointer.
    public void OverList(bool over)
    {
        if (field == null) return;
        field.Board.PointerOver = over;
    }

    private int? OnBoard(int? index) =>
        index is { } i && i >= 0 && i < slots.Count ? slots[i].IsLocal ? FieldBoard.Beside : slots[i].Board : null;

    /// Enter with nothing picked belongs to the engine for an answer
    /// (`23*47`) and for a scope (`files: invoice`), whose first row is what
    /// Enter takes.
    private bool Answering => field is { } f && (f.Query.Scope != Scope.All || f.Query.IsAnswer);

    /// `tabs:` and `history:` are those lists; `files:`, `apps:` and `clip:`
    /// have none of the browser's own rows. Null when nothing is scoped.
    private List<Suggestion>? Scoped()
    {
        var query = Reach.Query;
        switch (query.Scope)
        {
            case Scope.All:
                return null;
            case Scope.Tabs:
                return OpenPages(query.Text);
            case Scope.History:
                return query.Text.Length > 0
                    ? History.Suggestions(query.Text, 6)
                    : [.. History.Everything().Take(6).Select(t => new Suggestion(t.Key, t.Title, t.Url, SuggestionKind.Visited))];
            default:
                return [];
        }
    }

    /// The answer, or the scope's first row, waited for briefly if it's still
    /// on its way. No answer is a search after all; an empty scope is refused.
    private async void EnterFound()
    {
        var typed = Typed;
        var query = Reach.Query;
        FieldRow? row = null;
        try { row = await Reach.EnterAsync(TimeSpan.FromMilliseconds(150)); }
        catch { }
        // The typing went on while the answer was on its way.
        if (Typed != typed) return;
        if (row is { IsPending: false })
        {
            Act(row);
            return;
        }
        if (query.Scope != Scope.All)
        {
            Refusals++;
            return;
        }
        SubmitTyped();
    }

    /// What an engine row does: a file opens in a tab (a PDF, a picture, a
    /// text), plays (audio and video) or opens in its own app; an app starts;
    /// an answer is copied, and the line at the bottom says so.
    private void Act(FieldRow row)
    {
        Summoning = false;
        Editing = false;
        Typed = "";
        Picked = null;
        switch (row.Action)
        {
            case RowAction.Copy:
                Copy(row.Target);
                Announce(row.Target.Length <= 32 && !row.Target.Contains('\n') ? $"Copied {row.Target}" : "Copied");
                break;
            case RowAction.OpenInTab:
                OpenFile(row.Target);
                break;
            case RowAction.Play:
                OpenPlayer(row.Target);
                break;
            case RowAction.OpenWithApp:
                Send("open_search_result_path", new JsonObject { ["path"] = row.Target }, "Couldn't open that");
                break;
            case RowAction.Reveal:
                Reveal(row.Target);
                break;
            case RowAction.Launch:
                Send("launch_cached_target", new JsonObject { ["path"] = row.Target }, "Couldn't start that");
                Announce($"Opening {row.Title}");
                break;
            case RowAction.CopyClip when long.TryParse(row.Target, out var id):
                // A secret goes back marked, so no clipboard history keeps it.
                Send("copy_clipboard_entry_to_clipboard", new JsonObject { ["id"] = id, ["quiet"] = row.Sensitive }, "Couldn't copy that");
                Announce("Copied");
                break;
            case RowAction.Go when Uri.TryCreate(row.Target, UriKind.Absolute, out var url):
                Visit(url);
                break;
            case RowAction.SwitchTab when row.Tab is { } tabId && Tabs.FirstOrDefault(t => t.Id == tabId) is { } tab:
                Select(tab);
                break;
            case RowAction.RunCommand when Commands.Registry.Get(row.Target) is { } command:
                Commands.Run(command, row.Argument);
                break;
        }
    }

    /// A file in a tab: the blank one you're on, or a new one beside it.
    /// Chromium shows PDFs, pictures and text itself.
    private void OpenFile(string path)
    {
        if (!File.Exists(path))
        {
            Announce("That file isn't there any more");
            return;
        }
        var url = new Uri(path);
        if (Active is { IsBlank: true } blank && Floating != blank.Id) Go(blank, url);
        else Open(url, foreground: true);
    }

    /// A program or a script: shown in its folder, selected, never run. What
    /// to do with it is Explorer's question, asked by the person.
    private void Reveal(string path)
    {
        if (!File.Exists(path))
        {
            Announce("That file isn't there any more");
            return;
        }
        Announce("Shown in its folder");
        _ = Task.Run(() =>
        {
            try
            {
                var explorer = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.Windows), "explorer.exe");
                using var _ = System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo(explorer)
                {
                    Arguments = $"/select,\"{path}\"",
                    UseShellExecute = false,
                });
            }
            catch (Exception error) when (error is System.ComponentModel.Win32Exception or InvalidOperationException)
            {
                Log.Write($"field: reveal: {error.Message}");
            }
        });
    }

    /// Audio and video, in the media player tab (Tools/#/play), not the bare
    /// file:// URL: it gets a playlist of the folder's other media, next/
    /// previous and a remembered position.
    private void OpenPlayer(string path)
    {
        if (!File.Exists(path))
        {
            Announce("That file isn't there any more");
            return;
        }
        if (ToolsHost.Resolve($"search://play?path={Uri.EscapeDataString(path)}") is not { } url) return;
        if (Active is { IsBlank: true } blank && Floating != blank.Id) Go(blank, url);
        else Open(url, foreground: true);
    }

    /// An engine call nothing waits on; if it fails, the line says `trouble`.
    private void Send(string method, JsonObject args, string trouble) =>
        _ = Task.Run(async () =>
        {
            try { await Engine.Client.CallAsync(method, args); }
            catch (EngineException error)
            {
                Log.Write($"field: {method}: {error.Message}");
                UI.Do(() => Announce(trouble));
            }
        });

    // MARK: - app icons

    /// Each app's icon, as the engine draws it to a PNG: null while asked
    /// for, or when it has none.
    private readonly Dictionary<string, string?> appIcons = new(StringComparer.OrdinalIgnoreCase);

    public string? IconOf(FieldRow row) => appIcons.GetValueOrDefault(row.Target);

    private void FetchIcons()
    {
        foreach (var row in board)
        {
            if (row.Action != RowAction.Launch || appIcons.ContainsKey(row.Target)) continue;
            appIcons[row.Target] = null;
            var path = row.Target;
            _ = Task.Run(async () =>
            {
                try
                {
                    var png = await Engine.Client.CallAsync("ensure_launcher_icon", new JsonObject { ["path"] = path });
                    if (png is JsonValue value && value.TryGetValue<string>(out var file) && File.Exists(file))
                        UI.Do(() =>
                        {
                            appIcons[path] = file;
                            Tell(nameof(Offers));
                        });
                }
                catch (EngineException) { }
            });
        }
    }
}
