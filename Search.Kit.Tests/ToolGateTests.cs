using System.Text.Json.Nodes;
using SearchKit.Engine;
using SearchKit.Web;

namespace SearchKit.Tests;

/// Who reaches the tool pages, and which paths a tool page may use: the
/// review's window.open → player → network share finding.
public class ToolGateTests
{
    [Theory]
    // Anything but a tool page: always.
    [InlineData(false, false, false, true, true)]
    // Into a tool page: Search's own navigation, from a tool page, or back/forward/reload.
    [InlineData(true, false, true, true, true)]
    [InlineData(true, true, false, true, true)]
    [InlineData(true, false, false, false, true)]
    // A web page (or a fresh popup, which has no document yet) sending itself there: no.
    [InlineData(true, false, false, true, false)]
    public void Only_Search_or_a_tool_page_navigates_into_the_tools(bool toTools, bool fromTools, bool ours, bool newDocument, bool may) =>
        Assert.Equal(may, ToolGate.MayNavigate(toTools, fromTools, ours, newDocument));

    [Fact]
    public void A_page_never_opens_a_window_onto_a_tool_page()
    {
        Assert.False(ToolGate.MayOpenWindow(toTools: true));
        Assert.True(ToolGate.MayOpenWindow(toTools: false));
    }

    [Theory]
    [InlineData(@"\\attacker\share\a.mp4")]
    [InlineData("//attacker/share/a.mp4")]
    [InlineData(@"/\attacker\share")]
    [InlineData(@"\\?\UNC\attacker\share\a.mp4")]
    [InlineData(@"\\?\C:\Music\a.mp4")]
    [InlineData(@"\\.\pipe\x")]
    [InlineData(@"\??\C:\Music\a.mp4")]
    [InlineData(@"  \\attacker\share")]
    public void Shares_and_device_paths_are_refused(string path)
    {
        Assert.True(ToolGate.IsRemote(path));
        Assert.NotNull(ToolGate.PathRefused(path, []));
    }

    [Theory]
    [InlineData(@"C:\Music\a.mp4")]
    [InlineData("C:/Music/a.mp4")]
    [InlineData(@"E:\")]
    [InlineData("")]
    public void Local_paths_go_through(string path) =>
        Assert.Null(ToolGate.PathRefused(path, []));

    [Fact]
    public void A_share_chosen_in_Settings_is_allowed_but_not_climbed_out_of()
    {
        IReadOnlyList<string> chosen = [@"\\nas\media"];
        Assert.Null(ToolGate.PathRefused(@"\\nas\media\films\a.mp4", chosen));
        Assert.Null(ToolGate.PathRefused("//NAS/Media/films/a.mp4", chosen));
        Assert.NotNull(ToolGate.PathRefused(@"\\nas\media\..\private\a.mp4", chosen));
        Assert.NotNull(ToolGate.PathRefused(@"\\nas\mediakit\a.mp4", chosen));
        Assert.NotNull(ToolGate.PathRefused(@"\\attacker\share\a.mp4", chosen));
    }

    [Fact]
    public void Any_argument_named_like_a_path_is_checked_at_any_depth()
    {
        Assert.NotNull(ToolGate.ArgsRefused(new JsonObject { ["path"] = @"\\attacker\share" }, []));
        Assert.NotNull(ToolGate.ArgsRefused(new JsonObject { ["folderPath"] = "//attacker/share" }, []));
        Assert.NotNull(ToolGate.ArgsRefused(new JsonObject { ["paths"] = new JsonArray(@"C:\a", @"\\attacker\b") }, []));
        Assert.NotNull(ToolGate.ArgsRefused(new JsonObject { ["options"] = new JsonObject { ["roots"] = new JsonArray(@"\\attacker\share") } }, []));
        // Text that merely starts with slashes isn't a path.
        Assert.Null(ToolGate.ArgsRefused(new JsonObject { ["contents"] = "// a comment", ["path"] = @"C:\notes.txt" }, []));
        Assert.Null(ToolGate.ArgsRefused(new JsonObject { ["path"] = @"C:\Music" }, []));
        Assert.Null(ToolGate.ArgsRefused(null, []));
    }

    // MARK: - addresses

    [Theory]
    [InlineData(@"C:\Music\plain.mp3")]
    [InlineData(@"C:\Music\Rock & Roll\a.mp3")]
    [InlineData(@"C:\Music\#1 hit.mp3")]
    [InlineData(@"C:\Music\100% pure.mp3")]
    [InlineData(@"C:\Music\a+b c.mp3")]
    [InlineData(@"C:\Music\%20 not a space & #x +.mp3")]
    [InlineData(@"E:\song.mp3")]
    public void The_player_s_address_survives_being_shown_and_typed_back(string path)
    {
        var page = ToolAddress.Resolve($"search://play?path={Uri.EscapeDataString(path)}")!;
        var shown = ToolAddress.Pretty(page);
        Assert.StartsWith("search://play?path=", shown);
        var again = ToolAddress.Resolve(shown)!;
        Assert.Equal(page.AbsoluteUri, again.AbsoluteUri);
        Assert.Equal(shown, ToolAddress.Pretty(again));
    }

    [Fact]
    public void Pretty_keeps_a_path_readable()
    {
        var page = ToolAddress.Resolve($"search://play?path={Uri.EscapeDataString(@"C:\My Music\a.mp3")}")!;
        Assert.Equal(@"search://play?path=C:\My Music\a.mp3", ToolAddress.Pretty(page));
    }

    [Fact]
    public void Tool_addresses_resolve_as_before()
    {
        Assert.Equal("https://tools.search/index.html", ToolAddress.Resolve("search://tools")!.AbsoluteUri);
        Assert.Equal("https://tools.search/index.html#/tool/json", ToolAddress.Resolve("search://tools/json")!.AbsoluteUri);
        Assert.Equal("search://tools/json", ToolAddress.Pretty(ToolAddress.Resolve("search://tools/json")!));
        Assert.Null(ToolAddress.Resolve("search://elsewhere"));
        Assert.Null(ToolAddress.Resolve("search://play"));
    }

    [Fact]
    public void The_player_refuses_what_it_is_told_to()
    {
        var share = $"search://play?path={Uri.EscapeDataString(@"\\attacker\share\a.mp4")}";
        Assert.Null(ToolAddress.Resolve(share, path => ToolGate.PathRefused(path, []) != null));
        Assert.NotNull(ToolAddress.Resolve(share));
    }

    // MARK: - the engine

    [Fact]
    public void An_old_engine_is_told_apart_by_what_it_lacks()
    {
        var all = new JsonArray([.. EngineMethods.Required.Select(m => (JsonNode)m), "other"]);
        Assert.Empty(EngineMethods.Missing(all));
        var old = new JsonArray([.. EngineMethods.Required.Where(m => m != "clear_file_search_index").Select(m => (JsonNode)m)]);
        Assert.Equal(["clear_file_search_index"], EngineMethods.Missing(old));
        Assert.Equal(EngineMethods.Required.Count, EngineMethods.Missing(null).Count);
    }
}
