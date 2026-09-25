using System.Diagnostics;
using SearchKit.Shields;
using Xunit.Abstractions;

namespace SearchKit.Tests.Shields;

public class SiteSheetTests
{
    private readonly ITestOutputHelper output;
    public SiteSheetTests(ITestOutputHelper output) => this.output = output;

    private static string Script() =>
        SiteSheet.Script("\"news.example\"", "\".ad-slot { display: none !important; }\\n.promo { display: none !important; }\"");

    // `window[Symbol.for('search:shield-site')]` was a flag any page could
    // read by name — and Object.getOwnPropertySymbols(window) listed it.
    [Fact]
    public void The_script_leaves_no_mark_on_the_page()
    {
        var script = Script();
        Assert.DoesNotContain("Symbol", script);
        Assert.DoesNotContain("defineProperty", script);
        Assert.DoesNotContain("window", script);
        Assert.DoesNotContain("__search", script, StringComparison.OrdinalIgnoreCase);
    }

    // Run twice on one document (at creation, and again as a fast page
    // arrives), in node with a stand-in for the few DOM parts it touches:
    // one sheet adopted, beside the page's own, and nothing added to the
    // global object. Without node there is nothing to run, and it says so.
    [Fact]
    public void Run_twice_it_adopts_its_sheet_once_and_adds_no_globals()
    {
        const string harness = """
        'use strict';
        const vm = require('node:vm');
        const fs = require('node:fs');
        class CSSStyleSheet {
          constructor() { this.cssRules = []; }
          replaceSync(text) { this.cssRules = text.split('\n').filter(Boolean).map(t => ({ cssText: t.trim() })); }
        }
        const pages = new CSSStyleSheet(); pages.replaceSync('.theirs { color: red; }');
        const context = { location: { hostname: 'news.example' }, document: { adoptedStyleSheets: [pages] }, CSSStyleSheet };
        vm.createContext(context);
        const before = new Set([...Object.getOwnPropertyNames(context), ...Object.getOwnPropertySymbols(context)]);
        const script = fs.readFileSync(process.argv[2], 'utf8');
        vm.runInContext(script, context);
        vm.runInContext(script, context);
        const added = [...Object.getOwnPropertyNames(context), ...Object.getOwnPropertySymbols(context)].filter(k => !before.has(k));
        const other = { location: { hostname: 'elsewhere.example' }, document: { adoptedStyleSheets: [] }, CSSStyleSheet };
        vm.createContext(other);
        vm.runInContext(script, other);
        console.log(JSON.stringify({ sheets: context.document.adoptedStyleSheets.length, added: added.map(String), elsewhere: other.document.adoptedStyleSheets.length }));
        """;
        var folder = Directory.CreateTempSubdirectory("sitesheet");
        try
        {
            var harnessPath = Path.Combine(folder.FullName, "harness.js");
            var scriptPath = Path.Combine(folder.FullName, "site.js");
            File.WriteAllText(harnessPath, harness);
            File.WriteAllText(scriptPath, Script());
            string said;
            try
            {
                using var node = Process.Start(new ProcessStartInfo("node", [harnessPath, scriptPath])
                {
                    RedirectStandardOutput = true,
                    RedirectStandardError = true,
                    UseShellExecute = false,
                })!;
                said = node.StandardOutput.ReadToEnd().Trim();
                var error = node.StandardError.ReadToEnd();
                node.WaitForExit();
                Assert.True(node.ExitCode == 0, error);
            }
            catch (System.ComponentModel.Win32Exception)
            {
                output.WriteLine("node isn't installed; nothing run.");
                return;
            }
            Assert.Equal("""{"sheets":2,"added":[],"elsewhere":0}""", said);
        }
        finally
        {
            folder.Delete(recursive: true);
        }
    }
}
