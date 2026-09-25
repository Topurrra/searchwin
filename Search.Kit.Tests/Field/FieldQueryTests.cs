using System.Diagnostics;
using SearchKit.Commands;
using SearchKit.Field;

namespace SearchKit.Tests.Field;

public class FieldQueryTests
{
    private static readonly Bangs bangs = new();

    private static FieldQuery Read(string typed) => FieldQuery.Read(typed, bangs);

    [Theory]
    [InlineData("", QueryKind.Empty)]
    [InlineData("   ", QueryKind.Empty)]
    [InlineData("github.com", QueryKind.Address)]
    [InlineData("https://example.com/a b", QueryKind.Words)] // a space: not an address
    [InlineData("https://example.com/a", QueryKind.Address)]
    [InlineData("localhost:3000", QueryKind.Address)]
    [InlineData("192.168.1.1", QueryKind.Address)]
    [InlineData("about:blank", QueryKind.Address)]
    [InlineData("rust ownership", QueryKind.Words)]
    [InlineData("todo", QueryKind.Words)]
    [InlineData("1.5", QueryKind.Words)]
    [InlineData("2024", QueryKind.Words)]
    [InlineData("23*47", QueryKind.Calculation)]
    [InlineData("23 * 47", QueryKind.Calculation)]
    [InlineData("(2+3)^2", QueryKind.Calculation)]
    [InlineData("-5+3", QueryKind.Calculation)]
    [InlineData("15% of 240", QueryKind.Calculation)]
    [InlineData("240 + 15%", QueryKind.Calculation)]
    [InlineData("10/4", QueryKind.Words)] // a bare pair: see Ambiguous_sums_stay_words
    [InlineData("10 / 4", QueryKind.Calculation)]
    [InlineData("23*", QueryKind.Words)] // still being typed
    [InlineData("-5", QueryKind.Words)]
    [InlineData("2024-01-05", QueryKind.Words)] // a date
    [InlineData("12/31/2024", QueryKind.Words)]
    [InlineData("555-1234", QueryKind.Words)] // a phone number
    [InlineData("10 km to mi", QueryKind.Conversion)]
    [InlineData("5km in m", QueryKind.Conversion)]
    [InlineData("32 °F to °C", QueryKind.Conversion)]
    [InlineData("1.5 GB to MB", QueryKind.Conversion)]
    [InlineData("5 things to do", QueryKind.Words)]
    [InlineData("10 to 20", QueryKind.Words)]
    [InlineData("#e07a5f", QueryKind.Color)]
    [InlineData("#fff", QueryKind.Color)]
    [InlineData("#e07a5f to hsl", QueryKind.Color)]
    [InlineData("rgb(224, 122, 95)", QueryKind.Color)]
    [InlineData("#hashtag", QueryKind.Words)]
    [InlineData("#e07a5f is nice", QueryKind.Words)]
    [InlineData("sha256 hello", QueryKind.Hash)]
    [InlineData("MD5 hello world", QueryKind.Hash)]
    [InlineData("sha256", QueryKind.Words)]
    [InlineData("base64 hello", QueryKind.Encode)]
    [InlineData("base64 decode aGk=", QueryKind.Encode)]
    [InlineData("url encode a b", QueryKind.Encode)]
    [InlineData("urlencode a b", QueryKind.Encode)]
    [InlineData("url shortener", QueryKind.Words)] // everyday words need a mode
    [InlineData("html tutorial", QueryKind.Words)]
    [InlineData("json2yaml {\"a\":1}", QueryKind.Format)]
    [InlineData("json2json {}", QueryKind.Words)]
    [InlineData(">close others", QueryKind.Command)]
    [InlineData("!yt cats", QueryKind.Bang)]
    [InlineData("cats !yt", QueryKind.Bang)]
    [InlineData("!nosuchbang x", QueryKind.Words)]
    [InlineData("? what's the catch", QueryKind.Ask)]
    [InlineData("\\>not a command", QueryKind.Words)]
    [InlineData("\\23*47", QueryKind.Words)]
    public void Each_kind_of_input_is_told_apart(string typed, QueryKind kind)
    {
        Assert.Equal(kind, Read(typed).Kind);
    }

    [Theory]
    [InlineData("clip: invoice", Scope.Clipboard, QueryKind.Words, "invoice")]
    [InlineData("clip:", Scope.Clipboard, QueryKind.Empty, "")]
    [InlineData("CLIPBOARD:  a b ", Scope.Clipboard, QueryKind.Words, "a b")]
    [InlineData("clipboard", Scope.Clipboard, QueryKind.Empty, "")]
    [InlineData(" Clipboard ", Scope.Clipboard, QueryKind.Empty, "")]
    [InlineData("clipboard manager", Scope.All, QueryKind.Words, "clipboard manager")]
    [InlineData("files: invoice 2024", Scope.Files, QueryKind.Words, "invoice 2024")]
    [InlineData("apps: code", Scope.Apps, QueryKind.Words, "code")]
    [InlineData("apps:", Scope.Apps, QueryKind.Empty, "")]
    [InlineData("tabs: git", Scope.Tabs, QueryKind.Words, "git")]
    [InlineData("history: git", Scope.History, QueryKind.Words, "git")]
    public void A_scope_narrows_the_field(string typed, Scope scope, QueryKind kind, string text)
    {
        var query = Read(typed);
        Assert.Equal(scope, query.Scope);
        Assert.Equal(kind, query.Kind);
        Assert.Equal(text, query.Text);
    }

    [Theory]
    [InlineData("24/7")]
    [InlineData("9/11")]
    [InlineData("7-11")]
    [InlineData("50/50")]
    [InlineData("20-20")]
    [InlineData("1/2")]
    [InlineData("3.5/2")]
    public void Ambiguous_sums_stay_words(string typed)
    {
        var query = Read(typed);
        Assert.Equal(QueryKind.Words, query.Kind);
        Assert.False(query.IsAnswer);
        // The calculator may still read it, on the side.
        Assert.Equal(typed, query.Sum);
    }

    [Theory]
    [InlineData("24 / 7", "24 / 7")]
    [InlineData("7 - 11", "7 - 11")]
    [InlineData("24/7=", "24/7")]
    [InlineData("7-11 =", "7-11")]
    [InlineData("2+2", "2+2")]
    [InlineData("6*7", "6*7")]
    [InlineData("6×7", "6×7")]
    [InlineData("10÷4", "10÷4")]
    [InlineData("2^10", "2^10")]
    [InlineData("50%", "50%")]
    [InlineData("(10-2)/4", "(10-2)/4")]
    [InlineData("15% of 240", "15% of 240")]
    public void An_operator_nothing_else_uses_makes_a_sum(string typed, string text)
    {
        var query = Read(typed);
        Assert.Equal(QueryKind.Calculation, query.Kind);
        Assert.Equal(text, query.Text);
        Assert.Null(query.Sum);
    }

    [Theory]
    [InlineData("(555) 123-4567")]
    [InlineData("555-1234")]
    [InlineData("2024-01-05")]
    [InlineData("12/31/2024")]
    [InlineData("1-2-3")]
    [InlineData("24/")]
    public void Dates_numbers_and_half_typed_sums_are_neither(string typed)
    {
        var query = Read(typed);
        Assert.False(query.IsAnswer);
        Assert.Null(query.Sum);
    }

    [Fact]
    public void An_address_is_never_a_sum()
    {
        var query = Read("192.168.1.1/24");
        Assert.Equal(QueryKind.Address, query.Kind);
        Assert.Null(query.Sum);
    }

    [Fact]
    public void A_file_address_is_not_a_scope()
    {
        Assert.Equal(Scope.All, Read("file:///C:/x.pdf").Scope);
        Assert.Equal(QueryKind.Address, Read("file:///C:/x.pdf").Kind);
    }

    [Fact]
    public void Answers_carry_their_tool_mode_and_operand()
    {
        var hash = Read("sha-256 hello there");
        Assert.Equal(("sha256", "hello there"), (hash.Tool, hash.Operand));

        var encode = Read("base64 hello");
        Assert.Equal(("base64", "encode", "hello"), (encode.Tool, encode.Mode, encode.Operand));
        var decode = Read("b64 dec aGk=");
        Assert.Equal(("base64", "decode", "aGk="), (decode.Tool, decode.Mode, decode.Operand));
        var glued = Read("urldecode a%20b");
        Assert.Equal(("url", "decode", "a%20b"), (glued.Tool, glued.Mode, glued.Operand));

        var format = Read("yml2json a: 1");
        Assert.Equal(("yaml", "json", "a: 1"), (format.Tool, format.Mode, format.Operand));

        var color = Read("#E07A5F in hsl");
        Assert.Equal(("hex", "hsl", "#E07A5F"), (color.Tool, color.Mode, color.Operand));
        Assert.True(color.IsAnswer);
        Assert.False(Read("rust").IsAnswer);
    }

    [Fact]
    public void Bangs_keep_their_words()
    {
        var query = Read("!yt funny cats");
        Assert.Equal("yt", query.Bang!.Trigger);
        Assert.Equal("funny cats", query.Text);
        // Without the list of bangs, it's words.
        Assert.Equal(QueryKind.Words, FieldQuery.Read("!yt cats").Kind);
    }

    [Fact]
    public void The_browsers_address_test_wins_when_given()
    {
        Assert.Equal(QueryKind.Words, FieldQuery.Read("github.com", isAddress: _ => false).Kind);
        Assert.Equal(QueryKind.Address, FieldQuery.Read("intranet", isAddress: t => t == "intranet").Kind);
    }
}

[Collection("Budgets")]
public class FieldQueryBudgetTests(Xunit.Abstractions.ITestOutputHelper output)
{
    [Fact]
    public void Reading_is_far_under_half_a_millisecond()
    {
        var bangs = new Bangs();
        var inputs = new[]
        {
            "github.com", "rust ownership", "23*47", "10 km to mi", "#e07a5f to hsl", "sha256 hello", "base64 decode aGk=",
            "clip: invoice", "files: invoice 2024", "apps: code", ">close others", "!yt cats", "? why", "json2yaml {}",
            "invoice 2024", "https://example.com/a?b=c", "2024-01-05", "url shortener",
        };
        foreach (var input in inputs) FieldQuery.Read(input, bangs);
        var worst = 0.0;
        var clock = Stopwatch.StartNew();
        for (var i = 0; i < 20_000; i++)
        {
            var start = clock.Elapsed.TotalMilliseconds;
            FieldQuery.Read(inputs[i % inputs.Length], bangs);
            worst = Math.Max(worst, clock.Elapsed.TotalMilliseconds - start);
        }
        var perRead = clock.Elapsed.TotalMilliseconds / 20_000;
        output.WriteLine($"{perRead * 1000:F2} µs per read, worst {worst:F3} ms");
        Assert.True(perRead < 0.05, $"{perRead:F4} ms per read");
        // One slow read is a GC pause or the scheduler, not the reader.
        Assert.True(worst < 5, $"worst read {worst:F3} ms");
    }
}

[CollectionDefinition("Budgets", DisableParallelization = true)]
public class BudgetCollection;
