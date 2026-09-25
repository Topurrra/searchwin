namespace SearchKit.Web;

/// Whether a value set on something the window is drawn from (Search's
/// `Model.Set`) actually changed. An address changes with its fragment too:
/// `Uri`'s own equality ignores the fragment (RFC 3986: never sent to a
/// server), but Search's tool pages live entirely in it
/// (`tools.search/index.html#/play?path=<file>`), so the player moving to the
/// next file would otherwise be no change at all.
public static class Change
{
    public static bool Is<T>(T old, T now) =>
        old is Uri a && now is Uri b ? Text(a) != Text(b) : !EqualityComparer<T>.Default.Equals(old, now);

    private static string Text(Uri uri) => uri.IsAbsoluteUri ? uri.AbsoluteUri : uri.OriginalString;
}
