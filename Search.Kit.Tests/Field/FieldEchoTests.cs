using SearchKit.Field;

namespace SearchKit.Tests.Field;

public class FieldEchoTests
{
    [Fact]
    public void Two_puts_in_one_turn_are_both_ours()
    {
        // `>dark`: the list redrawn puts the typed text, then its first row
        // picked puts the command's title. The box reports twice, each time
        // reading the text as it is by then: the title.
        var echo = new FieldEcho();
        echo.Put(">dark");
        echo.Put("Dark");
        Assert.True(echo.Ours("Dark"));
        Assert.True(echo.Ours("Dark"));
    }

    [Fact]
    public void A_key_is_never_taken_for_ours()
    {
        var echo = new FieldEcho();
        Assert.False(echo.Ours("a"));
        echo.Put("github.com");
        Assert.True(echo.Ours("github.com"));
        // Backspace, then the same letter typed back: both are typing.
        Assert.False(echo.Ours("github.co"));
        Assert.False(echo.Ours("github.com"));
    }

    [Fact]
    public void A_key_before_the_report_is_typing()
    {
        var echo = new FieldEcho();
        echo.Put("Dark");
        // The report arrives after a key already changed the text.
        Assert.False(echo.Ours("Darkx"));
        Assert.False(echo.Ours("Darkx"));
    }
}
