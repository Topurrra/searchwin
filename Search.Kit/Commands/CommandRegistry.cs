namespace SearchKit.Commands;

/// Every command Search has, in one list, with what each one does.
///
/// The field (`>…`), voice, the agent and scripts all find and run commands
/// here, so a command is written once and behaves the same however it's
/// reached. Running one checks its tier first: an `AlwaysAsks` command
/// calls `ask`, which puts the question in front of a person — whoever made
/// the call — and does nothing unless they say yes.
public sealed class CommandRegistry
{
    private readonly Dictionary<string, (Command Command, Func<CommandCall, Task> Run)> commands =
        new(StringComparer.Ordinal);

    public IEnumerable<Command> All => commands.Values.Select(entry => entry.Command);

    public void Add(Command command, Func<CommandCall, Task> run)
    {
        if (!commands.TryAdd(command.Id, (command, run)))
            throw new InvalidOperationException($"a command called {command.Id} is already registered");
    }

    public void Add(Command command, Action<CommandCall> run) =>
        Add(command, call => { run(call); return Task.CompletedTask; });

    public Command? Get(string id) => commands.TryGetValue(id, out var entry) ? entry.Command : null;

    public enum Outcome { Ran, Declined, Unknown }

    /// Runs a command, asking first when its tier says to.
    public async Task<Outcome> RunAsync(string id, string argument, Source source, Func<Command, Task<bool>> ask)
    {
        if (!commands.TryGetValue(id, out var entry)) return Outcome.Unknown;
        if (entry.Command.Tier == Tier.AlwaysAsks && !await ask(entry.Command)) return Outcome.Declined;
        await entry.Run(new CommandCall(entry.Command, argument, source));
        return Outcome.Ran;
    }

    /// The commands a `>query` could mean, best first, with whatever of the
    /// query is left over as the argument ("space Work" → "new space" with
    /// "Work" if the command's words were "new space").
    public IReadOnlyList<(Command Command, string Argument)> Find(string query, int limit = 8)
    {
        // Matched lower-case, but the argument keeps what was typed (a space
        // called "Work" stays Work).
        var typed = string.Join(' ', query.Split(' ', StringSplitOptions.RemoveEmptyEntries));
        var wanted = typed.ToLowerInvariant();
        if (wanted.Length == 0)
            return [.. commands.Values.Select(e => (e.Command, "")).OrderBy(m => m.Command.Title).Take(limit)];

        var ranked = new List<(int Score, Command Command, string Argument)>();
        foreach (var (command, _) in commands.Values)
        {
            var best = 0;
            var argument = "";
            foreach (var candidate in command.Words.Append(command.Title))
            {
                var (score, rest) = Score(Normal(candidate), wanted);
                if (score > best) (best, argument) = (score, rest.Length == 0 ? "" : typed[^rest.Length..]);
            }
            if (best > 0) ranked.Add((best, command, argument));
        }
        return [.. ranked
            .OrderByDescending(r => r.Score)
            .ThenBy(r => r.Command.Title.Length)
            .Take(limit)
            .Select(r => (r.Command, r.Argument))];
    }

    private static string Normal(string text) =>
        string.Join(' ', text.ToLowerInvariant().Split(' ', StringSplitOptions.RemoveEmptyEntries));

    /// How well `wanted` fits `words`. Exact beats "the words, then an
    /// argument" beats a prefix of the words beats their initials beats
    /// appearing somewhere in them.
    private static (int Score, string Left) Score(string words, string wanted)
    {
        if (words == wanted) return (100, "");
        if (wanted.StartsWith(words + " ", StringComparison.Ordinal)) return (90, wanted[(words.Length + 1)..]);
        if (words.StartsWith(wanted, StringComparison.Ordinal)) return (80, "");
        var initials = new string([.. words.Split(' ').Where(w => w.Length > 0).Select(w => w[0])]);
        if (initials.Length > 1 && initials == wanted.Replace(" ", "")) return (60, "");
        if (words.Split(' ').Any(w => w.StartsWith(wanted, StringComparison.Ordinal))) return (50, "");
        if (words.Contains(wanted, StringComparison.Ordinal)) return (30, "");
        return (0, "");
    }
}
