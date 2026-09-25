using SearchKit.Commands;
using SearchKit.Web;

namespace SearchKit.Tests;

public class ToolCatalogTests
{
    // The shape Tools/catalog.js writes (searchTools.ts's catalog()).
    private const string Json = """
        [
          {"id":"hash-check","name":"Hash Check","description":"Compute MD5, SHA-1, SHA-256, BLAKE3","pack":"Utilities"},
          {"id":"image-studio","name":"Image Studio","description":"Resize, crop","pack":"Images"},
          {"id":"ocr-image-to-text","name":"Image to Text (OCR)","description":"Extract text","pack":"Images"},
          {"id":"img-base64","name":"Image to Base64","description":"Data URIs","pack":"Development"},
          {"name":"No id"},
          {"id":"no-name"},
          42
        ]
        """;

    private static readonly IReadOnlyList<ToolEntry> Tools = ToolCatalog.Parse(Json);

    [Fact]
    public void Reads_the_tools_and_skips_what_isnt_one()
    {
        Assert.Equal(["hash-check", "image-studio", "ocr-image-to-text", "img-base64"], Tools.Select(t => t.Id));
        Assert.Equal(new ToolEntry("hash-check", "Hash Check", "Compute MD5, SHA-1, SHA-256, BLAKE3", "Utilities"), Tools[0]);
        // A missing or broken catalog is no tools, never a crash in the field.
        Assert.Empty(ToolCatalog.Parse("{\"not\":\"a list\"}"));
        Assert.Empty(ToolCatalog.Parse("[{\"id\":"));
    }

    [Theory]
    [InlineData("hash", "hash-check")]
    [InlineData("Hash Check", "hash-check")]
    [InlineData("image", "image-studio")]      // three start with it: the shortest name
    [InlineData("image to", "img-base64")]
    [InlineData("studio", "image-studio")]     // the start of a word, from four letters
    [InlineData("ocr", null)]                  // a word, but too short to guess from
    [InlineData("im", null)]
    [InlineData("check hash", null)]
    [InlineData("github.com", null)]
    public void Offers_a_tool_only_for_its_name(string typed, string? id) =>
        Assert.Equal(id, ToolCatalog.Offer(Tools, typed)?.Id);

    [Fact]
    public void Every_tool_is_a_command_that_opens_its_page()
    {
        var registry = new CommandRegistry();
        registry.Add(new Command("open.tools", "Tools", Tier.Read, ["tools", "all tools"]), _ => { });
        var opened = new List<string>();
        ToolCatalog.Register(registry, Tools, opened.Add);

        var (command, argument) = registry.Find("image studio")[0];
        Assert.Equal(("Image Studio", ""), (command.Title, argument));
        Assert.Equal(CommandRegistry.Outcome.Ran, registry.RunAsync(command.Id, "", Source.Field, _ => Task.FromResult(false)).Result);
        Assert.Equal(["image-studio"], opened);
    }

    [Fact]
    public void Each_tool_page_wears_its_own_icon()
    {
        Assert.Equal("tools.search-hash-check", ToolAddress.IconKey(ToolAddress.Resolve("search://tools/hash-check")));
        Assert.Equal("tools.search-image-studio", ToolAddress.IconKey(ToolAddress.Resolve("search://tools/image-studio")));
        // The index, the player and every other site keep their host's.
        Assert.Null(ToolAddress.IconKey(ToolAddress.Resolve("search://tools")));
        Assert.Null(ToolAddress.IconKey(new Uri("https://example.com/#/tool/hash-check")));
    }
}
