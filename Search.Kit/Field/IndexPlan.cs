using System.Text.Json.Nodes;

namespace SearchKit.Field;

/// The engine's file index as Settings › Search sets it up: only the
/// folders the person chose (none until they choose one), names always,
/// what's inside the files when they want that too.
public static class IndexPlan
{
    /// The engine's own default exclusions (Engine/src/commands/search.rs,
    /// `DEFAULT_EXCLUDE_FOLDERS`). A plain name matches any folder of that
    /// name anywhere in a path; one with a `/` matches that run of folders.
    public static readonly IReadOnlyList<string> DefaultExcludes =
    [
        ".git", ".hg", ".svn", ".cache", ".gradle", ".idea", ".mypy_cache", ".next", ".pytest_cache", ".ruff_cache",
        ".svelte-kit", ".venv", ".vscode", "$recycle.bin", "$windows.~bt", "$windows.~ws", "__pycache__",
        "appdata/local", "appdata/locallow", "appdata/local/crashdumps", "appdata/local/temp", "bin", "build",
        "com.keepitlocal.app", "config.msi", "dist", "file-search-index", "inetpub/logs", "microsoft/windows/wer",
        "node_modules", "obj", "programdata/microsoft/windows/wer", "programdata/package cache", "recovery",
        "keepitlocal-cache", "system volume information", "target", "temp", "tmp", "users/*/appdata/local/temp",
        "venv", "windows/debug", "windows/logs", "windows/panther", "windows/prefetch",
        "windows/softwaredistribution/download", "windows/temp", "windows/winsxs/temp",
    ];

    /// `save_file_search_index_options {options}` for these folders. The
    /// same folders feed both indexes; the watcher keeps them current.
    public static JsonObject Options(IReadOnlyList<string> folders, bool contents)
    {
        JsonArray Folders() => [.. folders.Select(f => (JsonNode)f)];
        return new JsonObject
        {
            ["roots"] = Folders(),
            ["filenameRoots"] = Folders(),
            // The engine skips anything under a folder whose name starts with
            // a dot; a folder chosen inside one is still wanted.
            ["includeHidden"] = folders.Any(f => Parts(f).Any(p => p.StartsWith('.'))),
            ["indexContent"] = contents,
            ["contentIndexingEnabled"] = contents,
            ["maxContentKb"] = null,
            ["commitEvery"] = null,
            ["watcherEnabled"] = true,
            ["excludeFolders"] = new JsonArray([.. Excludes(folders).Select(e => (JsonNode)e)]),
        };
    }

    /// The default exclusions, less the ones a chosen folder sits inside: the
    /// engine tests every folder of a path, the chosen one's own parents too,
    /// so a folder picked under Temp or a `build` folder would come back
    /// empty. Below the chosen folders the rest still apply.
    public static List<string> Excludes(IReadOnlyList<string> folders)
    {
        var paths = folders.Select(Normal).Where(p => p.Length > 0).ToList();
        var names = new HashSet<string>(paths.SelectMany(p => p.Split('/')), StringComparer.Ordinal);
        return [.. DefaultExcludes.Where(e => IsPathLike(e) ? !paths.Any(p => Matches(p, e)) : !names.Contains(e))];
    }

    /// Whether `path` is inside one of `folders`, so a result left in the
    /// index from a folder since removed is never shown.
    public static bool Covers(IReadOnlyList<string> folders, string path)
    {
        var file = Normal(path);
        foreach (var folder in folders)
        {
            var root = Normal(folder);
            if (root.Length == 0) continue;
            if (file == root || file.StartsWith(root.EndsWith('/') ? root : root + "/", StringComparison.Ordinal)) return true;
        }
        return false;
    }

    private static string Normal(string path) =>
        path.Trim().Replace('\\', '/').TrimEnd('/').ToLowerInvariant();

    private static IEnumerable<string> Parts(string path) =>
        path.Replace('\\', '/').Split('/', StringSplitOptions.RemoveEmptyEntries);

    private static bool IsPathLike(string exclusion) =>
        exclusion.Contains('/') || exclusion.Contains(':') || exclusion.Contains('*');

    /// The engine's `exclusion_pattern_matches_path`.
    private static bool Matches(string path, string exclusion)
    {
        if (exclusion.Contains('*'))
        {
            var rest = path.AsSpan();
            foreach (var part in exclusion.Split('*', StringSplitOptions.RemoveEmptyEntries))
            {
                var at = rest.IndexOf(part, StringComparison.Ordinal);
                if (at < 0) return false;
                rest = rest[(at + part.Length)..];
            }
            return true;
        }
        return path == exclusion || path.StartsWith(exclusion + "/", StringComparison.Ordinal)
            || path.Contains("/" + exclusion + "/", StringComparison.Ordinal) || path.EndsWith("/" + exclusion, StringComparison.Ordinal);
    }
}

/// What `get_file_search_status` says, in the few numbers Settings shows.
/// `Files` and `Building` are the index of what's inside files; `Names` and
/// `NamesBuilding` the index of names. The two are built separately.
public sealed record IndexStatus(long Files, long Names, bool Building, bool NamesBuilding, long? LastIndexedMs, string? Error)
{
    public static IndexStatus Read(JsonNode? status)
    {
        long? last = Nodes.Long(status, "lastIndexedAtMs") is var ms and > 0 ? ms : null;
        var error = Nodes.Str(status, "lastError");
        return new IndexStatus(Nodes.Long(status, "indexedFiles"), Nodes.Long(status, "filenameIndexedFiles"),
            Nodes.Bool(status, "indexing"), Nodes.Bool(status, "filenameIndexing"), last, error.Length > 0 ? error : null);
    }

    public bool Indexing => Building || NamesBuilding;

    /// "1,204 files" — the larger of the two indexes' counts.
    public long Count => Math.Max(Files, Names);
}
