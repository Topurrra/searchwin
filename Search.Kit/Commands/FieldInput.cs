namespace SearchKit.Commands;

/// What was typed in the field, before anyone decides whether it's an address.
public abstract record FieldInput(string Typed)
{
    /// `!yt cats` or `cats !yt`: the bang and the words.
    public sealed record ToBang(string Typed, Bang Bang, string Query) : FieldInput(Typed);

    /// `>close others`: a command, found by what follows the `>`.
    public sealed record ToCommand(string Typed, string Query) : FieldInput(Typed);

    /// `?what does this page say about pricing`: a question for the AI.
    public sealed record ToAsk(string Typed, string Question) : FieldInput(Typed);

    /// Anything else: an address, or words to search for. `Text` is what to
    /// use (a leading `\` escape removed).
    public sealed record Plain(string Typed, string Text) : FieldInput(Typed);

    /// Reads what was typed. A leading `\` means "take the rest literally":
    /// `\!important` searches for "!important". An unknown bang is plain text.
    public static FieldInput Read(string typed, Bangs bangs)
    {
        var text = typed.Trim();
        if (text.StartsWith('\\')) return new Plain(typed, text[1..].TrimStart());
        if (text.Length > 1 && text[0] == '>') return new ToCommand(typed, text[1..].Trim());
        if (text.Length > 1 && text[0] == '?') return new ToAsk(typed, text[1..].Trim());

        if (text.Length > 1 && text[0] == '!')
        {
            var space = text.IndexOf(' ');
            var trigger = space < 0 ? text[1..] : text[1..space];
            if (bangs.Find(trigger) is { } bang)
                return new ToBang(typed, bang, space < 0 ? "" : text[(space + 1)..].Trim());
        }

        // DuckDuckGo's other spelling: the bang at the end.
        var last = text.LastIndexOf(" !", StringComparison.Ordinal);
        if (last > 0 && text.IndexOf(' ', last + 2) < 0 && bangs.Find(text[(last + 2)..]) is { } trailing)
            return new ToBang(typed, trailing, text[..last].Trim());

        return new Plain(typed, text);
    }
}
