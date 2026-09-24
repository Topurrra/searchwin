namespace SearchKit.Commands;

/// How much a command may do without asking. The same tier holds wherever the
/// command comes from — typed in the field, spoken, run by the agent or a
/// script — so no way of reaching a command can skip its question.
public enum Tier
{
    /// Looks, never changes anything: list tabs, show clipboard history.
    Read,
    /// Changes the browser, undoably: close other tabs, new space.
    Act,
    /// Hard to take back or reaches outside: delete, shred, submit, send.
    /// Always asks a person, on screen, first.
    AlwaysAsks,
}

/// Where a call came from. Scripts and the agent can't answer a question
/// themselves; the browser shows it and a person answers.
public enum Source
{
    Field,
    Voice,
    Agent,
    Script,
}

/// One thing Search can do, however it's asked for.
/// `Words` are what you type after `>` ("close others"); `Phrases` what you
/// say. Both are matched loosely; the title is matched too.
public sealed record Command(
    string Id,
    string Title,
    Tier Tier,
    IReadOnlyList<string> Words,
    IReadOnlyList<string>? Phrases = null,
    string? Group = null,
    string? Keys = null)
{
    public IReadOnlyList<string> Phrases { get; init; } = Phrases ?? [];
}

/// A call: the command, whatever followed its words, and who asked.
public sealed record CommandCall(Command Command, string Argument, Source Source);
