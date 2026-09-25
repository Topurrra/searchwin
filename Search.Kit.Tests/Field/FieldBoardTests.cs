using SearchKit.Commands;
using SearchKit.Field;

namespace SearchKit.Tests.Field;

public class FieldLayoutTests
{
    private static readonly FieldCaps caps = new();

    private static FieldRow Tab(string key, string title, double score) =>
        new(Group.Tabs, "url:" + key, title, key, RowAction.SwitchTab, "https://" + key + "/") { Score = score, Tab = Guid.NewGuid() };

    [Fact]
    public void The_same_page_shows_once_and_an_open_tab_wins()
    {
        var query = FieldQuery.Read("docs");
        var laid = FieldLayout.Build(query,
            [Rows.Page(Group.History, "rust-lang.org/docs"), Tab("rust-lang.org/docs", "Rust docs", 4), Rows.Search("docs")],
            null, caps);
        Assert.Single(laid, r => r.Key == "url:rust-lang.org/docs");
        Assert.Contains(laid, r => r.Key == "url:rust-lang.org/docs" && r.Origin == Group.Tabs);
    }

    [Fact]
    public void Groups_are_drawn_in_order_and_capped()
    {
        var query = FieldQuery.Read("zzz");
        var history = Enumerable.Range(0, 6).Select(i => Rows.Page(Group.History, $"h{i}.zzz.com")).ToList();
        var laid = FieldLayout.Build(query, [Rows.Search("zzz"), .. history, Tab("t.zzz.com", "t", 2)], null, caps);
        // The search row is the top hit (nothing starts with "zzz"), then tabs, then three places.
        Assert.Equal([Group.TopHit, Group.Tabs, Group.History, Group.History, Group.History], laid.Select(r => r.Group));
        Assert.Equal(RowKey.WebSearch("zzz"), laid[0].Key);
        Assert.Equal(["url:h0.zzz.com", "url:h1.zzz.com", "url:h2.zzz.com"], laid.Where(r => r.Group == Group.History).Select(r => r.Key));
    }

    [Fact]
    public void The_place_being_finished_is_the_top_hit_and_gives_the_ending()
    {
        var query = FieldQuery.Read("git");
        var laid = FieldLayout.Build(query,
            [Rows.Page(Group.History, "gitlab.example.org/x"), Rows.Page(Group.History, "github.com"), Rows.Search("git")], null, caps);
        // The first place (in the source's order) whose key starts with the text.
        Assert.Equal("url:gitlab.example.org/x", laid[0].Key);
        Assert.Equal(Group.TopHit, laid[0].Group);
        Assert.Equal(Group.History, laid[0].Origin);
        Assert.Equal("lab.example.org/x", FieldLayout.Ending(query, laid[0]));
        Assert.Null(FieldLayout.Ending(FieldQuery.Read("g"), laid[0]));
    }

    [Fact]
    public void A_tab_that_starts_with_the_text_beats_history()
    {
        var query = FieldQuery.Read("mail");
        var laid = FieldLayout.Build(query,
            [Rows.Page(Group.History, "mail.google.com"), Tab("outlook.com", "Mail - Outlook", 5), Rows.Search("mail")], null, caps);
        Assert.Equal("url:outlook.com", laid[0].Key);
        Assert.Equal(RowAction.SwitchTab, laid[0].Action);
    }

    [Fact]
    public void Commands_bangs_and_questions_take_their_first_row()
    {
        var registry = new CommandRegistry();
        registry.Add(new Command("tabs.close-others", "Close Other Tabs", Tier.Act, ["close others"]), _ => { });
        var command = FieldQuery.Read(">close");
        var rows = new CommandSource(registry).Suggest(command, 6);
        Assert.Equal("tabs.close-others", FieldLayout.Build(command, rows, null, caps)[0].Target);

        var typed = new TypedSource(_ => null, t => ("Google", new Uri("https://www.google.com/search?q=" + Uri.EscapeDataString(t))));
        var bang = FieldQuery.Read("!yt cats", new Bangs());
        var top = FieldLayout.Build(bang, typed.Suggest(bang, 1), null, caps)[0];
        Assert.StartsWith("https://www.youtube.com/results?search_query=cats", top.Target);

        var ask = FieldQuery.Read("? what now");
        Assert.Equal(RowAction.Ask, FieldLayout.Build(ask, typed.Suggest(ask, 1), null, caps)[0].Action);
    }

    [Fact]
    public void Answers_and_engine_scopes_reserve_the_top_hit()
    {
        Assert.Equal(Group.Answer, FieldLayout.Reserve(FieldQuery.Read("23*47"), [Group.Answer, Group.Files]));
        Assert.Equal(Group.Files, FieldLayout.Reserve(FieldQuery.Read("files: invoice"), [Group.Files]));
        Assert.Equal(Group.Clipboard, FieldLayout.Reserve(FieldQuery.Read("clip:"), [Group.Clipboard]));
        // Nothing to wait for when no source was asked.
        Assert.Null(FieldLayout.Reserve(FieldQuery.Read("23*47"), [Group.Files]));
        Assert.Null(FieldLayout.Reserve(FieldQuery.Read("invoice"), [Group.Files, Group.Apps]));

        var laid = FieldLayout.Build(FieldQuery.Read("23*47"), [Rows.Search("23*47")], Group.Answer, caps);
        Assert.True(laid[0].IsPending);
        Assert.Equal(Group.Answer, laid[0].Origin);
        Assert.Equal(RowKey.WebSearch("23*47"), laid[1].Key);
    }
}

public class FieldBoardTests
{
    private static FieldBoard Board(params FieldRow[] rows)
    {
        var board = new FieldBoard();
        board.Reset(1, [.. rows], null);
        return board;
    }

    private static FieldRow Top(FieldRow row) => row with { Group = Group.TopHit };

    [Fact]
    public void Late_rows_go_below_the_rows_of_groups_drawn_above_theirs()
    {
        var board = Board(Top(Rows.Search("inv")), Rows.Page(Group.History, "a.com"), Rows.Page(Group.History, "b.com"));
        Assert.True(board.Arrive(1, Group.Files, [Rows.File("inv.pdf"), Rows.File("inv2.pdf")], 4, 16));
        Assert.Equal([RowKey.WebSearch("inv"), "url:a.com", "url:b.com", RowKey.File(@"C:\t\inv.pdf"), RowKey.File(@"C:\t\inv2.pdf")],
            board.Keys());
    }

    [Fact]
    public void Late_rows_never_move_the_picked_row_or_anything_above_it()
    {
        var board = Board(Top(Rows.Search("inv")), Rows.Page(Group.History, "a.com"), Rows.Page(Group.History, "b.com"),
            Rows.Page(Group.Bookmarks, "c.com"));
        board.Walk(1);
        board.Walk(1);
        board.Walk(1);
        Assert.Equal(2, board.Picked);
        var before = board.Keys();

        // Tabs belong right under the top hit, above history; the pick holds them below it.
        Assert.True(board.Arrive(1, Group.Tabs, [Rows.Page(Group.Tabs, "t1.com"), Rows.Page(Group.Tabs, "t2.com")], 3, 16));
        Assert.Equal(2, board.Picked);
        Assert.Equal("url:b.com", board.EnterRow!.Key);
        var after = board.Keys();
        Assert.Equal(before.Take(3), after.Take(3));
        Assert.Equal(["url:t1.com", "url:t2.com"], after.Skip(3).Take(2));
        // Everything that was there is still there, in the same order.
        Assert.Equal(before, after.Where(before.Contains));
    }

    [Fact]
    public void The_row_under_the_pointer_holds_too()
    {
        var board = Board(Top(Rows.Search("x")), Rows.Page(Group.History, "a.com"), Rows.Page(Group.History, "b.com"));
        board.Hold(1);
        board.Arrive(1, Group.Tabs, [Rows.Page(Group.Tabs, "t.com")], 3, 16);
        Assert.Equal(["url:a.com", "url:t.com", "url:b.com"], board.Keys().Skip(1));
        // Let go of, the next tab joins the other one.
        board.Hold(null);
        board.Arrive(1, Group.Tabs, [Rows.Page(Group.Tabs, "u.com")], 3, 16);
        Assert.Equal(["url:a.com", "url:t.com", "url:u.com", "url:b.com"], board.Keys().Skip(1));
    }

    [Fact]
    public void Without_a_top_hit_or_a_pick_late_rows_take_their_natural_place()
    {
        var board = Board(Rows.Page(Group.History, "a.com"), Rows.Search("x"));
        board.Arrive(1, Group.Tabs, [Rows.Page(Group.Tabs, "t.com")], 3, 16);
        board.Arrive(1, Group.Files, [Rows.File("x.txt")], 4, 16);
        Assert.Equal(["url:t.com", "url:a.com", RowKey.File(@"C:\t\x.txt"), RowKey.WebSearch("x")], board.Keys());
    }

    [Fact]
    public void A_reserved_top_hit_is_filled_where_it_stands()
    {
        var board = Board(FieldRow.Placeholder(Group.Answer), Rows.Search("#e07a5f"));
        Assert.True(board.Waiting);
        Assert.Null(board.EnterRow);
        board.Arrive(1, Group.Answer, [Rows.Answer("rgb(224, 122, 95)"), Rows.Answer("hsl(12, 67%, 63%)")], 3, 16);
        var rows = board.Rows;
        Assert.Equal((Group.TopHit, Group.Answer, "rgb(224, 122, 95)"), (rows[0].Group, rows[0].Origin, rows[0].Title));
        Assert.Equal((Group.Answer, "hsl(12, 67%, 63%)"), (rows[1].Group, rows[1].Title));
        Assert.Equal(RowKey.WebSearch("#e07a5f"), rows[2].Key);
        Assert.False(board.Waiting);
        Assert.Equal("rgb(224, 122, 95)", board.EnterRow!.Target);
        Assert.False(board.Settle(1, Group.Answer));
    }

    [Fact]
    public void A_reserved_top_hit_that_never_comes_goes_away()
    {
        var board = Board(FieldRow.Placeholder(Group.Answer), Rows.Page(Group.History, "a.com"), Rows.Search("5 km to zz"));
        Assert.True(board.Settle(1, Group.Answer));
        Assert.Equal(["url:a.com", RowKey.WebSearch("5 km to zz")], board.Keys());
        Assert.False(board.Waiting);
        Assert.Null(board.EnterRow);
        Assert.Equal(RowKey.WebSearch("5 km to zz"), board.Fallback!.Key);
    }

    [Fact]
    public void Under_a_pick_it_stays_as_a_spacer_and_nothing_moves()
    {
        var board = Board(FieldRow.Placeholder(Group.Answer), Rows.Page(Group.History, "a.com"), Rows.Search("5 km to zz"));
        board.Walk(1);
        Assert.Equal("url:a.com", board.EnterRow!.Key);
        Assert.False(board.Settle(1, Group.Answer));
        Assert.Equal([FieldRow.Placeholder(Group.Answer).Key, "url:a.com", RowKey.WebSearch("5 km to zz")], board.Keys());
        Assert.Equal(1, board.Picked);
        Assert.Equal("url:a.com", board.EnterRow!.Key);
        Assert.Equal(RowKey.WebSearch("5 km to zz"), board.Fallback!.Key);
        // The next question starts clean.
        board.Reset(2, [FieldRow.Placeholder(Group.Answer), Rows.Search("5 km to zzz")], null);
        Assert.True(board.Settle(2, Group.Answer));
    }

    [Theory]
    [InlineData("hold board row")]
    [InlineData("hold beside")]
    [InlineData("pick beside")]
    [InlineData("pointer over")]
    public void Under_the_pointer_or_a_pick_of_the_browsers_own_rows_it_stays_too(string how)
    {
        // The browser's own rows are drawn below the board's top hit (see
        // FieldMix): they move up if it goes away.
        var board = Board(FieldRow.Placeholder(Group.Answer), Rows.File("a.txt"));
        switch (how)
        {
            case "hold board row": board.Hold(1); break;
            case "hold beside": board.Hold(FieldBoard.Beside); break;
            case "pick beside": board.Pick(FieldBoard.Beside); break;
            case "pointer over": board.PointerOver = true; break;
        }
        var before = FieldMix.Compose(3, board.Rows);
        Assert.False(board.Settle(1, Group.Answer));
        Assert.Equal(before, FieldMix.Compose(3, board.Rows));
        // Waiting is over, though: Enter doesn't wait for what won't come.
        Assert.False(board.Waiting);
        Assert.Null(board.EnterRow);
    }

    [Fact]
    public void The_pointer_over_the_list_outlasts_a_new_question_a_hold_does_not()
    {
        var board = Board(FieldRow.Placeholder(Group.Answer), Rows.File("a.txt"));
        board.Hold(FieldBoard.Beside);
        board.Pick(FieldBoard.Beside);
        board.PointerOver = true;
        board.Reset(2, [FieldRow.Placeholder(Group.Answer), Rows.File("a.txt")], null);
        Assert.True(board.PointerOver);
        Assert.False(board.Settle(2, Group.Answer));

        board.PointerOver = false;
        board.Reset(3, [FieldRow.Placeholder(Group.Answer), Rows.File("a.txt")], null);
        Assert.True(board.Settle(3, Group.Answer));

        // Letting go of the browser's row lets the slot go.
        board.Reset(4, [FieldRow.Placeholder(Group.Answer), Rows.File("a.txt")], null);
        board.Hold(FieldBoard.Beside);
        board.Hold(null);
        Assert.True(board.Settle(4, Group.Answer));
    }

    [Fact]
    public void Walking_skips_reserved_slots_and_lets_go_off_the_ends()
    {
        var board = Board(FieldRow.Placeholder(Group.Files), Rows.Page(Group.History, "a.com"), Rows.Search("x"));
        board.Walk(1);
        Assert.Equal(1, board.Picked);
        board.Walk(-1);
        Assert.Null(board.Picked);
        board.Walk(-1);
        Assert.Equal(2, board.Picked);
        board.Pick(0);
        Assert.Null(board.Picked);
    }

    [Fact]
    public void Duplicates_caps_and_the_total_drop_late_rows_never_shown_ones()
    {
        var board = Board(Top(Rows.Search("inv")), Rows.File("a.pdf"));
        board.Arrive(1, Group.Files, [Rows.File("a.pdf"), Rows.File("b.pdf"), Rows.File("c.pdf"), Rows.File("d.pdf")], 3, 16);
        Assert.Equal(3, board.Rows.Count(r => r.Group == Group.Files));
        Assert.Single(board.Rows, r => r.Key == RowKey.File(@"C:\t\a.pdf"));

        board.Arrive(1, Group.Apps, [.. Enumerable.Range(0, 9).Select(i => new FieldRow(Group.Apps, $"app:{i}", $"{i}", "", RowAction.Launch, $"{i}"))], 9, 6);
        Assert.Equal(6, board.Rows.Count);
    }

    [Fact]
    public void Answers_to_an_older_question_are_dropped()
    {
        var board = Board(Top(Rows.Search("x")));
        board.Reset(2, [Top(Rows.Search("xy"))], null);
        Assert.False(board.Arrive(1, Group.Files, [Rows.File("x.txt")], 4, 16));
        Assert.False(board.Settle(1, Group.Files));
        Assert.Single(board.Rows);
    }

    [Fact]
    public void Many_late_batches_keep_every_shown_row_in_order()
    {
        var random = new Random(7);
        for (var trial = 0; trial < 200; trial++)
        {
            var board = Board(Top(Rows.Search("q")), Rows.Page(Group.Tabs, "t.com"), Rows.Page(Group.History, "h1.com"),
                Rows.Page(Group.History, "h2.com"), Rows.Search("q2"));
            var groups = new[] { Group.Answer, Group.Tabs, Group.History, Group.Files, Group.Apps, Group.Clipboard };
            for (var batch = 0; batch < 6; batch++)
            {
                if (random.Next(3) == 0) board.Walk(1);
                if (random.Next(4) == 0) board.Hold(random.Next(board.Rows.Count));
                var before = board.Keys();
                var picked = board.Picked;
                var pickedKey = picked is { } p ? before[p] : null;
                var group = groups[random.Next(groups.Length)];
                var late = Enumerable.Range(0, random.Next(1, 4))
                    .Select(i => new FieldRow(group, $"{group}:{trial}:{batch}:{i}", "", "", RowAction.Go, "")).ToList();
                board.Arrive(1, group, late, 5, 30);
                var after = board.Keys();
                Assert.Equal(before, after.Where(before.Contains));
                if (picked is { } kept)
                {
                    Assert.Equal(kept, board.Picked);
                    Assert.Equal(pickedKey, after[kept]);
                    Assert.Equal(before.Take(kept + 1), after.Take(kept + 1));
                }
                Assert.Equal(before[0], after[0]);
            }
        }
    }
}
