using System.Diagnostics;
using SearchKit.Field;

namespace SearchKit.Tests.Field;

/// What the browser may do with a clipboard entry: the review's findings
/// on secrets, paste targets, test worlds, the pause and the list.
public class ClipGuardTests
{
    private static ClipEntry Text(long id, string text, bool secret = false, long at = 0) =>
        new(id, false, text, secret ? ["sensitive", "github_token"] : [], "", "code", false, at, 0, 0, "");

    private static ClipEntry Picture(long id) => new(id, true, "", [], "", "snip", false, 0, 800, 600, "");

    [Fact]
    public void Shift_Enter_on_a_secret_is_never_a_search()
    {
        var token = Text(1, "ghp_notARealTokenButShapedLikeOne1234567890", secret: true);
        Assert.False(ClipGuard.MayGo(token));
        var phrase = Text(2, "hunter2 is my password", secret: true);
        Assert.False(ClipGuard.MayGo(phrase));
    }

    // The address is the secret: opening it hands it to the site and to
    // History. Refused like any other secret.
    [Fact]
    public void Shift_Enter_on_a_secret_that_is_an_address_is_refused_too()
    {
        // Built at run time: a literal webhook here trips GitHub push protection.
        var webhook = $"https://hooks.slack.com/services/T{new string('0', 8)}/B{new string('0', 8)}/{new string('X', 24)}";
        Assert.False(ClipGuard.MayGo(Text(1, webhook, secret: true)));
        Assert.False(ClipGuard.MayGo(Text(2, "  " + webhook + "  ", secret: true)));
        Assert.False(ClipGuard.MayGo(Text(3, "https://example.com/reset?token=notARealResetToken", secret: true)));
    }

    [Fact]
    public void Shift_Enter_on_plain_text_searches_or_goes_as_before()
    {
        Assert.True(ClipGuard.MayGo(Text(1, "rust ownership")));
        Assert.True(ClipGuard.MayGo(Text(2, "example.com")));
        Assert.True(ClipGuard.MayGo(Text(3, "https://example.com/page")));
        Assert.False(ClipGuard.MayGo(Text(4, "   ")));
        Assert.False(ClipGuard.MayGo(Picture(5)));
    }

    [Fact]
    public void Only_a_secret_goes_back_quietly()
    {
        Assert.True(ClipGuard.Quiet(Text(1, "x", secret: true)));
        Assert.False(ClipGuard.Quiet(Text(2, "x")));
        Assert.False(ClipGuard.Quiet(Picture(3)));
    }

    [Fact]
    public void A_paste_goes_into_the_page_it_was_aimed_at()
    {
        var opened = new ClipGuard.Page(ClipGuard.Origin(new Uri("https://Mail.Example.com/inbox")), 3);
        var same = new ClipGuard.Page(ClipGuard.Origin(new Uri("https://mail.example.com/compose#x")), 3);
        Assert.Equal(ClipGuard.Paste.Insert, ClipGuard.IntoPage(opened, same, sensitive: false, caretInMainFrame: false));
        Assert.Equal(ClipGuard.Paste.Insert, ClipGuard.IntoPage(opened, same, sensitive: true, caretInMainFrame: true));
    }

    [Fact]
    public void A_page_that_moved_on_gets_a_copy_not_the_paste()
    {
        var opened = new ClipGuard.Page("https://mail.example.com", 3);
        Assert.Equal(ClipGuard.Paste.Moved, ClipGuard.IntoPage(opened, new("https://evil.example", 3), false, true));
        // Same site, another document: a navigation happened since.
        Assert.Equal(ClipGuard.Paste.Moved, ClipGuard.IntoPage(opened, new("https://mail.example.com", 4), false, true));
        // A page with no address is never a target.
        Assert.Equal(ClipGuard.Paste.Moved, ClipGuard.IntoPage(new("", 0), new("", 0), false, true));
        Assert.Equal("", ClipGuard.Origin(null));
    }

    [Fact]
    public void A_secret_needs_the_caret_in_the_page_itself()
    {
        var page = new ClipGuard.Page("https://bank.example", 1);
        Assert.Equal(ClipGuard.Paste.Framed, ClipGuard.IntoPage(page, page, sensitive: true, caretInMainFrame: false));
        // Plain text may go into a frame, as a person's typing would.
        Assert.Equal(ClipGuard.Paste.Insert, ClipGuard.IntoPage(page, page, sensitive: false, caretInMainFrame: false));
    }

    [Fact]
    public void Test_worlds_keep_nothing_unless_asked_and_the_real_profile_keeps_by_default()
    {
        Assert.False(ClipGuard.OnByDefault(testing: true));
        Assert.True(ClipGuard.OnByDefault(testing: false));
    }

    [Fact]
    public void Reports_leave_out_what_was_copied_before_the_run()
    {
        var entries = new[] { Text(1, "before", at: 999), Text(2, "at start", at: 1000), Text(3, "after", at: 1500) };
        Assert.Equal([2, 3], ClipList.Since(entries, 1000).Select(e => e.Id));
    }

    [Fact]
    public void The_list_holds_still_under_the_pointer_and_just_after_a_key()
    {
        Assert.False(ClipGuard.MayRedraw(pointerOver: true, msSinceKey: 10_000));
        Assert.False(ClipGuard.MayRedraw(pointerOver: false, msSinceKey: 120));
        Assert.True(ClipGuard.MayRedraw(pointerOver: false, msSinceKey: ClipGuard.StillMs));
    }

    [Fact]
    public void A_pause_is_never_lifted_on_reconnect()
    {
        // The user paused; the engine restarted afresh: pause it again.
        Assert.Equal((true, (bool?)true), ClipGuard.Rejoin(remembered: true, engine: false));
        // The engine is paused (a pause Search didn't hear of): keep it.
        Assert.Equal((true, (bool?)null), ClipGuard.Rejoin(remembered: false, engine: true));
        Assert.Equal((true, (bool?)null), ClipGuard.Rejoin(remembered: true, engine: true));
        Assert.Equal((false, (bool?)null), ClipGuard.Rejoin(remembered: false, engine: false));
    }

    [Fact]
    public void Tool_pages_can_neither_start_the_listener_nor_touch_the_pause()
    {
        Assert.NotNull(SearchKit.Web.ToolCalls.Refused("start_clipboard_listener"));
        Assert.NotNull(SearchKit.Web.ToolCalls.Refused("set_clipboard_paused"));
        // Reading is fine, and so is everything else.
        Assert.Null(SearchKit.Web.ToolCalls.Refused("get_clipboard_paused"));
        Assert.Null(SearchKit.Web.ToolCalls.Refused("get_clipboard_history"));
        Assert.Null(SearchKit.Web.ToolCalls.Refused("host:clipboard.secret"));
    }

    [Fact]
    public async Task A_secret_clip_row_says_so_so_its_copy_goes_back_quietly()
    {
        // The field's CopyClip sends `quiet: row.Sensitive`.
        var history = FakeEngine.Json("""
            [{"id":8,"capturedAtMs":800,"kind":"text","text":"ghp_x","sensitiveKinds":["github_token"]},
             {"id":9,"capturedAtMs":900,"kind":"text","text":"plain words","sensitiveKinds":[]}]
            """);
        var engine = new FakeEngine { Answer = (_, _, _) => Task.FromResult(history) };
        var rows = await new ClipboardSource(engine).SuggestAsync(FieldQuery.Read("clip:"), 10, default);
        Assert.True(rows.Single(r => r.Target == "8").Sensitive);
        Assert.False(rows.Single(r => r.Target == "9").Sensitive);
    }

    [Fact]
    public void Typing_searches_only_the_start_of_a_long_entry()
    {
        var head = "invoice " + new string('x', ClipEntry.Searched);
        var entry = Text(1, head + " buried-needle");
        Assert.True(entry.Matches("invoice"));
        Assert.False(entry.Matches("buried-needle"));
    }

    [Fact]
    public void Filtering_a_full_history_of_big_entries_stays_within_a_keystroke()
    {
        // 200 entries of 256 KB each: the engine's limits.
        var big = new string('a', 256 * 1024);
        var entries = Enumerable.Range(0, 200).Select(i => Text(i, big, at: i)).ToList();
        ClipList.Filter(entries, "warm");
        var clock = Stopwatch.StartNew();
        for (var i = 0; i < 5; i++) ClipList.Filter(entries, "zzz" + i);
        Assert.True(clock.Elapsed.TotalMilliseconds / 5 < 5, $"{clock.Elapsed.TotalMilliseconds / 5:F2} ms per filter");
    }
}
