namespace SearchKit.Web;

/// .NET's default `Uri` equality (what `EqualityComparer&lt;Uri&gt;.Default`
/// uses, and so anything that compares two `Uri?` the ordinary way) ignores
/// the fragment: RFC 3986 says a fragment is never sent to a server, so two
/// URIs differing only there are "the same resource" as far as `Uri.Equals`
/// is concerned. Search's own tool pages live entirely in the fragment
/// (`https://tools.search/index.html#/play?path=<file>`), so a change that
/// only moves the playing file — tone-a.wav to tone-b.wav — compares equal
/// under the default comparer, and a caller keyed off "did this change?"
/// (a bound `Set`, a dictionary, a duplicate check) misses it entirely.
public static class UriEquality
{
    /// True only when `a` and `b` are the exact same address, fragment
    /// included. Null is only equal to null.
    public static bool SameAddress(Uri? a, Uri? b) => a?.AbsoluteUri == b?.AbsoluteUri;
}
