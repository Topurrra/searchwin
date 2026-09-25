using SearchKit.Web;

namespace SearchKit.Tests;

/// What `Model.Set` takes for a change, so the window redraws. The player
/// bug: next/previous moved to a new file (a change only in the address's
/// fragment) and the tab kept showing the file that had stopped playing.
public class ChangeTests
{
    private static readonly Uri ToneA = new("https://tools.search/index.html#/play?path=C%3A%5Ctone-a.wav");
    private static readonly Uri ToneB = new("https://tools.search/index.html#/play?path=C%3A%5Ctone-b.wav");

    [Fact]
    public void An_address_moving_only_in_its_fragment_is_a_change()
    {
        Assert.True(Change.Is<Uri?>(ToneA, ToneB));
        Assert.False(Change.Is<Uri?>(ToneA, new Uri(ToneA.AbsoluteUri)));
    }

    [Fact]
    public void Relative_addresses_compare_too()
    {
        Assert.False(Change.Is(new Uri("a#x", UriKind.Relative), new Uri("a#x", UriKind.Relative)));
        Assert.True(Change.Is(new Uri("a#x", UriKind.Relative), new Uri("a#y", UriKind.Relative)));
    }
}
