using System.Text.Json.Nodes;
using SearchKit.Field;

namespace SearchKit.Tests.Field;

/// IndexPlan's options checked the way the engine reads them: exclusions per
/// chosen folder, hidden folders, and what is never indexed at all.
public class IndexPlanRootTests
{
    /// The engine's own test (search.rs `should_include_path`, less the
    /// system folders): exclusions normalised as `normalize_unique_tokens`
    /// does, then a path-like one against the whole path, a name against
    /// every folder of it, and dot-folders unless hidden ones are on.
    private static bool Skipped(JsonObject options, string path)
    {
        var normal = path.Replace('\\', '/').ToLowerInvariant().TrimEnd('/');
        foreach (var node in options["excludeFolders"]!.AsArray())
        {
            var excluded = node!.GetValue<string>().Trim().Trim('"').Trim('\'').ToLowerInvariant().Replace('\\', '/').Trim('/').Trim();
            if (excluded.Length == 0) continue;
            if (excluded.Contains('/') || excluded.Contains(':') || excluded.Contains('*'))
            {
                if (EngineMatches(normal, excluded)) return true;
            }
            else if (normal.Split('/').Contains(excluded)) return true;
        }
        return !options["includeHidden"]!.GetValue<bool>() && normal.Split('/').Any(p => p.StartsWith('.'));
    }

    /// search.rs `exclusion_pattern_matches_path`.
    private static bool EngineMatches(string path, string excluded)
    {
        if (excluded.Contains('*'))
        {
            var rest = path;
            foreach (var part in excluded.Split('*', StringSplitOptions.RemoveEmptyEntries))
            {
                var at = rest.IndexOf(part, StringComparison.Ordinal);
                if (at < 0) return false;
                rest = rest[(at + part.Length)..];
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
    ];

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
