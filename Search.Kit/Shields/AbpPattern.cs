using System.Text;

namespace SearchKit.Shields;

/// One Adblock-Plus-syntax URL pattern, compiled into literal pieces plus
/// the two wildcards the syntax defines: `*` (anything, including nothing)
/// and `^` ("separator" — one character that isn't a letter, digit, or one
/// of `_.%-`, or the end of the string). A leading/trailing `|` anchors that
/// end to the start or end of the match; for a `||domain` rule, the domain
/// itself is matched separately (`NetworkRule.AnchorDomain`, against the
/// request's actual host, not text), and what's left after it is parsed
/// here with `forceStartAnchor: true`, since it has to follow the domain
/// immediately.
internal sealed class AbpPattern
{
    private enum Kind { Literal, Wildcard, Separator }
    private readonly record struct Token(Kind Kind, string Text);

    private readonly Token[] tokens;
    private readonly bool startAnchor;
    private readonly bool endAnchor;

    // The tokens between one `*` and the next, as [start, end) ranges into
    // `tokens` — one more piece than there are wildcards, some of them empty.
    private readonly (int Start, int End)[] pieces;

    private AbpPattern(Token[] tokens, bool startAnchor, bool endAnchor)
    {
        this.tokens = tokens;
        this.startAnchor = startAnchor;
        this.endAnchor = endAnchor;
        var list = new List<(int, int)>();
        var from = 0;
        for (var i = 0; i <= tokens.Length; i++)
        {
            if (i < tokens.Length && tokens[i].Kind != Kind.Wildcard) continue;
            list.Add((from, i));
            from = i + 1;
        }
        pieces = [.. list];
    }

    /// The shortest word, on either side, that the index goes by: a request
    /// looks up every run of letters and digits in its address at least this
    /// long, and a rule is filed under one of its own.
    public const int MinToken = 2;

    /// `text` is already lower-cased by the caller (matching is always
    /// case-insensitive here — `$match-case` isn't one of the options this
    /// engine acts on, so a block that needs it is skipped upstream, and an
    /// exception carrying it simply allows a little more).
    public static AbpPattern Parse(string text, bool forceStartAnchor = false)
    {
        var startAnchor = forceStartAnchor || text.StartsWith('|');
        var bodyStart = !forceStartAnchor && text.StartsWith('|') ? 1 : 0;
        var endAnchor = text.Length > bodyStart && text.EndsWith('|');
        var bodyEnd = endAnchor ? text.Length - 1 : text.Length;
        var body = bodyEnd > bodyStart ? text[bodyStart..bodyEnd] : "";

        var tokens = new List<Token>();
        var literal = new StringBuilder();
        void Flush() { if (literal.Length > 0) { tokens.Add(new Token(Kind.Literal, literal.ToString())); literal.Clear(); } }
        foreach (var ch in body)
        {
            if (ch == '*') { Flush(); tokens.Add(new Token(Kind.Wildcard, "")); }
            else if (ch == '^') { Flush(); tokens.Add(new Token(Kind.Separator, "")); }
            else literal.Append(ch);
        }
        Flush();
        return new AbpPattern([.. tokens], startAnchor, endAnchor);
    }

    private static bool IsSeparator(char c) => !(char.IsAsciiLetterOrDigit(c) || c is '_' or '.' or '%' or '-');

    /// `from` is where the match is allowed to start counting from — 0 for a
    /// plain pattern, or the index right after the request's host for a
    /// domain-anchored one. A start anchor means the match must begin
    /// exactly there, not merely somewhere at or after it.
    ///
    /// No backtracking: each piece between two `*` is fixed in length (a
    /// `^` is one character, or none at the very end), so the earliest place
    /// a piece fits also ends earliest, and whatever follows a `*` can only
    /// do better from an earlier place. Each piece is looked for once, left
    /// to right, which keeps a rule with many wildcards linear in the
    /// address instead of growing with a power of its length. Only a piece
    /// held at an end by `|` is looked for there instead.
    ///
    /// `limit`: a piece looked for is only looked for starting at or before
    /// it, so a rule reads that much of a very long address and no more.
    /// A piece held at the end is still checked at the real end (it takes
    /// only its own few characters), so a match found is always a real one;
    /// what is missed is only a match that starts further on.
    public bool IsMatch(string haystack, int from, int limit = int.MaxValue)
    {
        if (tokens.Length == 0) return true;
        var pos = from;
        for (var i = 0; i < pieces.Length; i++)
        {
            var (start, end) = pieces[i];
            var heldStart = i == 0 && startAnchor;
            var heldEnd = i == pieces.Length - 1 && endAnchor;
            if (heldEnd)
                return heldStart ? MatchAt(start, end, haystack, pos) == haystack.Length : EndsWith(start, end, haystack, pos);
            pos = heldStart ? MatchAt(start, end, haystack, pos) : Find(start, end, haystack, pos, limit);
            if (pos < 0) return false;
        }
        return true;
    }

    /// Nothing after a `||domain` but at most its closing `^`: the rule is
    /// the domain and nothing else, so a set of names can stand in for it.
    /// After a host there is always a separator (`/`, `:`, `?`) or the end,
    /// so the `^` can't fail once the host has matched.
    public bool IsDomainOnly =>
        !endAnchor && (tokens.Length == 0 || (tokens.Length == 1 && tokens[0].Kind == Kind.Separator));

    /// Every word (a run of at least `MinToken` letters/digits) this pattern
    /// can only match as a *whole* word of the address — `FilterList` files a
    /// non-domain-anchored rule under the rarest of them, so a request only
    /// tests the rules that share a word it actually contains.
    ///
    /// Whole means held on both sides: by a character in the pattern that
    /// isn't a letter or digit, by a `^`, or by a `|` at that end. A request
    /// only looks up whole runs of its address, so a word the pattern leaves
    /// open — at an unanchored end (`banner` also matches "banners"), or
    /// next to a `*` (`-ad-*banner` also matches "bigbanner") — would never
    /// be looked up when it's part of a longer one, and the rule would never
    /// be tried. A rule with no whole word goes in the bucket every request
    /// tries. (After a `||domain`, what's left starts with `/`, `^` or `*`,
    /// so the anchor there never holds a word against the host.)
    public IEnumerable<string> IndexTokens()
    {
        for (var i = 0; i < tokens.Length; i++)
        {
            if (tokens[i].Kind != Kind.Literal) continue;
            var text = tokens[i].Text;
            var openLeft = i == 0 ? !startAnchor : tokens[i - 1].Kind == Kind.Wildcard;
            var openRight = i == tokens.Length - 1 ? !endAnchor : tokens[i + 1].Kind == Kind.Wildcard;
            var start = -1;
            for (var j = 0; j <= text.Length; j++)
            {
                if (j < text.Length && char.IsAsciiLetterOrDigit(text[j])) { if (start < 0) start = j; continue; }
                if (start < 0) continue;
                var whole = !(start == 0 && openLeft) && !(j == text.Length && openRight);
                if (whole && j - start >= MinToken) yield return text[start..j];
                start = -1;
            }
        }
    }

    /// Where the piece `tokens[start..end]` ends if it begins exactly at
    /// `pos`, or -1. A `^` takes one separator character, or nothing at the
    /// end of the address.
    private int MatchAt(int start, int end, string haystack, int pos)
    {
        for (var i = start; i < end; i++)
        {
            var token = tokens[i];
            if (token.Kind == Kind.Literal)
            {
                if (pos + token.Text.Length > haystack.Length
                    || string.CompareOrdinal(haystack, pos, token.Text, 0, token.Text.Length) != 0) return -1;
                pos += token.Text.Length;
            }
            else if (pos < haystack.Length)
            {
                if (!IsSeparator(haystack[pos])) return -1;
                pos++;
            }
        }
        return pos;
    }

    /// Where the piece ends at its earliest place at or after `from` and at
    /// or before `limit`, or -1.
    private int Find(int start, int end, string haystack, int from, int limit)
    {
        if (start == end) return from;
        var last = Math.Min(limit, haystack.Length);
        if (from > last) return -1;
        if (tokens[start].Kind == Kind.Literal)
        {
            var text = tokens[start].Text;
            // Where the text may end: it may run past `last`, not start past it.
            var stop = (int)Math.Min(haystack.Length, (long)last + text.Length);
            for (var at = haystack.IndexOf(text, from, stop - from, StringComparison.Ordinal); at >= 0;
                 at = at < last ? haystack.IndexOf(text, at + 1, stop - at - 1, StringComparison.Ordinal) : -1)
            {
                var found = MatchAt(start, end, haystack, at);
                if (found >= 0) return found;
            }
            return -1;
        }
        for (var at = from; at <= last; at++)
        {
            var found = MatchAt(start, end, haystack, at);
            if (found >= 0) return found;
        }
        return -1;
    }

    /// Whether the piece fits somewhere at or after `from` and ends exactly
    /// at the end of the address. It takes a fixed number of characters,
    /// less any `^` that falls on the end, so only its last few starting
    /// places can work.
    private bool EndsWith(int start, int end, string haystack, int from)
    {
        var longest = 0;
        for (var i = start; i < end; i++) longest += tokens[i].Kind == Kind.Literal ? tokens[i].Text.Length : 1;
        for (var at = Math.Max(from, haystack.Length - longest); at <= haystack.Length; at++)
            if (MatchAt(start, end, haystack, at) == haystack.Length) return true;
        return false;
    }

    internal void WriteTo(BinaryWriter w)
    {
        w.Write(startAnchor);
        w.Write(endAnchor);
        w.Write(tokens.Length);
        foreach (var t in tokens)
        {
            w.Write((byte)t.Kind);
            w.Write(t.Text);
        }
    }

    internal static AbpPattern ReadFrom(BinaryReader r)
    {
        var startAnchor = r.ReadBoolean();
        var endAnchor = r.ReadBoolean();
        var count = r.ReadInt32();
        var tokens = new Token[count];
        for (var i = 0; i < count; i++)
            tokens[i] = new Token((Kind)r.ReadByte(), r.ReadString());
        return new AbpPattern(tokens, startAnchor, endAnchor);
    }
}
