namespace SearchKit.Shields;

/// One site's own stylesheet (FilterList.SiteCss) as a page script, for
/// that host only: a page on another site that inherits the script before
/// the browser swaps it leaves it be.
///
/// It can run twice for one document — added at document creation, and run
/// again the moment a page that came faster than it starts arriving — and
/// the second time finds its own rules already adopted and stops. No flag
/// says so: anything left on `window` (a name, even a Symbol in the global
/// registry) is one more thing a page can look up to tell Search from other
/// browsers.
public static class SiteSheet
{
    /// `hostLiteral` and `cssLiteral` are JavaScript string literals, quotes
    /// and escapes included (the browser's Bridge.Literal).
    public static string Script(string hostLiteral, string cssLiteral) => $$"""
    (function () {
      if (location.hostname !== {{hostLiteral}}) return;
      try {
        var sheet = new CSSStyleSheet();
        sheet.replaceSync({{cssLiteral}});
        var mine = sheet.cssRules, have = document.adoptedStyleSheets;
        for (var i = 0; i < have.length; i++) {
          var theirs = have[i].cssRules, j = 0;
          if (theirs.length !== mine.length) continue;
          while (j < mine.length && theirs[j].cssText === mine[j].cssText) j++;
          if (j === mine.length) return;
        }
        document.adoptedStyleSheets = have.concat([sheet]);
      } catch (e) {}
    })();
    """;
}
