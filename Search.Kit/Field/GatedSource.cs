namespace SearchKit.Field;

/// An engine source the browser can switch off, or narrow, without the
/// source knowing: `allowed` is asked per keystroke (a setting, whether the
/// engine is there at all), and `keep` drops rows that shouldn't be shown
/// (a file from a folder no longer chosen).
public sealed class GatedSource(IEngineSource inner, Func<FieldQuery, bool> allowed, Func<FieldRow, bool>? keep = null) : IEngineSource
{
    public Group Group => inner.Group;

    public bool Wants(FieldQuery query) => allowed(query) && inner.Wants(query);

    public TimeSpan Delay(FieldQuery query) => inner.Delay(query);

    public async Task<IReadOnlyList<FieldRow>> SuggestAsync(FieldQuery query, int limit, CancellationToken cancel)
    {
        var rows = await inner.SuggestAsync(query, limit, cancel).ConfigureAwait(false);
        return keep == null ? rows : [.. rows.Where(keep)];
    }
}
