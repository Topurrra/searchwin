namespace SearchKit.Shields;

/// Where a filter list comes from, so the browser knows what to fetch and
/// how to label it in Settings. Nothing here fetches anything — downloading
/// is the caller's job, on its own schedule, never from a test.
public sealed record ListSource(string Name, Uri Url, string License)
{
    public static readonly ListSource EasyList = new(
        "EasyList", new Uri("https://easylist.to/easylist/easylist.txt"),
        "GPL-3.0 — downloaded at runtime, never bundled or committed.");

    public static readonly ListSource EasyPrivacy = new(
        "EasyPrivacy", new Uri("https://easylist.to/easylist/easyprivacy.txt"),
        "GPL-3.0 — downloaded at runtime, never bundled or committed.");

    public static readonly IReadOnlyList<ListSource> BuiltIn = [EasyList, EasyPrivacy];
}
