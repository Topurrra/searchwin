using System.Text.Json;
using SearchKit.FishCatcher;

namespace SearchKit.Tests;

/// What a page can and can't make Search say about it. The probe's facts
/// arrive from the page itself (fish.facts over the bridge), so anything in
/// them may be forged: they can make a page look worse, never put the page's
/// own words into Search's warning.
public class PageFactsTests
{
    private static FishData Data => ParityTests.Data.Value;

    private const string Lure = "Your PC is infected. Call Microsoft Support 1-888-555-0199";

    [Fact]
    public void A_forged_safe_browsing_key_is_not_a_reason()
    {
        var facts = PageFacts.Parse($$"""{"gsbThreat":"{{Lure}}"}""");
        var v = Analyzer.Analyze("https://quiet-site.example/", Data, facts)!;
        Assert.DoesNotContain(v.Reasons, r => r.Contains("1-888", StringComparison.Ordinal));
        Assert.DoesNotContain(v.Signals, s => s.Key == Lure);
        Assert.Equal(Analyzer.Analyze("https://quiet-site.example/", Data)!.Score, v.Score);
    }

    [Fact]
    public void A_form_action_that_is_not_a_host_name_is_ignored()
    {
        var facts = PageFacts.Parse($$$"""
            {"aitm":{"interactions":["password"],"identityHints":{"title":"Sign in"},"resourceHosts":{},
                     "faviconCrossOrigin":false,"formActions":["{{{Lure}}}"]}}
            """);
        var v = Analyzer.Analyze("https://quiet-site.example/", Data, facts)!;
        Assert.DoesNotContain(v.Reasons, r => r.Contains("1-888", StringComparison.Ordinal) || r.Contains("infected", StringComparison.Ordinal));
        Assert.DoesNotContain(v.Signals, s => s.Key is "reasonFormAction" or "reasonFormExfil");
    }

    [Theory]
    [InlineData("call-now-18885550199")]      // no dot: the naive PSL keeps it whole
    [InlineData("support line.example")]
    [InlineData("evil.example/<b>x</b>")]
    [InlineData("")]
    public void Only_host_names_are_form_actions(string action)
    {
        var facts = new PageFacts
        {
            Aitm = new AitmFacts { Interactions = ["password"], FormActions = [action] },
        };
        var v = Analyzer.Analyze("https://quiet-site.example/", Data, facts)!;
        Assert.DoesNotContain(v.Signals, s => s.Key is "reasonFormAction" or "reasonFormExfil");
    }

    [Fact]
    public void Real_form_actions_still_count()
    {
        var facts = new PageFacts
        {
            Aitm = new AitmFacts { Interactions = ["password"], FormActions = ["COLLECTOR.Other-Site.net"] },
        };
        var v = Analyzer.Analyze("https://quiet-site.example/", Data, facts)!;
        Assert.Contains("The login form sends what you type to other-site.net, not to this site", v.Reasons);
    }

    [Fact]
    public void The_probe_cannot_say_what_Safe_Browsing_or_the_domain_age_said()
    {
        // Even well-formed ones: those are Search's to find out.
        using var doc = JsonDocument.Parse("""{"gsbThreat":"reasonGsbMalware","youngDomainDays":1,"deviceCode":true}""");
        var facts = PageFacts.FromProbe(doc.RootElement)!;
        Assert.Null(facts.GsbThreat);
        Assert.Null(facts.YoungDomainDays);
        Assert.False(facts.DeviceCode);
        var v = Analyzer.Analyze("https://quiet-site.example/", Data, facts)!;
        Assert.Equal(Analyzer.Analyze("https://quiet-site.example/", Data)!.Score, v.Score);
    }

    [Fact]
    public void The_probe_reads_as_the_extension_within_its_limits()
    {
        // The parity corpus's page facts give the same verdicts through FromProbe
        // as through From, as long as they hold nothing only Search may say.
        var golden = JsonDocument.Parse(File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, "FishCatcher", "golden.json")));
        int compared = 0;
        foreach (var item in golden.RootElement.GetProperty("pages").EnumerateArray())
        {
            var raw = item.GetProperty("facts");
            if (raw.TryGetProperty("gsbThreat", out _) || raw.TryGetProperty("youngDomainDays", out _) || raw.TryGetProperty("deviceCode", out _)) continue;
            var url = item.GetProperty("url").GetString()!;
            var a = Analyzer.Analyze(url, Data, PageFacts.From(raw))!;
            var b = Analyzer.Analyze(url, Data, PageFacts.FromProbe(raw))!;
            Assert.Equal(a.Score, b.Score);
            Assert.Equal(a.Reasons, b.Reasons);
            compared++;
        }
        Assert.True(compared >= 15, $"{compared} compared");
    }

    [Fact]
    public void The_probe_facts_are_held_to_the_probe_limits()
    {
        var huge = new string('a', 5000);
        var json = new System.Text.StringBuilder("""{"aitm":{"interactions":["password","password","sorcery","otp"],""");
        json.Append($$$"""
            "identityHints":{"title":"{{{huge}}}","ogSiteName":"{{{huge}}}","logoAlts":[{{{string.Join(",", Enumerable.Range(0, 500).Select(i => $"\"logo {i}\""))}}}],
                             "brandTokens":[{{{string.Join(",", Enumerable.Range(0, 500).Select(i => $"\"t{i}\""))}}}]},
            "resourceHosts":{"EVIL.example":3,"not a host":9,"bad.example":-4,{{{string.Join(",", Enumerable.Range(0, 500).Select(i => $"\"h{i}.example\":1"))}}}},
            "formActions":["Collector.Example","collector.example","{{{Lure}}}",{{{string.Join(",", Enumerable.Range(0, 500).Select(i => $"\"f{i}.example\""))}}}]}}
            """);
        using var doc = JsonDocument.Parse(json.ToString());
        var aitm = PageFacts.FromProbe(doc.RootElement)!.Aitm!;

        Assert.Equal(["password", "otp"], aitm.Interactions);
        Assert.Equal(PageFacts.MaxTitle, aitm.IdentityHints.Title.Length);
        Assert.Equal(PageFacts.MaxHint, aitm.IdentityHints.OgSiteName.Length);
        Assert.Equal(PageFacts.MaxLogoAlts, aitm.IdentityHints.LogoAlts.Count);
        Assert.Equal(PageFacts.MaxBrandTokens, aitm.IdentityHints.BrandTokens.Count);
        Assert.True(aitm.ResourceHosts.Count <= PageFacts.MaxResourceHosts);
        Assert.Equal(3, aitm.ResourceHosts["evil.example"]);
        Assert.DoesNotContain("not a host", aitm.ResourceHosts.Keys);
        Assert.DoesNotContain("bad.example", aitm.ResourceHosts.Keys);
        Assert.True(aitm.FormActions.Count <= PageFacts.MaxFormActions);
        Assert.Equal("collector.example", aitm.FormActions[0]);
        Assert.Single(aitm.FormActions, a => a == "collector.example");
        Assert.DoesNotContain(aitm.FormActions, a => a.Contains(' '));
    }

    [Fact]
    public void A_title_is_never_cut_through_a_surrogate_pair()
    {
        var title = new string('x', PageFacts.MaxTitle - 1) + "🐟";
        using var doc = JsonDocument.Parse("""{"aitm":{"interactions":["password"],"identityHints":{"title":""" + JsonSerializer.Serialize(title) + "}}}");
        var cut = PageFacts.FromProbe(doc.RootElement)!.Aitm!.IdentityHints.Title;
        Assert.Equal(PageFacts.MaxTitle - 1, cut.Length);
        Assert.False(char.IsHighSurrogate(cut[^1]));
    }

    [Theory]
    [InlineData("paypal.com", true)]
    [InlineData("a.b.c.example.", true)]
    [InlineData("xn--pple-43d.com", true)]
    [InlineData("my_host.example", true)]
    [InlineData("192.0.2.9", true)]
    [InlineData("[::1]", true)]
    [InlineData("[2001:db8::1]", true)]
    [InlineData("localhost", false)]
    [InlineData("call-now-18885550199", false)]
    [InlineData("support line.example", false)]
    [InlineData(".example.com", false)]
    [InlineData("a..example", false)]
    [InlineData("pаypal.com", false)]   // Cyrillic а: URL would have given xn--
    [InlineData("[call me]", false)]
    [InlineData("", false)]
    public void Host_names_are_host_names(string host, bool expected) => Assert.Equal(expected, PageFacts.IsHost(host));

    [Fact]
    public void Host_names_are_at_most_253_characters_in_labels_of_63()
    {
        Assert.True(PageFacts.IsHost(new string('a', 63) + ".example"));
        Assert.False(PageFacts.IsHost(new string('a', 64) + ".example"));
        var longest = string.Join(".", Enumerable.Repeat(new string('a', 49), 5)) + ".abc"; // 249 + 4 = 253
        Assert.Equal(253, longest.Length);
        Assert.True(PageFacts.IsHost(longest));
        Assert.False(PageFacts.IsHost("a" + longest));
    }

    [Fact]
    public void A_document_is_heard_a_handful_of_times()
    {
        var budget = new ProbeBudget();
        for (int i = 0; i < ProbeBudget.PerDocument; i++) Assert.True(budget.Take());
        Assert.False(budget.Take());
        Assert.False(budget.Take());
        budget.NewDocument();
        Assert.True(budget.Take());
    }

    [Fact]
    public void A_signal_with_no_sentence_is_never_shown()
    {
        var v = new Verdict
        {
            Url = "https://x.example/", Host = "x.example", Registrable = "x.example", Score = 60, Level = Level.High,
            Signals = [new Signal(Lure, 60), new Signal("reasonHttp", 5)],
        };
        Assert.Equal(["Connection is not encrypted (http instead of https)"], v.Reasons);
        Assert.Equal("", new Signal(Lure, 60).Sentence);
    }
}
