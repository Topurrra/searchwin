using System.Text.Json.Nodes;
using SearchKit.Field;

namespace SearchKit.Tests.Field;

/// IndexPlan's options checked the way the engine reads them: exclusions per
/// chosen folder, hidden folders, and what is never indexed at all.
public class IndexPlanRootTests
{
    /// The engine's own test (search.rs `should_include_path`, less the
    /// system folders): exclusions normalised as `normalize_unique_tokens`
    /// does, then, the last matching one deciding (`!` keeps), a path-like
    /// one against the whole path and a name against every folder of it;
    /// and dot-folders unless hidden ones are on.
    private static bool Skipped(JsonObject options, string path)
    {
        var normal = path.Replace('\\', '/').ToLowerInvariant().TrimEnd('/');
        var skipped = false;
        foreach (var node in options["excludeFolders"]!.AsArray())
        {
            var excluded = node!.GetValue<string>().Trim().Trim('"').Trim('\'').ToLowerInvariant().Replace('\\', '/').Trim('/').Trim();
            var keep = excluded.StartsWith('!');
            if (keep) excluded = excluded[1..];
            if (excluded.Length == 0) continue;
            var matches = excluded.Contains('/') || excluded.Contains(':') || excluded.Contains('*')
                ? EngineMatches(normal, excluded)
                : normal.Split('/').Contains(excluded);
            if (matches) skipped = !keep;
        }
        return skipped || !options["includeHidden"]!.GetValue<bool>() && normal.Split('/').Any(p => p.StartsWith('.'));
    }

    /// search.rs `exclusion_pattern_matches_path`: pieces between `*`s in
    /// order, a last piece with no `*` after it ending at a folder name.
    private static bool EngineMatches(string path, string excluded)
    {
        if (excluded.Contains('*'))
        {
            var parts = excluded.Split('*', StringSplitOptions.RemoveEmptyEntries);
            var rest = path;
            for (var i = 0; i < parts.Length; i++)
            {
                if (i == parts.Length - 1 && !excluded.EndsWith('*'))
                {
                    for (var from = 0; ;)
                    {
                        var hit = rest.IndexOf(parts[i], from, StringComparison.Ordinal);
                        if (hit < 0) return false;
                        var end = hit + parts[i].Length;
                        if (end == rest.Length || rest[end] == '/') return true;
                        from = hit + 1;
                    }
                }
                var at = rest.IndexOf(parts[i], StringComparison.Ordinal);
                if (at < 0) return false;
                rest = rest[(at + parts[i].Length)..];
            }
            return true;
        }
        return path == excluded || path.StartsWith(excluded + "/", StringComparison.Ordinal)
            || path.Contains("/" + excluded + "/", StringComparison.Ordinal) || path.EndsWith("/" + excluded, StringComparison.Ordinal);
    }

    [Fact]
    public void An_excluded_name_a_folder_sits_in_still_applies_below_every_folder()
    {
        var options = IndexPlan.Options([@"C:\Temp\search-test", @"D:\Work"], contents: true);
        Assert.False(Skipped(options, @"C:\Temp\search-test\a.txt"));
        Assert.False(Skipped(options, @"C:\Temp\search-test\deep\b.txt"));
        Assert.True(Skipped(options, @"C:\Temp\search-test\temp\a.txt"));
        Assert.True(Skipped(options, @"C:\Temp\search-test\x\temp\a.txt"));
        // The excluded folder itself too (no row for it), not one it begins.
        Assert.True(Skipped(options, @"C:\Temp\search-test\x\temp"));
        Assert.False(Skipped(options, @"C:\Temp\search-test\x\temperature"));
        Assert.False(Skipped(options, @"C:\Temp\search-test\x\temperature\a.txt"));
        // The other folder keeps every exclusion.
        Assert.True(Skipped(options, @"D:\Work\temp\a.txt"));
        Assert.True(Skipped(options, @"D:\Work\app\temp\a.txt"));
        Assert.True(Skipped(options, @"D:\Work\app\node_modules\x\a.js"));
        Assert.False(Skipped(options, @"D:\Work\temperature\a.txt"));
        Assert.False(Skipped(options, @"D:\Work\app\temperature\a.txt"));
        Assert.DoesNotContain("temp", options["excludeFolders"]!.AsArray().Select(n => n!.GetValue<string>()));

        var build = IndexPlan.Options([@"C:\src\build\docs", @"C:\src\app"], contents: false);
        Assert.False(Skipped(build, @"C:\src\build\docs\readme.md"));
        Assert.True(Skipped(build, @"C:\src\app\build\out.txt"));
        Assert.True(Skipped(build, @"C:\src\build\docs\build\out.txt"));
        Assert.True(Skipped(build, @"C:\src\build\docs\sub\build"));
        Assert.False(Skipped(build, @"C:\src\build\docs\sub\builder\notes.md"));

        // A folder chosen under AppData\Local; LocalLow and Temp still out.
        var local = IndexPlan.Options([@"C:\Users\me\AppData\Local\Notes", @"D:\Work"], contents: false);
        Assert.False(Skipped(local, @"C:\Users\me\AppData\Local\Notes\a.md"));
        Assert.True(Skipped(local, @"C:\Users\me\AppData\LocalLow\x\a.md"));
        Assert.True(Skipped(local, @"D:\Work\AppData\Local\x.md"));
    }

    [Fact]
    public void Hidden_folders_are_skipped_below_every_folder_even_one_inside_a_dot_folder()
    {
        var options = IndexPlan.Options([@"C:\Users\me\.notes\work", @"D:\Work"], contents: true);
        Assert.True(options["includeHidden"]!.GetValue<bool>());
        Assert.False(Skipped(options, @"C:\Users\me\.notes\work\plan.md"));
        Assert.True(Skipped(options, @"C:\Users\me\.notes\work\.private\plan.md"));
        Assert.True(Skipped(options, @"C:\Users\me\.notes\work\a\.hidden\plan.md"));
        Assert.True(Skipped(options, @"D:\Work\.env"));
        Assert.True(Skipped(options, @"D:\Work\app\.config\x.json"));
        Assert.False(Skipped(options, @"D:\Work\app\notes.md"));

        var plain = IndexPlan.Options([@"D:\Work"], contents: true);
        Assert.False(plain["includeHidden"]!.GetValue<bool>());
        Assert.True(Skipped(plain, @"D:\Work\.env"));
    }

    public static TheoryData<string> SecretPaths =>
    [
        @"C:\Users\me\.ssh\id_ed25519",
        @"C:\Users\me\.aws\credentials",
        @"C:\Users\me\.gnupg\private-keys-v1.d\x.key",
        @"C:\Users\me\.azure\msal_token_cache.json",
        @"C:\Users\me\.kube\config",
        @"C:\Users\me\.docker\config.json",
        @"C:\Users\me\AppData\Roaming\Microsoft\Credentials\ABC",
        @"C:\Users\me\AppData\Local\Microsoft\Credentials\ABC",
        @"C:\Users\me\AppData\Roaming\Microsoft\Protect\S-1\key",
        @"C:\Users\me\AppData\Local\Google\Chrome\User Data\Default\Login Data",
        @"C:\Users\me\AppData\Local\Search\passwords.json",
        @"C:\Users\me\AppData\Local\Search (test)\settings.json",
        @"C:\Data\Mine\WebView\Cookies",
        @"C:\Users\me\AppData\Roaming\discord\Local Storage\leveldb\000003.log",
        @"C:\Users\me\AppData\Roaming\Slack\Local Storage\leveldb\000003.log",
        @"C:\Users\me\AppData\Roaming\Microsoft\Teams\Local Storage\leveldb\x.ldb",
        @"C:\Users\me\AppData\Local\Packages\MSTeams_8wekyb3d8bbwe\LocalCache\x.json",
        @"C:\Users\me\AppData\Roaming\GitHub CLI\hosts.yml",
        @"C:\Users\me\.config\gh\hosts.yml",
        @"C:\Users\me\AppData\Roaming\FileZilla\sitemanager.xml",
        @"C:\Users\me\AppData\Roaming\Telegram Desktop\tdata\key_datas",
        @"C:\Users\me\.git-credentials",
    ];

    [Fact]
    public void Hidden_folders_on_skip_AppData_below_a_profile()
    {
        // A dot-folder chosen turns hidden folders on for every folder: the
        // profile's AppData (Windows-hidden, not dotted) must not come with it.
        var options = IndexPlan.Options([@"C:\Users\me", @"C:\Users\me\.config"], contents: true);
        Assert.True(options["includeHidden"]!.GetValue<bool>());
        Assert.Contains("c:/users/me/appdata", options["excludeFolders"]!.AsArray().Select(n => n!.GetValue<string>()));
        Assert.True(Skipped(options, @"C:\Users\me\AppData\Roaming\Code\User\settings.json"));
        Assert.True(Skipped(options, @"C:\Users\me\AppData\Local\Packages\x\y.dat"));
        Assert.False(Skipped(options, @"C:\Users\me\Documents\cv.pdf"));

        // A folder of profiles: every profile's AppData.
        var users = IndexPlan.Options([@"C:\Users", @"D:\.notes"], contents: true);
        Assert.True(Skipped(users, @"C:\Users\me\AppData\Roaming\x.txt"));
        Assert.False(Skipped(users, @"C:\Users\me\Documents\cv.pdf"));
    }

    [Fact]
    public void A_folder_chosen_inside_another_ones_dot_folder_is_indexed_and_only_it()
    {
        var options = IndexPlan.Options([@"C:\Users\me", @"C:\Users\me\.config"], contents: true);
        Assert.False(Skipped(options, @"C:\Users\me\.config"));
        Assert.False(Skipped(options, @"C:\Users\me\.config\app\settings.json"));
        // The profile's other dot-folders, and the chosen one's own, stay out.
        Assert.True(Skipped(options, @"C:\Users\me\.cargo\registry\x.rs"));
        Assert.True(Skipped(options, @"C:\Users\me\projects\.venv\x.py"));
        Assert.True(Skipped(options, @"C:\Users\me\.config\.hidden\x"));
        Assert.True(Skipped(options, @"C:\Users\me\.config\app\.cache\x"));
        // Exclusions and secrets still apply inside it.
        Assert.True(Skipped(options, @"C:\Users\me\.config\app\node_modules\x.js"));
        Assert.True(Skipped(options, @"C:\Users\me\.config\gh\hosts.yml"));
        Assert.False(Skipped(options, @"C:\Users\me\Documents\cv.pdf"));
    }

    [Fact]
    public void A_folder_chosen_inside_AppData_is_still_indexed()
    {
        var options = IndexPlan.Options([@"C:\Users\me", @"C:\Users\me\AppData\Roaming\Notes", @"D:\.notes"], contents: true);
        Assert.True(options["includeHidden"]!.GetValue<bool>());
        Assert.False(Skipped(options, @"C:\Users\me\AppData\Roaming\Notes\plan.md"));
        Assert.True(Skipped(options, @"C:\Users\me\AppData\Local\Packages\x\y.dat"));
        // Only that folder: the rest of AppData stays out.
        Assert.True(Skipped(options, @"C:\Users\me\AppData\Roaming\Code\User\settings.json"));
        Assert.False(Skipped(options, @"C:\Users\me\Documents\cv.pdf"));
        // Chosen inside AppData itself: no AppData exclusion of its own.
        var inside = IndexPlan.Options([@"C:\Users\me\AppData\Roaming\.tool"], contents: true);
        Assert.False(Skipped(inside, @"C:\Users\me\AppData\Roaming\.tool\notes.md"));
    }

    [Theory]
    [MemberData(nameof(SecretPaths))]
    public void Secrets_and_the_browsers_own_data_are_never_indexed(string path)
    {
        // With the whole profile chosen, with hidden folders on, with AppData chosen.
        string[][] choices = [[@"C:\Users\me"], [@"C:\Users\me", @"C:\Users\me\.notes"], [@"C:\Users\me\AppData", @"C:\Data"]];
        foreach (var folders in choices)
        {
            var options = IndexPlan.Options(folders, contents: true, own: @"C:\Data\Mine");
            Assert.True(Skipped(options, path), $"{path} with {string.Join(", ", folders)}");
        }
    }

    [Fact]
    public void A_secret_folder_chosen_itself_still_gives_nothing()
    {
        Assert.True(Skipped(IndexPlan.Options([@"C:\Users\me\.ssh"], true), @"C:\Users\me\.ssh\id_ed25519"));
        Assert.False(Skipped(IndexPlan.Options([@"C:\Users\me"], true, own: @"C:\Data\Mine"), @"C:\Users\me\Documents\cv.pdf"));
    }
}
