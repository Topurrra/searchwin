using System.Text.Json.Nodes;
using SearchKit.Field;

namespace SearchKit.Tests.Field;

/// The clipboard history as Ctrl+Shift+V lists it.
public class ClipListTests
{
    private const string History = """
        [
          {"id":9,"capturedAtMs":900,"kind":"text","text":"invoice #42\n for March","sourceApp":"outlook.exe","sensitiveKinds":[],"isPinned":false},
          {"id":8,"capturedAtMs":800,"kind":"text","text":"hunter2 is not a real secret","sourceApp":"code.exe","sensitiveKinds":["sensitive","github_token"],"isPinned":false},
          {"id":7,"capturedAtMs":700,"kind":"image","text":"","sourceApp":"snip.exe","sensitiveKinds":[],"isPinned":false,"imageWidth":800,"imageHeight":600,"imagePath":"C:\\c\\7.png","thumbnailPath":"C:\\c\\7-thumb.png"},
          {"id":6,"capturedAtMs":600,"kind":"text","text":"old invoice notes","sourceApp":null,"sensitiveKinds":[],"isPinned":true,"pinLabel":"Invoice template"},
          {"id":5,"capturedAtMs":500,"kind":"text","text":"","sensitiveKinds":[],"isPinned":false},
          {"id":4,"capturedAtMs":400,"kind":"text","text":"4111 1111 1111 1111","sensitiveKinds":["credit_card"],"isPinned":true}
        ]
        """;

    private static List<ClipEntry> Entries(bool secrets = true) => ClipList.Read(JsonNode.Parse(History), secrets);

    [Fact]
    public void Pinned_come_first_then_newest()
    {
        Assert.Equal([6, 4, 9, 8, 7], ClipList.Filter(Entries(), "").Select(e => e.Id));
    }

    [Fact]
    public void An_empty_entry_is_not_one()
    {
        Assert.DoesNotContain(Entries(), e => e.Id == 5);
    }

    [Fact]
    public void A_secret_shows_only_its_kind_until_asked()
    {
        var secret = Entries().Single(e => e.Id == 8);
        Assert.True(secret.Sensitive);
        Assert.Equal("Hidden: github token", secret.Face);
        Assert.DoesNotContain("hunter2", secret.Face);
        // Pasting needs the text, and a click shows it.
        Assert.Equal("hunter2 is not a real secret", secret.Text);
        Assert.Equal("hunter2 is not a real secret", secret.Shown);
        Assert.Equal("Hidden: credit card", Entries().Single(e => e.Id == 4).Face);
        Assert.Equal("secret", ClipEntry.Secret(["sensitive"]));
    }

    [Fact]
    public void A_secret_is_never_found_by_its_text()
    {
        Assert.Empty(ClipList.Filter(Entries(), "hunter2"));
        Assert.Empty(ClipList.Filter(Entries(), "4111"));
        Assert.Equal([8], ClipList.Filter(Entries(), "github").Select(e => e.Id));
        Assert.Equal([4], ClipList.Filter(Entries(), "card").Select(e => e.Id));
    }

    [Fact]
    public void Read_without_secrets_drops_their_text()
    {
        var secret = Entries(secrets: false).Single(e => e.Id == 8);
        Assert.Equal("", secret.Text);
        Assert.Equal("Hidden: github token", secret.Face);
    }

    [Fact]
    public void Text_is_found_by_its_words_and_label_and_shown_on_one_line()
    {
        Assert.Equal([6, 9], ClipList.Filter(Entries(), "INVOICE").Select(e => e.Id));
        Assert.Equal([6], ClipList.Filter(Entries(), "template").Select(e => e.Id));
        Assert.Equal("invoice #42 for March", Entries().Single(e => e.Id == 9).Face);
    }

    [Fact]
    public void A_picture_has_its_size_and_thumbnail()
    {
        var picture = Entries().Single(e => e.Id == 7);
        Assert.True(picture.Image);
        Assert.Equal("Image 800×600", picture.Face);
        Assert.Equal(@"C:\c\7-thumb.png", picture.Thumbnail);
        Assert.Equal([7], ClipList.Filter(Entries(), "image").Select(e => e.Id));
    }

    [Fact]
    public void A_limit_keeps_the_first()
    {
        Assert.Equal([6, 4], ClipList.Filter(Entries(), "", 2).Select(e => e.Id));
    }

    [Fact]
    public void Odd_shapes_read_as_nothing()
    {
        Assert.Empty(ClipList.Read(null));
        Assert.Empty(ClipList.Read(JsonNode.Parse("""{"id":1}""")));
        Assert.Empty(ClipList.Read(JsonNode.Parse("""[1, "x", null, {"id":"nope"}]""")));
    }
}
