using System.Text.Json.Nodes;
using SearchKit.Engine;
using SearchKit.Field;

namespace SearchKit.Tests.Field;

public class AnswerTests
{
    [Fact]
    public void The_engine_is_asked_only_for_what_it_answers()
    {
        var calc = Answers.Plan(FieldQuery.Read("23*47"))!;
        Assert.Equal("evaluate_quick_query", calc.Method);
        Assert.Equal("""{"query":"23*47","webSearchEnabled":false}""", calc.Args.ToJsonString());

        Assert.Equal("evaluate_quick_query", Answers.Plan(FieldQuery.Read("10 km to mi"))!.Method);

        var encode = Answers.Plan(FieldQuery.Read("base64 decode aGk="))!;
        Assert.Equal("encode_decode", encode.Method);
        Assert.Equal("""{"algorithm":"base64","mode":"decode","input":"aGk="}""", encode.Args.ToJsonString());

        var format = Answers.Plan(FieldQuery.Read("json2yaml {\"a\":1}"))!;
        Assert.Equal("convert_format", format.Method);
        Assert.Equal(("json", "yaml", "{\"a\":1}"), (format.Args["from"]!.GetValue<string>(), format.Args["to"]!.GetValue<string>(), format.Args["input"]!.GetValue<string>()));

        // Worked out here, or not an answer at all.
        Assert.Null(Answers.Plan(FieldQuery.Read("sha256 hello")));
        Assert.Null(Answers.Plan(FieldQuery.Read("#e07a5f")));
        Assert.Null(Answers.Plan(FieldQuery.Read("rust")));
    }

    [Fact]
    public void Calculator_and_conversion_answers_become_rows_Enter_copies()
    {
        var sum = Assert.Single(Answers.Shape(FieldQuery.Read("23*47"),
            FakeEngine.Json("""{"type":"calculator","expression":"23*47","result":"1081"}""")));
        Assert.Equal(("1081", "23*47 =", RowAction.Copy, "1081"), (sum.Title, sum.Detail, sum.Action, sum.Target));

        var miles = Assert.Single(Answers.Shape(FieldQuery.Read("10 km to mi"),
            FakeEngine.Json("""{"type":"unitConversion","original":"10 km","result":"6.213712 mi"}""")));
        Assert.Equal(("6.213712 mi", "10 km ="), (miles.Title, miles.Detail));
    }

    [Fact]
    public void System_commands_addresses_and_bangs_never_come_back_as_answers()
    {
        var query = FieldQuery.Read("23*47");
        Assert.Empty(Answers.Shape(query, FakeEngine.Json("""{"type":"systemCommand","id":"shutdown","name":"Shut down","description":"","requiresConfirmation":true}""")));
        Assert.Empty(Answers.Shape(query, FakeEngine.Json("""{"type":"openUrl","url":"https://x","display":"x","result":"x"}""")));
        Assert.Empty(Answers.Shape(query, FakeEngine.Json("""{"type":"webSearch","provider":"g","url":"https://x","query":"x"}""")));
        Assert.Empty(Answers.Shape(query, null));
        Assert.Empty(Answers.Shape(query, FakeEngine.Json("[1,2]")));
    }

    [Fact]
    public void Encoded_and_converted_text_is_copied_whole()
    {
        var hi = Assert.Single(Answers.Shape(FieldQuery.Read("base64 hi"), JsonValue.Create("aGk=")));
        Assert.Equal(("aGk=", "Base64 encoded"), (hi.Title, hi.Detail));
        var decoded = Assert.Single(Answers.Shape(FieldQuery.Read("url decode a%20b"), JsonValue.Create("a b")));
        Assert.Equal("URL decoded", decoded.Detail);

        var yaml = "a: 1\nb:\n  - x\n  - y\n";
        var row = Assert.Single(Answers.Shape(FieldQuery.Read("json2yaml {\"a\":1,\"b\":[\"x\",\"y\"]}"), JsonValue.Create(yaml)));
        Assert.Equal(yaml, row.Target);
        Assert.Equal("a: 1 b: - x - y", row.Title);
        Assert.Equal("JSON → YAML", row.Detail);
    }

    [Theory]
    [InlineData("md5 hello", "5d41402abc4b2a76b9719d911017c592")]
    [InlineData("sha1 hello", "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")]
    [InlineData("sha256 hello", "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")]
    public void Hashes_are_worked_out_here(string typed, string hex)
    {
        var row = Assert.Single(Answers.Hash(FieldQuery.Read(typed)));
        Assert.Equal((hex, RowAction.Copy, hex), (row.Title, row.Action, row.Target));
    }

    [Fact]
    public void Colours_are_offered_in_the_other_formats()
    {
        var hex = Answers.Color(FieldQuery.Read("#e07a5f"));
        Assert.Equal(["rgb(224, 122, 95)", "hsl(13, 68%, 63%)"], hex.Select(r => r.Title));

        var wanted = Answers.Color(FieldQuery.Read("#e07a5f to hsl"));
        Assert.Equal("hsl(13, 68%, 63%)", wanted[0].Title);

        Assert.Equal(["#e07a5f", "hsl(13, 68%, 63%)"], Answers.Color(FieldQuery.Read("rgb(224, 122, 95)")).Select(r => r.Title));
        Assert.Equal("#ffffff", Answers.Color(FieldQuery.Read("hsl(0, 0%, 100%)"))[0].Title);
        Assert.Equal("rgba(255, 0, 0, 0.5)", Answers.Color(FieldQuery.Read("#ff000080"))[0].Title);
        Assert.Equal("rgb(255, 255, 255)", Answers.Color(FieldQuery.Read("#fff"))[0].Title);
    }

    [Fact]
    public async Task An_engine_error_is_no_answer()
    {
        var engine = new FakeEngine { Answer = (_, _, _) => Task.FromException<JsonNode?>(new EngineException("Invalid Base64")) };
        var rows = await new AnswerSource(engine).SuggestAsync(FieldQuery.Read("base64 decode !!"), 3, default);
        Assert.Empty(rows);
    }
}
