using SearchKit.Web;

namespace SearchKit.Tests;

/// The player tab bug: next/previous/auto-advance moved to a new file (a
/// change only in the URL fragment), but the tab kept showing the file that
/// had just stopped playing. Root cause: Model.Set&lt;T&gt; (Search/Core/Model.cs)
/// compared the old and new Uri with EqualityComparer&lt;Uri&gt;.Default, and
/// Uri's own equality ignores the fragment — so tone-a.wav and tone-b.wav,
/// differing only after the '#', compared equal and the address field never
/// actually changed.
public class UriEqualityTests
{
    private static readonly Uri ToneA = new("https://tools.search/index.html#/play?path=C%3A%5Ctone-a.wav");
    private static readonly Uri ToneB = new("https://tools.search/index.html#/play?path=C%3A%5Ctone-b.wav");

    [Fact]
    public void Addresses_differing_only_in_the_fragment_are_not_the_same()
    {
        // Regression: fails if this reads `ToneA.Equals(ToneB)` (or anything
        // else that falls back to Uri's own equality), because .NET's Uri
        // comparison ignores the fragment by design — the two would compare
        // equal even though they name different files.
        Assert.False(UriEquality.SameAddress(ToneA, ToneB));
        // The gap this guards against: plain Uri equality really does think
        // they're the same address.
        Assert.True(ToneA.Equals(ToneB));
    }

    [Fact]
    public void The_same_address_is_still_the_same() =>
        Assert.True(UriEquality.SameAddress(ToneA, new Uri(ToneA.AbsoluteUri)));

    [Fact]
    public void Null_is_only_equal_to_null()
    {
        Assert.True(UriEquality.SameAddress(null, null));
        Assert.False(UriEquality.SameAddress(ToneA, null));
        Assert.False(UriEquality.SameAddress(null, ToneA));
    }
}
