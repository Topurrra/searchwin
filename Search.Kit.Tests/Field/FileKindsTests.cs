using SearchKit.Field;

namespace SearchKit.Tests.Field;

public class FileKindsTests
{
    [Theory]
    [InlineData("exe")]
    [InlineData(".EXE")]
    [InlineData("com")]
    [InlineData("bat")]
    [InlineData("cmd")]
    [InlineData("ps1")]
    [InlineData("vbs")]
    [InlineData("js")]
    [InlineData("wsf")]
    [InlineData("hta")]
    [InlineData("scr")]
    [InlineData("msi")]
    [InlineData("reg")]
    [InlineData("lnk")]
    [InlineData("url")]
    [InlineData("appref-ms")]
    [InlineData("jar")]
    [InlineData("cpl")]
    [InlineData("msc")]
    public void Programs_and_scripts_are_only_shown_in_their_folder(string extension)
    {
        Assert.True(FileKinds.Runs(extension));
        Assert.Equal(RowAction.Reveal, FileKinds.ActionFor(extension));
    }

    [Theory]
    [InlineData("pdf", RowAction.OpenInTab)]
    [InlineData("json", RowAction.OpenInTab)]
    [InlineData("mp3", RowAction.Play)]
    [InlineData("docx", RowAction.OpenWithApp)]
    [InlineData("", RowAction.OpenWithApp)]
    public void Documents_open_as_before(string extension, RowAction action)
    {
        Assert.False(FileKinds.Runs(extension));
        Assert.Equal(action, FileKinds.ActionFor(extension));
    }

    [Fact]
    public void Whatever_PATHEXT_adds_runs_too()
    {
        var was = Environment.GetEnvironmentVariable("PATHEXT");
        // Read once per process: only check that what's there counts.
        foreach (var ext in (was ?? "").Split(';', StringSplitOptions.RemoveEmptyEntries))
            Assert.True(FileKinds.Runs(ext), ext);
    }

    [Theory]
    [InlineData(@"C:\t\setup.exe")]
    [InlineData(@"C:\t\setup.exe.")]
    [InlineData(@"C:\t\setup.exe . .")]
    [InlineData(@"C:\t\Run Me.LNK")]
    [InlineData("/home/t/run.sh.bat")]
    public void A_path_is_judged_as_Windows_would_open_it(string path)
    {
        Assert.Equal(RowAction.Reveal, FileKinds.ActionForPath(path));
    }

    [Fact]
    public void A_folder_opens_in_Explorer_whatever_its_name()
    {
        Assert.Equal(RowAction.OpenWithApp, FileKinds.ActionForPath(@"C:\t\tools.exe", folder: true));
        Assert.Equal(RowAction.OpenInTab, FileKinds.ActionForPath(@"C:\t\exe\notes.txt"));
    }

    [Fact]
    public async Task Engine_rows_for_programs_show_in_their_folder()
    {
        const string results = """
            {"results":[
              {"path":"C:\\t\\setup.exe","fileName":"setup.exe","entryType":"file","extension":"exe"},
              {"path":"C:\\t\\deploy.ps1.","fileName":"deploy.ps1.","entryType":"file","extension":""},
              {"path":"C:\\t\\odd","fileName":"odd","entryType":"file","extension":"bat"},
              {"path":"C:\\t\\notes.txt","fileName":"notes.txt","entryType":"file","extension":"txt"}
            ]}
            """;
        var engine = new FakeEngine { Answer = (_, _, _) => Task.FromResult(FakeEngine.Json(results)) };
        var rows = await new FileNameSource(engine).SuggestAsync(FieldQuery.Read("files: s"), 8, default);
        Assert.Equal([RowAction.Reveal, RowAction.Reveal, RowAction.Reveal, RowAction.OpenInTab], rows.Select(r => r.Action));
    }

    [Fact]
    public void A_program_never_fills_the_scopes_top_hit()
    {
        // `files: setup`: the first file is what Enter takes, unless it's a
        // program; that goes below, and the next file may lead.
        var board = new FieldBoard();
        board.Reset(1, [FieldRow.Placeholder(Group.Files)], null);
        board.Arrive(1, Group.Files, [Rows.File("setup.exe"), Rows.File("setup.txt")], 12, 16);
        var rows = board.Rows;
        Assert.Equal((Group.TopHit, RowAction.OpenInTab), (rows[0].Group, rows[0].Action));
        Assert.Equal((Group.Files, RowAction.Reveal), (rows[1].Group, rows[1].Action));
        Assert.Equal("setup.txt", board.EnterRow!.Title);

        // Only programs: nothing to take, and the empty slot goes.
        board.Reset(2, [FieldRow.Placeholder(Group.Files)], null);
        board.Arrive(2, Group.Files, [Rows.File("setup.exe"), Rows.File("install.cmd")], 12, 16);
        Assert.True(board.Waiting);
        Assert.Null(board.EnterRow);
        Assert.True(board.Settle(2, Group.Files));
        Assert.Null(board.EnterRow);
        Assert.All(board.Rows, r => Assert.Equal(RowAction.Reveal, r.Action));
    }

    [Fact]
    public async Task Enter_on_files_scope_with_only_programs_takes_nothing()
    {
        var engine = new FakeEngine
        {
            Answer = (_, _, _) => Task.FromResult(FakeEngine.Json("""
                {"results":[{"path":"C:\\t\\setup.exe","fileName":"setup.exe","entryType":"file","extension":"exe"}]}
                """)),
        };
        using var model = new FieldModel([], [new FileNameSource(engine)]);
        model.Type("files: setup");
        Assert.Null(await model.EnterAsync(TimeSpan.FromSeconds(5)));
        Assert.Contains(model.Board.Rows, r => r.Action == RowAction.Reveal);
    }
}
