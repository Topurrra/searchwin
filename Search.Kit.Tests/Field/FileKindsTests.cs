using SearchKit.Field;

namespace SearchKit.Tests.Field;

public class FileKindsTests
{
    [Theory]
    // Video WebView2 (Chromium) actually plays.
    [InlineData("mp4", RowAction.Play)]
    [InlineData("m4v", RowAction.Play)]
    [InlineData("webm", RowAction.Play)]
    [InlineData("mov", RowAction.Play)]
    [InlineData("ogv", RowAction.Play)]
    // Audio.
    [InlineData("mp3", RowAction.Play)]
    [InlineData("m4a", RowAction.Play)]
    [InlineData("aac", RowAction.Play)]
    [InlineData("wav", RowAction.Play)]
    [InlineData("ogg", RowAction.Play)]
    [InlineData("oga", RowAction.Play)]
    [InlineData("opus", RowAction.Play)]
    [InlineData("flac", RowAction.Play)]
    // Not playable: WebView2 has no demuxer for these without the FFmpeg
    // add-on, so they fall back to the file's own app.
    [InlineData("mkv", RowAction.OpenWithApp)]
    [InlineData("avi", RowAction.OpenWithApp)]
    [InlineData("wmv", RowAction.OpenWithApp)]
    [InlineData("flv", RowAction.OpenWithApp)]
    // Shown in a tab.
    [InlineData("pdf", RowAction.OpenInTab)]
    [InlineData("txt", RowAction.OpenInTab)]
    [InlineData("png", RowAction.OpenInTab)]
    // Everything else opens in its own app.
    [InlineData("exe", RowAction.OpenWithApp)]
    [InlineData("docx", RowAction.OpenWithApp)]
    [InlineData("", RowAction.OpenWithApp)]
    public void ActionFor_bare_extension(string extension, RowAction expected) =>
        Assert.Equal(expected, FileKinds.ActionFor(extension));

    [Theory]
    [InlineData(".mp4")]
    [InlineData(".MP4")]
    [InlineData(".Mp4")]
    public void ActionFor_ignores_dot_and_case(string extension) =>
        Assert.Equal(RowAction.Play, FileKinds.ActionFor(extension));

    [Fact]
    public void ActionFor_folder_always_opens_with_app() =>
        Assert.Equal(RowAction.OpenWithApp, FileKinds.ActionFor("mp4", folder: true));
}
