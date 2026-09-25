using SearchKit.Field;

namespace SearchKit.Tests.Field;

public class EngineSourceTests
{
    private const string FileResults = """
        {"query":"inv","totalHits":4,"returned":4,"tookMs":3,"results":[
          {"path":"C:\\Users\\t\\Documents\\invoice-2024.pdf","fileName":"invoice-2024.pdf","entryType":"file","extension":"pdf","size":1,"modifiedMs":0,"score":9.5,"matchedKeywords":[],"matchReason":"name","sensitiveKinds":[]},
          {"path":"C:\\Users\\t\\Music\\invasion.mp3","fileName":"invasion.mp3","entryType":"file","extension":"mp3","size":1,"modifiedMs":0,"score":5,"matchedKeywords":[],"matchReason":"name","sensitiveKinds":[]},
          {"path":"C:\\Users\\t\\Invoices","fileName":"Invoices","entryType":"folder","extension":"","size":0,"modifiedMs":0,"score":4,"matchedKeywords":[],"matchReason":"name","sensitiveKinds":[]},
          {"path":"C:\\Users\\t\\inv.xlsx","fileName":"inv.xlsx","entryType":"file","extension":"xlsx","size":1,"modifiedMs":0,"score":3,"matchedKeywords":[],"matchReason":"name","sensitiveKinds":[]}
        ]}
        """;

    [Fact]
    public async Task Files_open_in_a_tab_the_player_or_their_own_app()
    {
        var engine = new FakeEngine { Answer = (_, _, _) => Task.FromResult(FakeEngine.Json(FileResults)) };
        var rows = await new FileNameSource(engine).SuggestAsync(FieldQuery.Read("inv"), 8, default);
        Assert.Equal([RowAction.OpenInTab, RowAction.Play, RowAction.OpenWithApp, RowAction.OpenWithApp], rows.Select(r => r.Action));
        Assert.Equal(("invoice-2024.pdf", @"C:\Users\t\Documents"), (rows[0].Title, rows[0].Detail));
        Assert.Equal(RowKey.File(@"c:/users/t/documents/invoice-2024.pdf"), rows[0].Key);

        var (method, args) = Assert.Single(engine.Calls);
        Assert.Equal("search_local_files", method);
        Assert.Equal("""{"options":{"query":"inv","limit":8}}""", args!.ToJsonString());
    }

    [Fact]
    public void Which_queries_each_engine_source_wants()
    {
        var engine = new FakeEngine();
        var names = new FileNameSource(engine);
        var contents = new FileContentSource(engine);
        var apps = new AppSource(engine);
        var clip = new ClipboardSource(engine);
        var answers = new AnswerSource(engine);

        bool[] Wants(string typed)
        {
            var q = FieldQuery.Read(typed);
            return [names.Wants(q), contents.Wants(q), apps.Wants(q), clip.Wants(q), answers.Wants(q)];
        }
        Assert.Equal([true, true, true, true, false], Wants("invoice"));
        Assert.Equal([true, false, true, false, false], Wants("in"));
        Assert.Equal([false, false, false, false, false], Wants("i"));
        Assert.Equal([true, false, false, false, false], Wants("invoice.pdf"));
        Assert.Equal([false, false, false, false, false], Wants("https://example.com/"));
        Assert.Equal([false, false, false, false, true], Wants("23*47"));
        Assert.Equal([true, true, false, false, false], Wants("files: invoice"));
        Assert.Equal([false, false, true, false, false], Wants("apps:"));
        Assert.Equal([false, false, false, true, false], Wants("clip:"));
        Assert.Equal([false, false, false, false, false], Wants(">close"));
        Assert.False(new ClipboardSource(engine, inField: false).Wants(FieldQuery.Read("invoice")));
    }

    [Fact]
    public async Task Content_matches_show_their_snippet_unless_the_file_holds_a_secret()
    {
        var engine = new FakeEngine
        {
            Answer = (_, _, _) => Task.FromResult(FakeEngine.Json("""
                {"results":[
                  {"path":"C:\\t\\a.md","fileName":"a.md","extension":"md","snippet":"the <b>invoice</b>   total &amp; tax","sensitiveKinds":[]},
                  {"path":"C:\\t\\keys.txt","fileName":"keys.txt","extension":"txt","snippet":"<b>invoice</b> key AKIA…","sensitiveKinds":["sensitive","aws_access_key"]}
                ]}
                """)),
        };
        var rows = await new FileContentSource(engine).SuggestAsync(FieldQuery.Read("invoice"), 5, default);
        Assert.Equal("the invoice total & tax", rows[0].Detail);
        Assert.True(rows[1].Sensitive);
        Assert.DoesNotContain("AKIA", rows[1].Detail);
        Assert.Equal("search_file_contents", engine.Calls[0].Method);
    }

    [Fact]
    public async Task Apps_launch_and_an_empty_scope_lists_them_all()
    {
        var engine = new FakeEngine
        {
            Answer = (_, _, _) => Task.FromResult(FakeEngine.Json("""{"results":[{"id":"launch://x","name":"Notepad","path":"C:\\W\\notepad.exe","kind":"exe","source":"path","score":1}]}""")),
        };
        var rows = await new AppSource(engine).SuggestAsync(FieldQuery.Read("apps:"), 12, default);
        var row = Assert.Single(rows);
        Assert.Equal(("Notepad", RowAction.Launch, @"C:\W\notepad.exe"), (row.Title, row.Action, row.Target));
        Assert.Equal("""{"options":{"query":"","limit":12,"browseAll":true}}""", engine.Calls[0].Args!.ToJsonString());
    }

    private const string Clipboard = """
        [
          {"id":9,"capturedAtMs":9,"kind":"text","text":"invoice #42 for March","sourceApp":"outlook.exe","sensitiveKinds":[],"category":"text","isPinned":false},
          {"id":8,"capturedAtMs":8,"kind":"text","text":"hunter2 is not a real secret","sourceApp":"code.exe","sensitiveKinds":["sensitive","github_token"],"category":"text","isPinned":false},
          {"id":7,"capturedAtMs":7,"kind":"image","text":"","sourceApp":"snip.exe","sensitiveKinds":[],"category":"image","isPinned":false,"imageWidth":800,"imageHeight":600},
          {"id":6,"capturedAtMs":6,"kind":"text","text":"old invoice\n  notes","sourceApp":null,"sensitiveKinds":[],"category":"text","isPinned":true,"pinLabel":"Invoice template"}
        ]
        """;

    [Fact]
    public async Task Clipboard_scope_shows_pinned_first_and_hides_secrets()
    {
        var engine = new FakeEngine { Answer = (_, _, _) => Task.FromResult(FakeEngine.Json(Clipboard)) };
        var source = new ClipboardSource(engine);
        var rows = await source.SuggestAsync(FieldQuery.Read("clip:"), 12, default);
        Assert.Equal(["clip:6", "clip:9", "clip:8", "clip:7"], rows.Select(r => r.Key));
        Assert.Equal(("old invoice notes", "Invoice template"), (rows[0].Title, rows[0].Detail));
        Assert.Equal("Hidden: github token", rows[2].Title);
        Assert.True(rows[2].Sensitive);
        Assert.Equal("Image 800×600", rows[3].Title);
        Assert.All(rows, r => Assert.Equal(RowAction.CopyClip, r.Action));
        Assert.All(rows, r => Assert.DoesNotContain("hunter2", r.Title + r.Detail + r.Target));

        // The secret isn't matched on its own text.
        Assert.Empty(await source.SuggestAsync(FieldQuery.Read("clip: hunter2"), 12, default));
        Assert.Single(await source.SuggestAsync(FieldQuery.Read("clip: github"), 12, default));
    }

    [Fact]
    public async Task Plain_words_show_clipboard_text_only()
    {
        var engine = new FakeEngine { Answer = (_, _, _) => Task.FromResult(FakeEngine.Json(Clipboard)) };
        var rows = await new ClipboardSource(engine).SuggestAsync(FieldQuery.Read("invoice"), 2, default);
        Assert.Equal(["clip:9", "clip:6"], rows.Select(r => r.Key));
    }

    [Fact]
    public async Task The_clipboard_is_fetched_again_only_after_it_changed()
    {
        var engine = new FakeEngine { Answer = (_, _, _) => Task.FromResult(FakeEngine.Json(Clipboard)) };
        var source = new ClipboardSource(engine);
        await source.SuggestAsync(FieldQuery.Read("clip: a"), 5, default);
        await source.SuggestAsync(FieldQuery.Read("clip: ab"), 5, default);
        Assert.Single(engine.Calls);
        engine.Raise("some-other-event");
        await source.SuggestAsync(FieldQuery.Read("clip: abc"), 5, default);
        Assert.Single(engine.Calls);
        engine.Raise(ClipboardSource.ChangedEvent);
        await source.SuggestAsync(FieldQuery.Read("clip: abcd"), 5, default);
        Assert.Equal(2, engine.Calls.Count);
        Assert.Equal("get_clipboard_history", engine.Calls[1].Method);

        // An engine that restarted can't say so: the copy only lasts so long.
        var fresh = new FakeEngine { Answer = (_, _, _) => Task.FromResult(FakeEngine.Json(Clipboard)) };
        var brief = new ClipboardSource(fresh, maxAge: TimeSpan.Zero);
        await brief.SuggestAsync(FieldQuery.Read("clip: a"), 5, default);
        await brief.SuggestAsync(FieldQuery.Read("clip: ab"), 5, default);
        Assert.Equal(2, fresh.Calls.Count);
    }
}

public class LocalSourceTests
{
    [Fact]
    public void History_ranks_like_the_browser()
    {
        var now = new DateTime(2026, 9, 25, 0, 0, 0, DateTimeKind.Utc);
        var places = new List<Place>
        {
            new("github.com/topurrra/search", "Search", "https://github.com/topurrra/search", 30, now),
            new("github.com", "GitHub", "https://github.com/", 5, now.AddDays(-2)),
            new("gist.github.com", "Gists", "https://gist.github.com/", 1, now.AddDays(-60)),
            new("example.com/github", "Path only", "https://example.com/github", 50, now),
        };
        var source = new HistorySource(() => places, [("gitlab.com", "GitLab"), ("github.com", "GitHub")], () => now);
        var rows = source.Suggest(FieldQuery.Read("git"), 5);
        // The frequent deep page first, then the front door; a known name only where memory has nothing; never a path match.
        Assert.Equal(["url:github.com/topurrra/search", "url:github.com", "url:gist.github.com", "url:gitlab.com"], rows.Select(r => r.Key));
        Assert.Contains(source.Suggest(FieldQuery.Read("hub"), 5), r => r.Key == "url:github.com");
        Assert.DoesNotContain(source.Suggest(FieldQuery.Read("g"), 5), r => r.Key == "url:example.com/github");
        Assert.Empty(source.Suggest(FieldQuery.Read("23*47"), 5));
    }

    [Fact]
    public void Tabs_match_titles_and_addresses_but_never_the_active_tab()
    {
        var active = new OpenPage(Guid.NewGuid(), "Rust Book", new Uri("https://doc.rust-lang.org/book/"), DateTime.UtcNow, Active: true);
        var other = new OpenPage(Guid.NewGuid(), "The Rust Reference", new Uri("https://doc.rust-lang.org/reference/"), DateTime.UtcNow);
        var source = new TabSource(() => [active, other]);
        var row = Assert.Single(source.Suggest(FieldQuery.Read("rust"), 3));
        Assert.Equal((RowAction.SwitchTab, other.Id), (row.Action, row.Tab));
        // Keyed as the browser keys history: Address.Pretty, lower-cased.
        Assert.Equal("url:doc.rust-lang.org/reference/", row.Key);
        Assert.Single(source.Suggest(FieldQuery.Read("tabs:"), 3));
    }

    [Fact]
    public void Keys_match_across_sources()
    {
        Assert.Equal(RowKey.Place("github.com"), RowKey.Url(new Uri("https://www.github.com/")));
        Assert.Equal(RowKey.Place("example.com/a b"), RowKey.Url(new Uri("http://example.com/a%20b?x=1#y")));
        Assert.Equal(RowKey.File(@"C:\Docs\A.pdf"), RowKey.File("c:/docs/a.pdf"));
    }
}
