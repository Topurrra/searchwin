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

    /// Never indexed, whatever folder is chosen, even when it's one of them:
    /// keys, tokens and saved sign-ins, and browsers' profiles, this one's
    /// own included (every world's).
    public static readonly IReadOnlyList<string> Secrets =
    [
        ".ssh", ".aws", ".gnupg", ".azure", ".kube", ".docker", ".password-store", ".config/gcloud",
        "appdata/roaming/microsoft/credentials", "appdata/local/microsoft/credentials",
        "appdata/roaming/microsoft/protect", "appdata/roaming/microsoft/crypto",
        "appdata/roaming/microsoft/systemcertificates", "appdata/local/microsoft/vault",
        "appdata/local/google/chrome/user data", "appdata/local/microsoft/edge/user data",
        "appdata/local/bravesoftware/brave-browser/user data", "appdata/local/vivaldi/user data",
        "appdata/roaming/opera software", "appdata/roaming/mozilla/firefox/profiles",
        "appdata/local/search", "appdata/local/search (*",
        // Sign-ins other apps keep in files: chat apps' tokens (leveldb),
        // the GitHub CLI's, FTP and cloud clients', password managers' and
        // wallets'.
        "appdata/roaming/discord", "appdata/roaming/discordcanary", "appdata/roaming/discordptb",
        "appdata/roaming/slack", "appdata/local/slack", "appdata/roaming/microsoft/teams",
        "appdata/local/packages/msteams_8wekyb3d8bbwe", "appdata/local/microsoft/teams",
        "appdata/roaming/telegram desktop", "appdata/roaming/signal", "appdata/local/whatsapp",
        "appdata/roaming/github cli", ".config/gh", ".config/hub", "appdata/roaming/filezilla",
        "appdata/roaming/winscp", "appdata/roaming/rclone", ".config/rclone", "appdata/roaming/bitwarden",
        "appdata/local/1password", "appdata/roaming/keepassxc", "appdata/roaming/bitcoin",
        "appdata/roaming/electrum", "appdata/roaming/exodus", "appdata/roaming/ethereum",
        "appdata/local/chromium/user data", "appdata/local/arc/user data",
        "appdata/local/mozilla/firefox/profiles", "appdata/roaming/thunderbird/profiles",
        ".git-credentials", ".netrc", "_netrc", ".npmrc", ".pypirc", ".vault-token", ".terraform.d",
    ];

    /// `save_file_search_index_options {options}` for these folders. The
    /// same folders feed both indexes; the watcher keeps them current.
    /// `own` is the browser's own data folder, never indexed.
    public static JsonObject Options(IReadOnlyList<string> folders, bool contents, string? own = null)
    {
        JsonArray Folders() => [.. folders.Select(f => (JsonNode)f)];
        var roots = Roots(folders);
        // The engine skips anything with a folder whose name starts with a
        // dot anywhere in its path, the chosen folder's own parents too. A
        // folder chosen inside one needs that off, and then every folder's
        // dot-folders are skipped by name instead: below each chosen folder,
        // not above it.
        var hidden = roots.Any(r => r.Split('/').Any(p => p.StartsWith('.')));
        List<string> excludes = hidden ? HiddenRules(roots) : [];
        excludes.AddRange(Excludes(folders, own));
        return new JsonObject
        {
            ["roots"] = Folders(),
            ["filenameRoots"] = Folders(),
            ["includeHidden"] = hidden,
            ["indexContent"] = contents,
            ["contentIndexingEnabled"] = contents,
            ["maxContentKb"] = null,
            ["commitEvery"] = null,
            ["watcherEnabled"] = true,
            ["excludeFolders"] = new JsonArray([.. Once(excludes).Select(e => (JsonNode)e)]),
        };
    }

    /// With hidden folders on, each chosen folder's dot-folders (right below
    /// it and deeper) are skipped, and so is AppData, Windows' hidden folder,
    /// below a profile (or a folder of profiles), unless the folder was
    /// chosen inside AppData. A folder chosen inside another's skipped
    /// folder (the profile and its `.config`, or `AppData\Roaming\Notes`) is
    /// kept by a `!folder` rule: the engine lets a later rule win, so the
    /// keep undoes only the rules of the folders around it. Everything else
    /// stays out: the profile's other dot-folders and the rest of its
    /// AppData, the kept folder's own dot-folders (its rules come after its
    /// keep), and the default exclusions and secrets (listed after all of
    /// these).
    private static List<string> HiddenRules(List<string> roots)
    {
        var rules = new List<string>();
        // Outer folders first, so a folder's keep follows the rules of the
        // folders it sits in and comes before its own.
        foreach (var root in roots.OrderBy(r => r.Split('/').Length).ThenBy(r => r, StringComparer.Ordinal))
        {
            if (Skips(rules, root)) rules.Add("!" + root);
            rules.AddRange([root + "/.*", root + "/*/.*"]);
            if (!InAppData(root)) rules.AddRange([root + "/appdata", root + "/*/appdata"]);
        }
        return rules;
    }

    /// Whether `rules`, read as the engine reads them (the last one that
    /// matches wins; `!` keeps), skip `path`.
    private static bool Skips(List<string> rules, string path)
    {
        var skipped = false;
        foreach (var rule in rules)
        {
            var keep = rule.StartsWith('!');
            if (Matches(path, keep ? rule[1..] : rule)) skipped = !keep;
        }
        return skipped;
    }

    /// The exclusions, per chosen folder. The engine tests every folder of a
    /// path, the chosen one's own parents too, so a folder picked under Temp
    /// or a `build` folder would come back empty. Such an exclusion is
    /// narrowed to below each chosen folder (not dropped: a `build` inside
    /// another chosen folder is still skipped). The secrets and `own` always
    /// apply, even to a folder chosen inside them.
    public static List<string> Excludes(IReadOnlyList<string> folders, string? own = null)
    {
        var roots = Roots(folders);
        var list = new List<string>();
        foreach (var exclusion in DefaultExcludes)
        {
            if (!roots.Any(r => Within(r, exclusion)))
            {
                list.Add(exclusion);
                continue;
            }
            foreach (var root in roots)
            {
                // Right below the folder, and anywhere deeper: the excluded
                // folder itself and all it holds (a `*` pattern ending in a
                // name ends at a folder name, so `bin` isn't `binaries`).
                list.Add($"{root}/{exclusion}");
                list.Add($"{root}/*/{exclusion}");
            }
        }
        list.AddRange(Secrets);
        if (own != null && Normal(own) is { Length: > 0 } mine) list.Add(mine);
        return Once(list);
    }

    /// Each rule once, in the place of its last copy: the engine lets the
    /// last rule that matches decide.
    private static List<string> Once(List<string> rules)
    {
        var seen = new HashSet<string>(StringComparer.Ordinal);
        var once = new List<string>();
        for (var i = rules.Count - 1; i >= 0; i--)
            if (seen.Add(rules[i])) once.Add(rules[i]);
        once.Reverse();
        return once;
    }

    private static bool InAppData(string root) => root.Split('/').Contains("appdata", StringComparer.Ordinal);

    private static List<string> Roots(IReadOnlyList<string> folders) =>
        [.. folders.Select(Normal).Where(p => p.Length > 0).Distinct(StringComparer.Ordinal)];

    /// Whether the chosen folder itself, or a parent of it, is excluded.
    private static bool Within(string root, string exclusion) =>
        IsPathLike(exclusion) ? Matches(root, exclusion) : root.Split('/').Contains(exclusion, StringComparer.Ordinal);

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

    private static bool IsPathLike(string exclusion) =>
        exclusion.Contains('/') || exclusion.Contains(':') || exclusion.Contains('*');

    /// The engine's `exclusion_pattern_matches_path`: the pieces between
    /// `*`s in order, a last piece (no `*` after it) ending at a folder name.
    private static bool Matches(string path, string exclusion)
    {
        if (exclusion.Contains('*'))
        {
            var parts = exclusion.Split('*', StringSplitOptions.RemoveEmptyEntries);
            var rest = path.AsSpan();
            for (var i = 0; i < parts.Length; i++)
            {
                if (i == parts.Length - 1 && !exclusion.EndsWith('*'))
                {
                    for (var from = 0; ;)
                    {
                        var hit = rest[from..].IndexOf(parts[i], StringComparison.Ordinal);
                        if (hit < 0) return false;
                        var end = from + hit + parts[i].Length;
                        if (end == rest.Length || rest[end] == '/') return true;
                        from += hit + 1;
                    }
                }
                var at = rest.IndexOf(parts[i], StringComparison.Ordinal);
                if (at < 0) return false;
                rest = rest[(at + parts[i].Length)..];
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
