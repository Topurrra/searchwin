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

    private AbpPattern(Token[] tokens, bool startAnchor, bool endAnchor)
    {
        this.tokens = tokens;
        this.startAnchor = startAnchor;
        this.endAnchor = endAnchor;
    }

    /// `text` is already lower-cased by the caller (matching is always
    /// case-insensitive here — `$match-case` isn't one of the options this
    /// engine acts on, so a rule that needs it is skipped upstream instead).
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
    public bool IsMatch(string haystack, int from) => tokens.Length == 0 || Search(haystack, 0, from);

    /// Nothing after a `||domain` but at most its closing `^`: the rule is
    /// the domain and nothing else, so a set of names can stand in for it.
    /// After a host there is always a separator (`/`, `:`, `?`) or the end,
    /// so the `^` can't fail once the host has matched.
    public bool IsDomainOnly =>
        !endAnchor && (tokens.Length == 0 || (tokens.Length == 1 && tokens[0].Kind == Kind.Separator));

    /// Every token this pattern's literal pieces contain that's at least
    /// three plain letters/digits long, longest first — `FilterList` picks
    /// the first one as the index key for a non-domain-anchored rule, so a
    /// request only has to test the handful of rules that share a token it
    /// actually contains, not the whole list.
    public IEnumerable<string> IndexTokens()
    {
        foreach (var token in tokens)
        {
            if (token.Kind != Kind.Literal) continue;
            foreach (var run in AlnumRuns(token.Text))
                if (run.Length >= 3) yield return run;
        }
    }

    /// Maximal runs of ASCII letters/digits in `text` — the same tokenizer
    /// run over a rule's literal pieces at compile time and over a request
    /// URL at match time, so the two sides agree on what a "token" is.
    public static IEnumerable<string> AlnumRuns(string text)
    {
        var start = -1;
        for (var i = 0; i <= text.Length; i++)
        {
            var isAlnum = i < text.Length && char.IsAsciiLetterOrDigit(text[i]);
            if (isAlnum) { if (start < 0) start = i; }
            else { if (start >= 0) yield return text[start..i]; start = -1; }
        }
    }

    // A short backtracking search: patterns are almost always well under a
    // hundred characters, and this is only reached for the minority of
    // rules that aren't a plain domain match with nothing after it — real
    // ad-block engines do the same thing once their own token index has cut
    // the candidates down to a handful.
    private bool Search(string haystack, int tokenIndex, int pos)
    {
        if (tokenIndex == tokens.Length) return !endAnchor || pos == haystack.Length;
        var token = tokens[tokenIndex];
        switch (token.Kind)
        {
            case Kind.Literal:
                if (tokenIndex == 0 && startAnchor)
                {
                    if (pos + token.Text.Length > haystack.Length) return false;
                    return string.CompareOrdinal(haystack, pos, token.Text, 0, token.Text.Length) == 0
                        && Search(haystack, tokenIndex + 1, pos + token.Text.Length);
                }
                for (var at = haystack.IndexOf(token.Text, pos, StringComparison.Ordinal); at >= 0;
                     at = haystack.IndexOf(token.Text, at + 1, StringComparison.Ordinal))
                {
                    if (Search(haystack, tokenIndex + 1, at + token.Text.Length)) return true;
                }
                return false;

            case Kind.Wildcard:
                for (var p = pos; p <= haystack.Length; p++)
                    if (Search(haystack, tokenIndex + 1, p)) return true;
                return false;

            default: // Separator: exactly one separator character, or end of string.
                if (pos == haystack.Length) return Search(haystack, tokenIndex + 1, pos);
                return IsSeparator(haystack[pos]) && Search(haystack, tokenIndex + 1, pos + 1);
        }
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
