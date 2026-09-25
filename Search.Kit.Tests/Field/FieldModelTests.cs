using System.Diagnostics;
using System.Text.Json.Nodes;
using SearchKit.Commands;
using SearchKit.Field;

namespace SearchKit.Tests.Field;

public class FieldModelTests
{
    private static readonly TypedSource typed = new(
        t => FieldQuery.LooksLikeAddress(t) ? new Uri(t.Contains("://") ? t : "https://" + t) : null,
        t => ("Google", new Uri("https://www.google.com/search?q=" + Uri.EscapeDataString(t))));

    private static Task Settled(FieldModel model) => model.WhenSettled.WaitAsync(TimeSpan.FromSeconds(10));

    [Fact]
    public async Task Each_keystroke_cancels_the_engine_questions_still_out()
    {
        var tokens = new List<CancellationToken>();
        var files = new FakeSource(Group.Files, async (q, cancel) =>
        {
            lock (tokens) tokens.Add(cancel);
            await Task.Delay(Timeout.Infinite, cancel);
            return [];
        });
        using var model = new FieldModel([typed], [files]);
        model.Type("in");
        await WaitFor(() => { lock (tokens) return tokens.Count == 1; });
        model.Type("inv");
        await WaitFor(() => { lock (tokens) return tokens.Count == 2; });
        lock (tokens)
        {
            Assert.True(tokens[0].IsCancellationRequested);
            Assert.False(tokens[1].IsCancellationRequested);
        }
        model.Type("invo");
        await WaitFor(() => { lock (tokens) return tokens[1].IsCancellationRequested; });
        // Everything cancelled still counts as settled.
        model.Type("");
        await Settled(model);
    }

    [Fact]
    public async Task Questions_made_stale_before_they_start_are_never_asked()
    {
        var files = new FakeSource(Group.Files, (q, _) => Task.FromResult<IReadOnlyList<FieldRow>>([Rows.File(q.Text + ".txt")]),
            TimeSpan.FromMilliseconds(40));
        using var model = new FieldModel([typed], [files]);
        for (var n = 2; n <= 12; n++) model.Type(new string('x', n));
        await Settled(model);
        Assert.Equal([new string('x', 12)], files.Texts);
    }

    [Fact]
    public async Task An_answer_to_an_older_question_never_lands()
    {
        var release = new TaskCompletionSource();
        var files = new FakeSource(Group.Files, async (q, _) =>
        {
            // Ignores its cancellation, like an engine that answers anyway.
            if (q.Text == "inv") await release.Task;
            return [Rows.File(q.Text + ".pdf")];
        });
        using var model = new FieldModel([typed], [files]);
        model.Type("inv");
        await WaitFor(() => files.Asked == 1);
        model.Type("invoice");
        await Settled(model);
        release.SetResult();
        await Task.Delay(100);
        Assert.Contains(model.Board.Rows, r => r.Title == "invoice.pdf");
        Assert.DoesNotContain(model.Board.Rows, r => r.Title == "inv.pdf");
    }

    [Fact]
    public async Task A_pause_is_waited_for_so_fast_typing_asks_once()
    {
        var files = new FakeSource(Group.Files, (q, _) => Task.FromResult<IReadOnlyList<FieldRow>>([Rows.File(q.Text + ".txt")]),
            TimeSpan.FromMilliseconds(150));
        using var model = new FieldModel([typed], [files]);
        foreach (var text in new[] { "re", "rep", "repo", "report" }) model.Type(text);
        await Settled(model);
        Assert.Equal(1, files.Asked);
        Assert.Equal(["report"], files.Texts);
        Assert.Contains(model.Board.Rows, r => r.Title == "report.txt");
    }

    [Fact]
    public async Task Late_rows_land_below_what_the_user_is_about_to_press()
    {
        var gate = new TaskCompletionSource();
        var files = new FakeSource(Group.Files, async (q, _) =>
        {
            await gate.Task;
            return [Rows.File("rust-notes.md"), Rows.File("rust.pdf")];
        });
        var history = new ListSource(Group.History, _ => [Rows.Page(Group.History, "a.rust.org"), Rows.Page(Group.History, "b.rust.org")]);
        var changes = 0;
        using var model = new FieldModel([typed, history], [files]);
        model.Changed += () => Interlocked.Increment(ref changes);
        model.Type("rust");
        // Top hit: the web search (no place starts with "rust"); then the places.
        Assert.Equal([RowKey.WebSearch("rust"), "url:a.rust.org", "url:b.rust.org"], model.Board.Keys());
        model.Board.Walk(1);
        model.Board.Walk(1);
        Assert.Equal("url:a.rust.org", model.Board.EnterRow!.Key);

        gate.SetResult();
        await Settled(model);
        Assert.Equal(2, changes);
        Assert.Equal([RowKey.WebSearch("rust"), "url:a.rust.org", "url:b.rust.org", RowKey.File(@"C:\t\rust-notes.md"),
            RowKey.File(@"C:\t\rust.pdf")], model.Board.Keys());
        Assert.Equal(1, model.Board.Picked);
        Assert.Equal("url:a.rust.org", model.Board.EnterRow!.Key);
    }

    [Fact]
    public async Task A_calculation_waits_for_the_engine_and_Enter_copies_it()
    {
        var engine = new FakeEngine
        {
            Answer = (method, args, _) => Task.FromResult(method == "evaluate_quick_query" && args!["query"]!.GetValue<string>() == "23*47"
                ? FakeEngine.Json("""{"type":"calculator","expression":"23*47","result":"1081"}""")
                : null),
        };
        using var model = new FieldModel([typed], [new AnswerSource(engine)]);
        model.Type("23*47");
        var entered = await model.EnterAsync(TimeSpan.FromSeconds(5));
        Assert.Equal((RowAction.Copy, "1081"), (entered!.Action, entered.Target));
        await Settled(model);
        Assert.Equal(Group.TopHit, model.Board.Rows[0].Group);
        Assert.Equal(RowKey.WebSearch("23*47"), model.Board.Rows[1].Key);
        var call = Assert.Single(engine.Calls);
        Assert.False(call.Args!["webSearchEnabled"]!.GetValue<bool>());
    }

    [Fact]
    public async Task No_answer_falls_back_to_the_search()
    {
        var engine = new FakeEngine();
        using var model = new FieldModel([typed], [new AnswerSource(engine)]);
        model.Type("5 km to lb");
        var entered = await model.EnterAsync(TimeSpan.FromSeconds(5));
        Assert.Equal(RowKey.WebSearch("5 km to lb"), entered!.Key);
        await Settled(model);
        Assert.Equal([RowKey.WebSearch("5 km to lb")], model.Board.Keys());
    }

    [Fact]
    public async Task A_broken_source_costs_its_rows_only()
    {
        var bad = new FakeSource(Group.Apps, (_, _) => throw new InvalidOperationException("engine went away"));
        var worse = new ListSource(Group.Tabs, _ => throw new InvalidOperationException("oops"));
        var files = new FakeSource(Group.Files, (_, _) => Task.FromResult<IReadOnlyList<FieldRow>>([Rows.File("x.txt")]));
        using var model = new FieldModel([typed, worse], [bad, files]);
        model.Type("x y");
        await Settled(model);
        Assert.Contains(model.Board.Rows, r => r.Title == "x.txt");
    }

    [Fact]
    public async Task A_scope_reserves_the_top_hit_for_its_first_row()
    {
        var engine = new FakeEngine
        {
            Answer = (_, _, _) => Task.FromResult(FakeEngine.Json("""
                {"results":[{"path":"C:\\Apps\\Code.lnk","name":"Visual Studio Code","kind":"shortcut"},
                            {"path":"C:\\Apps\\Codex.exe","name":"Codex","kind":"exe"}]}
                """)),
        };
        using var model = new FieldModel([typed], [new AppSource(engine), new FileNameSource(engine)]);
        model.Type("apps: code");
        Assert.True(model.Board.Rows[0].IsPending);
        await Settled(model);
        Assert.Equal(["Visual Studio Code", "Codex"], model.Board.Rows.Select(r => r.Title));
        Assert.Equal((Group.TopHit, RowAction.Launch), (model.Board.Rows[0].Group, model.Board.Rows[0].Action));
        // Only the app source was asked.
        Assert.Equal("search_launch_targets", Assert.Single(engine.Calls).Method);
    }

    [Fact]
    public async Task Rows_arrive_through_the_ui_post()
    {
        var posted = 0;
        var files = new FakeSource(Group.Files, (_, _) => Task.FromResult<IReadOnlyList<FieldRow>>([Rows.File("x.txt")]));
        using var model = new FieldModel([typed], [files], new FieldOptions
        {
            Post = action => { Interlocked.Increment(ref posted); action(); },
        });
        model.Type("x y");
        await Settled(model);
        Assert.Equal(1, posted);
    }

    [Fact]
    public void Typing_the_same_text_again_does_nothing()
    {
        var files = new FakeSource(Group.Files, (_, _) => Task.FromResult<IReadOnlyList<FieldRow>>([]));
        using var model = new FieldModel([typed], [files]);
        model.Type("abc");
        model.Type("abc");
        Assert.Equal(1, model.Board.Generation);
    }

    internal static async Task WaitFor(Func<bool> condition)
    {
        var until = DateTime.UtcNow.AddSeconds(10);
        while (!condition())
        {
            if (DateTime.UtcNow > until) throw new TimeoutException();
            await Task.Delay(5);
        }
    }
}

[Collection("Budgets")]
public class FieldModelBudgetTests(Xunit.Abstractions.ITestOutputHelper output)
{
    /// 10,000 places, 60 tabs, 400 bookmarks, the commands: every keystroke
    /// of a few queries is read, ranked and laid out in well under 5 ms.
    [Fact]
    public void A_keystroke_with_a_big_history_is_under_five_milliseconds()
    {
        var random = new Random(3);
        var words = new[] { "git", "hub", "rust", "docs", "mail", "news", "shop", "map", "cloud", "dev", "blog", "wiki", "api", "app" };
        var places = new List<Place>();
        for (var i = 0; i < 10_000; i++)
        {
            var host = $"{words[random.Next(words.Length)]}{words[random.Next(words.Length)]}{i}.com";
            var key = random.Next(3) == 0 ? host : $"{host}/{words[random.Next(words.Length)]}/{i}";
            places.Add(new Place(key, $"{words[random.Next(words.Length)]} page {i}", "https://" + key, random.Next(1, 40),
                DateTime.UtcNow.AddDays(-random.Next(0, 200))));
        }
        var tabs = Enumerable.Range(0, 60)
            .Select(i => new OpenPage(Guid.NewGuid(), $"{words[i % words.Length]} tab {i}", new Uri($"https://{words[i % words.Length]}{i}.org/x"),
                DateTime.UtcNow.AddMinutes(-i)))
            .ToList();
        var marks = Enumerable.Range(0, 400).Select(i => new Mark($"{words[i % words.Length]} mark {i}", new Uri($"https://m{i}.net/"))).ToList();
        var registry = new CommandRegistry();
        registry.Add(new Command("tabs.new", "New Tab", Tier.Act, ["new tab"]), _ => { });
        var engine = new FakeEngine();
        using var model = new FieldModel(
            [new TabSource(() => tabs), new HistorySource(() => places), new BookmarkSource(() => marks), new CommandSource(registry),
                new TypedSource(t => FieldQuery.LooksLikeAddress(t) ? new Uri("https://" + t) : null, t => ("Google", new Uri("https://g/?q=" + t)))],
            [new FileNameSource(engine), new FileContentSource(engine), new AppSource(engine), new AnswerSource(engine), new ClipboardSource(engine)],
            new FieldOptions { Bangs = new Bangs() });

        var keystrokes = new List<string>();
        foreach (var query in new[] { "github.com", "rust docs", "mailnews", "23*47", ">new", "clip: x", "a" })
            for (var n = 1; n <= query.Length; n++) keystrokes.Add(query[..n]);

        foreach (var text in keystrokes.Take(10)) model.Type(text);
        model.Type("");
        var times = new List<double>();
        var clock = new Stopwatch();
        foreach (var text in keystrokes)
        {
            clock.Restart();
            model.Type(text);
            times.Add(clock.Elapsed.TotalMilliseconds);
        }
        times.Sort();
        var median = times[times.Count / 2];
        var p95 = times[(int)(times.Count * 0.95)];
        output.WriteLine($"{times.Count} keystrokes: median {median:F3} ms, 95th {p95:F3} ms, worst {times[^1]:F3} ms");
        Assert.True(median < 2, $"median {median:F3} ms");
        Assert.True(p95 < 5, $"95th percentile {p95:F3} ms (worst {times[^1]:F3} ms)");
    }

    [Fact]
    public void Ranking_a_ten_thousand_place_history_is_under_five_milliseconds()
    {
        var places = Enumerable.Range(0, 10_000)
            .Select(i => new Place($"site{i}.example.com/page/{i}", $"Page {i}", $"https://site{i}.example.com/page/{i}", i % 17, DateTime.UtcNow.AddDays(-(i % 90))))
            .ToList();
        var source = new HistorySource(() => places);
        var queries = new[] { "s", "si", "site1", "example", "exa", "page", "zzz" }.Select(t => FieldQuery.Read(t)).ToList();
        foreach (var query in queries) source.Suggest(query, 5);
        var worst = 0.0;
        foreach (var query in queries)
        {
            var clock = Stopwatch.StartNew();
            var rows = source.Suggest(query, 5);
            worst = Math.Max(worst, clock.Elapsed.TotalMilliseconds);
        }
        output.WriteLine($"10k-place history: worst query {worst:F3} ms");
        Assert.True(worst < 5, $"worst {worst:F3} ms");
    }
}
