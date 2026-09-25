using SearchKit.Commands;
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

/// `>command`, `!bang`, `?question` and Ctrl+K's empty field are the
/// browser's own rows, the first already picked so Enter takes it. The
/// engine's side must neither reserve a top hit above them nor move the pick.
public class FieldMixRegressionTests
{
    private static readonly Bangs bangs = new();

    /// Every engine source the browser has, all switched on.
    private static FieldModel Browserlike(FakeEngine engine) => new(
        local: [],
        engine:
        [
            new FileNameSource(engine),
            new FileContentSource(engine),
            new AppSource(engine),
            new ClipboardSource(engine, inField: true),
            new AnswerSource(engine),
        ],
        new FieldOptions { Bangs = bangs, IsAddress = FieldQuery.LooksLikeAddress });

    [Theory]
    [InlineData(">dark")]
    [InlineData(">tools")]
    [InlineData("!yt cats")]
    [InlineData("cats !yt")]
    [InlineData("? what is this")]
    [InlineData("")]
    public async Task The_browsers_first_row_stays_picked(string typed)
    {
        var engine = new FakeEngine();
        using var model = Browserlike(engine);
        model.Type(typed);
        await model.WhenSettled.WaitAsync(TimeSpan.FromSeconds(5));
        var board = model.Board.Rows;
        Assert.DoesNotContain(board, r => r.Group == Group.TopHit);

        // The browser's rows (say three), the first picked; the board lands.
        var slots = FieldMix.Compose(3, board);
        Assert.Equal(FieldSlot.Mine(0), slots[0]);
        Assert.Equal(0, FieldMix.Find(slots, board, slots[0], null));
        Assert.Null(model.Board.EnterRow);
        Assert.False(model.Board.Waiting);
    }

    [Theory]
    [InlineData(">dark", QueryKind.Command)]
    [InlineData("!yt cats", QueryKind.Bang)]
    [InlineData("? what is this", QueryKind.Ask)]
    public void None_of_them_is_an_answer_or_a_scope(string typed, QueryKind kind)
    {
        var query = FieldQuery.Read(typed, bangs, FieldQuery.LooksLikeAddress);
        Assert.Equal(kind, query.Kind);
        Assert.False(query.IsAnswer);
        Assert.Equal(Scope.All, query.Scope);
    }
}
