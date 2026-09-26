using System.Diagnostics;
using SearchKit.Commands;

namespace SearchKit.Tests;

public class FieldInputTests
{
    private static readonly Bangs bangs = new();

    [Theory]
    [InlineData("!yt cats", "yt", "cats")]
    [InlineData("  !yt   funny cats ", "yt", "funny cats")]
    [InlineData("funny cats !yt", "yt", "funny cats")]
    [InlineData("!YT cats", "yt", "cats")]
    [InlineData("!gh", "gh", "")]
    public void Bangs_are_found_first_or_last(string typed, string trigger, string query)
    {
        var read = Assert.IsType<FieldInput.ToBang>(FieldInput.Read(typed, bangs));
        Assert.Equal(trigger, read.Bang.Trigger, ignoreCase: true);
        Assert.Equal(query, read.Query);
    }

    [Fact]
    public void An_unknown_bang_is_just_text()
    {
        var read = Assert.IsType<FieldInput.Plain>(FieldInput.Read("!nosuchbang hello", bangs));
        Assert.Equal("!nosuchbang hello", read.Text);
    }

    [Fact]
    public void A_backslash_takes_the_rest_literally()
    {
        Assert.Equal("!yt cats", Assert.IsType<FieldInput.Plain>(FieldInput.Read("\\!yt cats", bangs)).Text);
        Assert.Equal(">not a command", Assert.IsType<FieldInput.Plain>(FieldInput.Read("\\>not a command", bangs)).Text);
    }

    [Fact]
    public void Commands_and_questions_have_their_marks()
    {
        Assert.Equal("close others", Assert.IsType<FieldInput.ToCommand>(FieldInput.Read(">close others", bangs)).Query);
        Assert.Equal("what's the catch", Assert.IsType<FieldInput.ToAsk>(FieldInput.Read("? what's the catch", bangs)).Question);
        // A lone mark is just text.
        Assert.IsType<FieldInput.Plain>(FieldInput.Read(">", bangs));
        Assert.IsType<FieldInput.Plain>(FieldInput.Read("?", bangs));
    }

    [Fact]
    public void Addresses_and_words_are_plain()
    {
        Assert.Equal("github.com", Assert.IsType<FieldInput.Plain>(FieldInput.Read("github.com", bangs)).Text);
        Assert.Equal("rust ownership", Assert.IsType<FieldInput.Plain>(FieldInput.Read("rust ownership", bangs)).Text);
        Assert.IsType<FieldInput.Plain>(FieldInput.Read("hello! world", bangs));
    }

    [Fact]
    public void Reading_the_field_is_far_under_a_keystroke()
    {
        var inputs = new[] { "!yt cats", "funny cats !yt", ">close others", "github.com", "rust ownership", "\\!x" };
        FieldInput.Read("warm up", bangs);
        var clock = Stopwatch.StartNew();
        for (var i = 0; i < 10_000; i++) FieldInput.Read(inputs[i % inputs.Length], bangs);
        var perRead = clock.Elapsed.TotalMilliseconds / 10_000;
        Assert.True(perRead < 0.5, $"{perRead:F4} ms per read");
    }
}

public class BangTests
{
    [Fact]
    public void A_bang_escapes_the_words_into_its_address()
    {
        var yt = new Bangs().Find("yt")!;
        Assert.Equal("https://www.youtube.com/results?search_query=cats%20%26%20dogs", yt.For("cats & dogs")!.AbsoluteUri);
        Assert.Equal("https://www.youtube.com/", yt.Home()!.AbsoluteUri);
    }

    [Fact]
    public void Yours_are_kept_and_win_over_built_ins()
    {
        var file = Path.Combine(Path.GetTempPath(), $"bangs-{Guid.NewGuid():N}.json");
        try
        {
            var bangs = new Bangs(file);
            bangs.Add("!jira", "Jira", "https://example.atlassian.net/browse/{q}");
            bangs.Add("yt", "My tube", "https://tube.example/?q={q}");

            var again = new Bangs(file);
            Assert.Equal("https://example.atlassian.net/browse/ABC-1", again.Find("jira")!.For("ABC-1")!.AbsoluteUri);
            Assert.Equal("My tube", again.Find("yt")!.Name);

            Assert.True(again.Remove("yt"));
            Assert.Equal("YouTube", again.Find("yt")!.Name);
            Assert.Equal("YouTube", new Bangs(file).Find("yt")!.Name);
        }
        finally
        {
            File.Delete(file);
        }
    }

    [Theory]
    [InlineData("two words", "https://x.example/?q={q}")]
    [InlineData("nowords", "https://x.example/")]
    [InlineData("js", "javascript:alert({q})")]
    public void A_bang_that_could_not_work_is_refused(string trigger, string address)
    {
        Assert.Throws<ArgumentException>(() => new Bangs().Add(trigger, "x", address));
    }

    [Fact]
    public void A_broken_file_loses_nothing_built_in()
    {
        var file = Path.Combine(Path.GetTempPath(), $"bangs-{Guid.NewGuid():N}.json");
        File.WriteAllText(file, "{ not json");
        try { Assert.NotNull(new Bangs(file).Find("g")); }
        finally { File.Delete(file); }
    }
}

public class CommandRegistryTests
{
    [Fact]
    public async Task Bare_ffmpeg_opens_packs_while_install_ffmpeg_starts_the_download()
    {
        var registry = new CommandRegistry();
        var opened = 0;
        var installed = 0;
        SearchKit.Packs.PackCommands.Register(registry, () => opened++, () => installed++);

        Assert.Equal("open.ffmpeg", registry.Find("ffmpeg")[0].Command.Id);
        await registry.RunAsync(registry.Find("ffmpeg")[0].Command.Id, "", Source.Field, _ => Task.FromResult(true));
        Assert.Equal((1, 0), (opened, installed));

        Assert.Equal("packs.ffmpeg", registry.Find("install ffmpeg")[0].Command.Id);
        await registry.RunAsync(registry.Find("install ffmpeg")[0].Command.Id, "", Source.Field, _ => Task.FromResult(true));
        Assert.Equal((2, 1), (opened, installed));
    }

    private static CommandRegistry Registry(List<string> ran)
    {
        var registry = new CommandRegistry();
        registry.Add(new Command("tabs.close-others", "Close Other Tabs", Tier.Act, ["close others", "close other tabs"]),
            call => ran.Add($"close-others:{call.Argument}"));
        registry.Add(new Command("spaces.new", "New Space", Tier.Act, ["new space"]),
            call => ran.Add($"new-space:{call.Argument}"));
        registry.Add(new Command("history.clear", "Clear History", Tier.AlwaysAsks, ["clear history"]),
            call => ran.Add("cleared"));
        registry.Add(new Command("tabs.reopen", "Reopen Closed Tab", Tier.Act, ["reopen"]),
            call => ran.Add("reopened"));
        return registry;
    }

    [Theory]
    [InlineData("close others", "tabs.close-others")]
    [InlineData("close", "tabs.close-others")]
    [InlineData("new sp", "spaces.new")]
    [InlineData("ns", "spaces.new")]
    [InlineData("history", "history.clear")]
    public void The_best_match_comes_first(string query, string id)
    {
        Assert.Equal(id, Registry([]).Find(query)[0].Command.Id);
    }

    [Fact]
    public void What_follows_the_words_is_the_argument()
    {
        var (command, argument) = Registry([]).Find("new space Work Stuff")[0];
        Assert.Equal("spaces.new", command.Id);
        Assert.Equal("Work Stuff", argument);
    }

    [Fact]
    public async Task Always_asks_means_nothing_happens_without_a_yes()
    {
        var ran = new List<string>();
        var registry = Registry(ran);
        var asked = 0;
        Assert.Equal(CommandRegistry.Outcome.Declined,
            await registry.RunAsync("history.clear", "", Source.Agent, _ => { asked++; return Task.FromResult(false); }));
        Assert.Empty(ran);
        Assert.Equal(CommandRegistry.Outcome.Ran,
            await registry.RunAsync("history.clear", "", Source.Script, _ => { asked++; return Task.FromResult(true); }));
        Assert.Equal(["cleared"], ran);
        Assert.Equal(2, asked);
    }

    [Fact]
    public async Task Act_commands_run_without_asking()
    {
        var ran = new List<string>();
        var result = await Registry(ran).RunAsync("tabs.reopen", "", Source.Voice, _ => throw new Exception("asked"));
        Assert.Equal(CommandRegistry.Outcome.Ran, result);
        Assert.Equal(["reopened"], ran);
    }

    [Fact]
    public void One_id_one_command()
    {
        var registry = Registry([]);
        Assert.Throws<InvalidOperationException>(() =>
            registry.Add(new Command("tabs.reopen", "Again", Tier.Act, ["again"]), _ => { }));
    }
}
