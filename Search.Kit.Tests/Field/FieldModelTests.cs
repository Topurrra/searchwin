using SearchKit.Field;

namespace SearchKit.Tests.Field;

public class FieldModelTests
{
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
        using var model = new FieldModel([files]);
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
        using var model = new FieldModel([files]);
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
        using var model = new FieldModel([files]);
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
        using var model = new FieldModel([files]);
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
        var files = new FakeSource(Group.Files, (_, _) => Task.FromResult<IReadOnlyList<FieldRow>>([Rows.File("rust-notes.md"), Rows.File("rust.pdf")]));
        // Answers are drawn above files, but this one comes after the pick.
        var answers = new FakeSource(Group.Answer, async (_, _) =>
        {
            await gate.Task;
            return [Rows.Answer("rust")];
        });
        var changes = 0;
        using var model = new FieldModel([files, answers]);
        model.Changed += () => Interlocked.Increment(ref changes);
        model.Type("rust");
        await WaitFor(() => model.Board.Rows.Count == 2);
        model.Board.Pick(1);
        Assert.Equal(RowKey.File(@"C:\t\rust.pdf"), model.Board.EnterRow!.Key);

        gate.SetResult();
        await Settled(model);
        Assert.Equal(3, changes);
        Assert.Equal([RowKey.File(@"C:\t\rust-notes.md"), RowKey.File(@"C:\t\rust.pdf"), RowKey.Answer("rust")], model.Board.Keys());
        Assert.Equal(1, model.Board.Picked);
        Assert.Equal(RowKey.File(@"C:\t\rust.pdf"), model.Board.EnterRow!.Key);
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
        using var model = new FieldModel([new AnswerSource(engine)]);
        model.Type("23*47");
        var entered = await model.EnterAsync(TimeSpan.FromSeconds(5));
        Assert.Equal((RowAction.Copy, "1081"), (entered!.Action, entered.Target));
        await Settled(model);
        Assert.Equal(Group.TopHit, model.Board.Rows[0].Group);
        var call = Assert.Single(engine.Calls);
        Assert.False(call.Args!["webSearchEnabled"]!.GetValue<bool>());
    }

    [Fact]
    public async Task A_broken_source_costs_its_rows_only()
    {
        var bad = new FakeSource(Group.Apps, (_, _) => throw new InvalidOperationException("engine went away"));
        var files = new FakeSource(Group.Files, (_, _) => Task.FromResult<IReadOnlyList<FieldRow>>([Rows.File("x.txt")]));
        using var model = new FieldModel([bad, files]);
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
        using var model = new FieldModel([new AppSource(engine), new FileNameSource(engine)]);
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
        using var model = new FieldModel([files], new FieldOptions
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
        using var model = new FieldModel([files]);
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
