using SearchKit.Field;

namespace SearchKit.Tests.Field;

public class FieldMixTests
{
    private static FieldRow Top(FieldRow row) => row with { Group = Group.TopHit };

    [Fact]
    public void The_engine_top_hit_leads_then_the_browsers_rows_then_the_rest()
    {
        IReadOnlyList<FieldRow> board = [Top(Rows.Answer("1081")), Rows.File("a.txt"), Rows.File("b.txt")];
        var slots = FieldMix.Compose(2, board);
        Assert.Equal([FieldSlot.Engine(0), FieldSlot.Mine(0), FieldSlot.Mine(1), FieldSlot.Engine(1), FieldSlot.Engine(2)], slots);
    }

    [Fact]
    public void Without_a_top_hit_the_browsers_rows_come_first()
    {
        var slots = FieldMix.Compose(3, [Rows.File("a.txt")]);
        Assert.Equal([FieldSlot.Mine(0), FieldSlot.Mine(1), FieldSlot.Mine(2), FieldSlot.Engine(0)], slots);
        Assert.Empty(FieldMix.Compose(0, []));
    }

    [Fact]
    public void Late_rows_never_move_the_browsers_rows()
    {
        // A reserved top hit, filled in place, then files arriving below:
        // every local row stays at the index it had.
        var board = new FieldBoard();
        board.Reset(1, [FieldRow.Placeholder(Group.Answer)]);
        var before = FieldMix.Compose(3, board.Rows);
        board.Arrive(1, Group.Answer, [Rows.Answer("1081")], 3, 16);
        board.Arrive(1, Group.Files, [Rows.File("a.txt"), Rows.File("b.txt")], 4, 16);
        var after = FieldMix.Compose(3, board.Rows);
        for (var i = 0; i < before.Count; i++)
            if (before[i].IsLocal) Assert.Equal(before[i], after[i]);
        Assert.Equal(6, after.Count);
    }

    [Fact]
    public void Walking_skips_a_slot_still_waiting()
    {
        IReadOnlyList<FieldRow> board = [FieldRow.Placeholder(Group.Answer), Rows.File("a.txt")];
        var slots = FieldMix.Compose(2, board);
        Assert.Equal(1, FieldMix.Step(slots, board, null, 1));
        Assert.Equal(3, FieldMix.Step(slots, board, null, -1));
        Assert.Null(FieldMix.Step(slots, board, 1, -1));
        Assert.Equal(3, FieldMix.Step(slots, board, 2, 1));
        Assert.Null(FieldMix.Step(slots, board, 3, 1));
        Assert.Null(FieldMix.Step([], [], null, 1));
    }

    [Fact]
    public void A_pick_is_found_again_after_the_list_is_mixed_again()
    {
        IReadOnlyList<FieldRow> before = [FieldRow.Placeholder(Group.Answer), Rows.File("a.txt")];
        var slots = FieldMix.Compose(2, before);
        // Nothing answered: the slot goes, and everything moves up one.
        IReadOnlyList<FieldRow> after = [Rows.File("z.txt"), Rows.File("a.txt")];
        var again = FieldMix.Compose(2, after);
        Assert.Equal(0, FieldMix.Find(again, after, slots[1], null));
        Assert.Equal(3, FieldMix.Find(again, after, slots[3], before[1].Key));
        Assert.Null(FieldMix.Find(again, after, null, null));
        Assert.Null(FieldMix.Find(again, after, FieldSlot.Engine(0), "file:gone"));
    }
}

public class IndexPlanTests
{
    [Fact]
    public void Options_carry_the_folders_to_both_indexes()
    {
        var options = IndexPlan.Options([@"D:\Notes"], contents: true);
        Assert.Equal(@"D:\Notes", options["roots"]![0]!.GetValue<string>());
        Assert.Equal(@"D:\Notes", options["filenameRoots"]![0]!.GetValue<string>());
        Assert.True(options["indexContent"]!.GetValue<bool>());
        Assert.True(options["contentIndexingEnabled"]!.GetValue<bool>());
        Assert.False(options["includeHidden"]!.GetValue<bool>());
        Assert.True(options["watcherEnabled"]!.GetValue<bool>());
        // The engine requires these even when null.
        Assert.True(options.ContainsKey("maxContentKb"));
        Assert.True(options.ContainsKey("commitEvery"));
        Assert.False(IndexPlan.Options([@"D:\Notes"], contents: false)["indexContent"]!.GetValue<bool>());
    }

    [Fact]
    public void A_folder_chosen_under_an_excluded_one_is_still_indexed()
    {
        var excludes = IndexPlan.Excludes([@"C:\Temp\search-test"]);
        Assert.DoesNotContain("temp", excludes);
        Assert.Contains("node_modules", excludes);
        Assert.Contains("tmp", excludes);

        var appData = IndexPlan.Excludes([@"C:\Users\me\AppData\Local\Notes"]);
        Assert.DoesNotContain("appdata/local", appData);
        Assert.Contains("appdata/locallow", appData);
        Assert.DoesNotContain("users/*/appdata/local/temp", IndexPlan.Excludes([@"C:\Users\me\AppData\Local\Temp\x"]));

        Assert.Equal(IndexPlan.DefaultExcludes.Concat(IndexPlan.Secrets), IndexPlan.Excludes([@"D:\Documents"]));
    }

    [Fact]
    public void Covers_only_whats_inside_the_folders()
    {
        IReadOnlyList<string> folders = [@"C:\Temp\Test", @"D:\Notes\"];
        Assert.True(IndexPlan.Covers(folders, @"c:\temp\test\a.txt"));
        Assert.True(IndexPlan.Covers(folders, @"C:/Temp/Test/deep/b.md"));
        Assert.True(IndexPlan.Covers(folders, @"D:\Notes\x.pdf"));
        Assert.True(IndexPlan.Covers(folders, @"C:\Temp\Test"));
        Assert.False(IndexPlan.Covers(folders, @"C:\Temp\Testing\a.txt"));
        Assert.False(IndexPlan.Covers(folders, @"C:\Other\a.txt"));
        Assert.False(IndexPlan.Covers([], @"C:\Temp\Test\a.txt"));
    }

    [Fact]
    public void Status_reads_the_engines_numbers()
    {
        var status = IndexStatus.Read(FakeEngine.Json(
            """{"indexedFiles": 12, "filenameIndexedFiles": 30, "indexing": false, "filenameIndexing": true, "lastIndexedAtMs": 1700000000000, "lastError": null}"""));
        Assert.Equal(30, status.Count);
        Assert.True(status.Indexing);
        Assert.Equal(1700000000000, status.LastIndexedMs);
        Assert.Null(status.Error);
        var empty = IndexStatus.Read(null);
        Assert.Equal(0, empty.Count);
        Assert.Null(empty.LastIndexedMs);
    }
}

public class GatedSourceTests
{
    [Fact]
    public async Task A_gate_closed_asks_nothing_and_keep_drops_rows()
    {
        var open = false;
        var inner = new FakeSource(Group.Files, (_, _) => Task.FromResult<IReadOnlyList<FieldRow>>([Rows.File("a.txt"), Rows.File("b.txt")]));
        var gated = new GatedSource(inner, _ => open, row => row.Title != "b.txt");
        var query = FieldQuery.Read("notes");
        Assert.False(gated.Wants(query));
        open = true;
        Assert.True(gated.Wants(query));
        var rows = await gated.SuggestAsync(query, 4, CancellationToken.None);
        Assert.Equal(["a.txt"], rows.Select(r => r.Title));
        Assert.Equal(Group.Files, gated.Group);
    }
}
