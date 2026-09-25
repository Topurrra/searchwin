using SearchKit.Commands;

namespace SearchKit.Field;

/// An open tab, as the field sees it.
public sealed record OpenPage(Guid Id, string Title, Uri Url, DateTime Touched, bool Active = false);

/// Somewhere you have been: the browser's History.Visit. `Key` is its
/// history key (Address.Pretty, lower-cased), `Url` the full address.
public sealed record Place(string Key, string Title, string Url, int Count, DateTime Last);

/// A bookmark with an address (folders don't come here).
public sealed record Mark(string Title, Uri Url);

internal static class Needles
{
    /// What the user meant, the way history keys are written: lower-case,
    /// without a scheme or `www.`.
    public static string Of(string text)
    {
        var needle = text.Trim().ToLowerInvariant();
        if (needle.StartsWith("https://", StringComparison.Ordinal)) needle = needle[8..];
        else if (needle.StartsWith("http://", StringComparison.Ordinal)) needle = needle[7..];
        if (needle.StartsWith("www.", StringComparison.Ordinal)) needle = needle[4..];
        return needle;
    }

    /// The pretty form of an address (a URL key without its "url:").
    public static string Pretty(Uri url) => RowKey.Url(url)[4..];

    /// Whether a word inside `title` starts with `needle` ("rust" in "The Rust Book").
    public static bool WordStarts(string title, string needle)
    {
        var at = 0;
        while ((at = title.IndexOf(needle, at, StringComparison.OrdinalIgnoreCase)) >= 0)
        {
            if (at == 0 || !char.IsLetterOrDigit(title[at - 1])) return true;
            at++;
        }
        return false;
    }
}

/// Open tabs, most recently looked at first among equals. The active tab is
/// never offered: you're already there.
public sealed class TabSource(Func<IReadOnlyList<OpenPage>> pages) : ILocalSource
{
    /// A row scored this or more starts with what was typed (its address or
    /// its title): it can be the top hit.
    public const double Strong = 5;

    public Group Group => Group.Tabs;

    public IReadOnlyList<FieldRow> Suggest(FieldQuery query, int limit)
    {
        if (query.Scope is not (Scope.All or Scope.Tabs)) return [];
        if (query.Kind is not (QueryKind.Words or QueryKind.Address) && !(query.Kind == QueryKind.Empty && query.Scope == Scope.Tabs))
            return [];
        var needle = Needles.Of(query.Text);
        var scored = new List<(double Score, OpenPage Page, string Pretty)>();
        foreach (var page in pages())
        {
            if (page.Active) continue;
            var pretty = Needles.Pretty(page.Url);
            double score;
            if (needle.Length == 0) score = 1;
            else if (pretty.StartsWith(needle, StringComparison.Ordinal)) score = 6;
            else if (page.Title.StartsWith(needle, StringComparison.OrdinalIgnoreCase)) score = 5;
            else if (Needles.WordStarts(page.Title, needle)) score = 4;
            else if (needle.Length >= 2 && pretty.Contains(needle, StringComparison.Ordinal)) score = 2;
            else if (needle.Length >= 2 && page.Title.Contains(needle, StringComparison.OrdinalIgnoreCase)) score = 1.5;
            else continue;
            scored.Add((score, page, pretty));
        }
        return [.. scored
            .OrderByDescending(s => s.Score)
            .ThenByDescending(s => s.Page.Touched)
            .Take(limit)
            .Select(s => new FieldRow(Group.Tabs, RowKey.Url(s.Page.Url), s.Page.Title.Length > 0 ? s.Page.Title : s.Pretty,
                s.Pretty, RowAction.SwitchTab, s.Page.Url.AbsoluteUri) { Tab = s.Page.Id, Score = s.Score })];
    }
}

/// Where you've been, ranked the way the browser's History.Suggestions ranks
/// it: where the match falls first (the start of the host is what people
/// mean), then how often and how lately. A bare domain beats a page inside
/// it. The well-known addresses fill in only where memory has nothing.
///
/// Built for a big history: one pass, no allocation per place that doesn't
/// match, and addresses parsed only for the rows returned.
public sealed class HistorySource(
    Func<IReadOnlyList<Place>> places,
    IReadOnlyList<(string Key, string Title)>? known = null,
    Func<DateTime>? now = null) : ILocalSource
{
    public Group Group => Group.History;

    public IReadOnlyList<FieldRow> Suggest(FieldQuery query, int limit)
    {
        if (query.Scope is not (Scope.All or Scope.History)) return [];
        var browsing = query.Scope == Scope.History && query.Kind == QueryKind.Empty;
        if (query.Kind is not (QueryKind.Words or QueryKind.Address) && !browsing) return [];
        var needle = Needles.Of(query.Text);
        if (needle.Length == 0 && !browsing) return [];

        // The well-known names that could match, so the pass over the places
        // can strike the ones memory already has.
        Dictionary<string, (string Title, double Rank)>? famous = null;
        if (known != null && needle.Length > 0)
        {
            foreach (var (key, title) in known)
            {
                var rank = Rank(key, needle);
                if (rank > 0) (famous ??= new(StringComparer.Ordinal))[key] = (title, rank);
            }
        }

        // One pass, keeping only the best few: a sort of every match costs
        // more than the whole keystroke may when "s" matches thousands.
        var at = (now ?? (() => DateTime.UtcNow))();
        var best = new Best(limit + 2);
        foreach (var place in places())
        {
            var rank = needle.Length == 0 ? 1 : Rank(place.Key, needle);
            if (rank == 0) continue;
            famous?.Remove(place.Key);
            best.Offer(rank + 4 + Frecency(place, at) + (place.Key.Contains('/') ? 0 : 1.5), place);
        }
        if (famous != null)
            foreach (var (key, (title, rank)) in famous)
                best.Offer(rank, new Place(key, title, "https://" + key + "/", 0, DateTime.MinValue));

        var rows = new List<FieldRow>(limit);
        foreach (var (score, place) in best.Ranked)
        {
            if (rows.Count >= limit) break;
            if (!Uri.TryCreate(place.Url, UriKind.Absolute, out _)) continue;
            rows.Add(new FieldRow(Group.History, RowKey.Place(place.Key), place.Title.Length > 0 ? place.Title : place.Key,
                place.Key, RowAction.Go, place.Url) { Score = score });
        }
        return rows;
    }

    /// The `size` best places seen so far, best first: higher score, then
    /// the shorter key (the front door before the room).
    private sealed class Best(int size)
    {
        private readonly List<(double Score, Place Place)> kept = new(size + 1);

        public IReadOnlyList<(double Score, Place Place)> Ranked => kept;

        public void Offer(double score, Place place)
        {
            if (kept.Count == size && !Beats(score, place, kept[^1])) return;
            var at = kept.Count;
            while (at > 0 && Beats(score, place, kept[at - 1])) at--;
            kept.Insert(at, (score, place));
            if (kept.Count > size) kept.RemoveAt(size);
        }

        private static bool Beats(double score, Place place, (double Score, Place Place) other) =>
            score > other.Score || (score == other.Score && place.Key.Length < other.Place.Key.Length);
    }

    /// History.Rank, without the allocations: a prefix of the key; a prefix of
    /// the host once the first label is skipped ("hub" → github.com); from two
    /// letters up, anywhere in the host. Never the path.
    private static double Rank(string key, string needle)
    {
        if (key.StartsWith(needle, StringComparison.Ordinal)) return 6;
        var slash = key.IndexOf('/');
        var host = slash < 0 ? key.AsSpan() : key.AsSpan(0, slash);
        var dot = host.IndexOf('.');
        if (dot >= 0 && host[(dot + 1)..].StartsWith(needle, StringComparison.Ordinal)) return 3;
        if (needle.Length >= 2 && host.IndexOf(needle, StringComparison.Ordinal) >= 0) return 2;
        return 0;
    }

    /// Often, and lately: a month-old visit counts about a third of a fresh one.
    private static double Frecency(Place place, DateTime now)
    {
        var days = Math.Max(0, (now - place.Last).TotalDays);
        return place.Count * Math.Exp(-days / 30);
    }
}

/// Bookmarks whose title or address holds what was typed.
public sealed class BookmarkSource(Func<IReadOnlyList<Mark>> marks) : ILocalSource
{
    public Group Group => Group.Bookmarks;

    public IReadOnlyList<FieldRow> Suggest(FieldQuery query, int limit)
    {
        if (query.Scope != Scope.All || query.Kind is not (QueryKind.Words or QueryKind.Address)) return [];
        var needle = Needles.Of(query.Text);
        if (needle.Length < 2) return [];
        var scored = new List<(double Score, Mark Mark, string Pretty)>();
        foreach (var mark in marks())
        {
            var pretty = Needles.Pretty(mark.Url);
            double score;
            if (pretty.StartsWith(needle, StringComparison.Ordinal)) score = 6;
            else if (Needles.WordStarts(mark.Title, needle)) score = 4;
            else if (pretty.Contains(needle, StringComparison.Ordinal)) score = 2;
            else if (mark.Title.Contains(needle, StringComparison.OrdinalIgnoreCase)) score = 1.5;
            else continue;
            scored.Add((score, mark, pretty));
        }
        return [.. scored
            .OrderByDescending(s => s.Score)
            .ThenBy(s => s.Pretty.Length)
            .Take(limit)
            .Select(s => new FieldRow(Group.Bookmarks, RowKey.Url(s.Mark.Url), s.Mark.Title.Length > 0 ? s.Mark.Title : s.Pretty,
                s.Pretty, RowAction.Go, s.Mark.Url.AbsoluteUri) { Score = s.Score })];
    }
}

/// `>…`: the command registry's matches, the way the field lists them today.
public sealed class CommandSource(CommandRegistry registry) : ILocalSource
{
    public Group Group => Group.Commands;

    public IReadOnlyList<FieldRow> Suggest(FieldQuery query, int limit)
    {
        if (query.Kind != QueryKind.Command) return [];
        return [.. registry.Find(query.Text, limit).Select(match => new FieldRow(Group.Commands,
            RowKey.Command(match.Command.Id, match.Argument),
            match.Argument.Length > 0 ? $"{match.Command.Title} “{match.Argument}”" : match.Command.Title,
            match.Command.Keys ?? match.Command.Group ?? "",
            RowAction.RunCommand, match.Command.Id) { Argument = match.Argument })];
    }
}

/// What the typed text itself means: go to the address, follow the bang,
/// ask the question, or search the web for the words. `address` is the
/// browser's Address.Url; `search` its search engine (Google.Url and the
/// engine's name).
public sealed class TypedSource(Func<string, Uri?> address, Func<string, (string Name, Uri Url)?> search) : ILocalSource
{
    public Group Group => Group.Search;

    public IReadOnlyList<FieldRow> Suggest(FieldQuery query, int limit)
    {
        if (limit <= 0 || query.Scope != Scope.All) return [];
        switch (query.Kind)
        {
            case QueryKind.Address when address(query.Text) is { } url:
                return [new FieldRow(Group.Search, RowKey.Url(url), query.Text, Needles.Pretty(url), RowAction.Go, url.AbsoluteUri)];
            case QueryKind.Bang when query.Bang is { } bang:
                var there = query.Text.Length == 0 ? bang.Home() : bang.For(query.Text);
                return there == null ? []
                    : [new FieldRow(Group.Search, RowKey.Url(there), query.Text.Length == 0 ? bang.Name : query.Text, bang.Name,
                        RowAction.Go, there.AbsoluteUri)];
            case QueryKind.Ask:
                return [new FieldRow(Group.Search, "ask:" + query.Text, query.Text, "Ask", RowAction.Ask, query.Text)];
            case QueryKind.Words or QueryKind.Address or QueryKind.Calculation or QueryKind.Conversion
                or QueryKind.Color or QueryKind.Hash or QueryKind.Encode or QueryKind.Format:
                return search(query.Text) is { } asked
                    ? [new FieldRow(Group.Search, RowKey.WebSearch(query.Text), query.Text, asked.Name, RowAction.Go, asked.Url.AbsoluteUri)]
                    : [];
            default:
                return [];
        }
    }
}
