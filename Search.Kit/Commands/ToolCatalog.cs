using System.Text.Json.Nodes;

namespace SearchKit.Commands;

/// One tool page Search has (search://tools/<id>).
public sealed record ToolEntry(string Id, string Name, string Description, string Pack);

/// The tool pages, for the field: every tool is a command (`>image studio`),
/// and typing a tool's name offers it as a row. The list is the one the tools
/// index shows, written by the pages' build (Tools/catalog.js →
/// tools/catalog.json beside Search.exe), so what the field offers and what
/// the index lists can't disagree.
public static class ToolCatalog
{
    /// The build's catalog.json. Whatever isn't a tool (no id, no name) is
    /// skipped; a file that can't be read is no tools, not an error.
    public static IReadOnlyList<ToolEntry> Parse(string json)
    {
        try
        {
            if (JsonNode.Parse(json) is not JsonArray items) return [];
            var tools = new List<ToolEntry>(items.Count);
            foreach (var item in items)
            {
                if (item is not JsonObject tool) continue;
                var id = Text(tool, "id");
                var name = Text(tool, "name");
                if (id.Length == 0 || name.Length == 0) continue;
                tools.Add(new ToolEntry(id, name, Text(tool, "description"), Text(tool, "pack")));
            }
            return tools;
        }
        catch (System.Text.Json.JsonException)
        {
            return [];
        }
    }

    private static string Text(JsonObject item, string key) =>
        item[key] is JsonValue value && value.TryGetValue<string>(out var text) ? text.Trim() : "";

    /// Each tool as a command: its name is its words, Enter opens its page.
    public static void Register(CommandRegistry registry, IEnumerable<ToolEntry> tools, Action<string> open)
    {
        foreach (var tool in tools)
        {
            var id = tool.Id;
            registry.Add(new Command($"tools.{id}", tool.Name, Tier.Read, [tool.Name], Group: "Tools"), _ => open(id));
        }
    }

    /// The tool someone typing `typed` most likely wants, or none. Only a
    /// strong match: the tool's name, or the start of it, from three letters;
    /// or the start of one of its words, from four ("studio" → Image Studio).
    /// The shortest name wins a tie ("image" → Image Studio).
    public static ToolEntry? Offer(IReadOnlyList<ToolEntry> tools, string typed)
    {
        var wanted = string.Join(' ', typed.ToLowerInvariant().Split(' ', StringSplitOptions.RemoveEmptyEntries));
        if (wanted.Length < 3) return null;
        ToolEntry? best = null;
        var bestScore = 0;
        foreach (var tool in tools)
        {
            var name = tool.Name.ToLowerInvariant();
            var score = name == wanted ? 3
                : name.StartsWith(wanted, StringComparison.Ordinal) ? 2
                : wanted.Length >= 4 && Words(name).Any(word => word.StartsWith(wanted, StringComparison.Ordinal)) ? 1
                : 0;
            if (score > bestScore || (score > 0 && score == bestScore && tool.Name.Length < best!.Name.Length))
                (best, bestScore) = (tool, score);
        }
        return best;
    }

    private static IEnumerable<string> Words(string name) =>
        name.Split([' ', '/', '(', ')', '&', '-'], StringSplitOptions.RemoveEmptyEntries);
}
