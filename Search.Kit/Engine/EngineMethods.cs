using System.Text.Json.Nodes;

namespace SearchKit.Engine;

/// What the browser needs the engine to answer. An engine that lacks one
/// is an old build (publish-aot.cmd once copied whatever lay in target\):
/// said in the log the moment it connects, rather than found out later as
/// a Settings button that does nothing.
public static class EngineMethods
{
    public static readonly IReadOnlyList<string> Required =
    [
        "clear_file_search_index", "save_file_search_index_options", "get_file_search_status",
        "list_folder_children", "get_clipboard_history",
    ];

    /// The required methods `engine.methods` (a list of names) doesn't have.
    public static List<string> Missing(JsonNode? methods)
    {
        var have = new HashSet<string>(StringComparer.Ordinal);
        if (methods is JsonArray list)
            foreach (var item in list)
                if (item is JsonValue value && value.TryGetValue<string>(out var name)) have.Add(name);
        return [.. Required.Where(name => !have.Contains(name))];
    }
}
