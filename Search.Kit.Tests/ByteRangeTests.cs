using SearchKit.Web;

namespace SearchKit.Tests;

public class ByteRangeTests
{
    private const ulong Ten = 10 * 1024 * 1024;

    [Theory]
    [InlineData("bytes=0-9", 0UL, 9UL)]
    [InlineData("bytes=100-", 100UL, 100UL + ByteRange.Chunk - 1)]
    [InlineData("bytes=-100", Ten - 100, Ten - 1)]
    [InlineData("bytes=0-99999999999", 0UL, ByteRange.Chunk - 1)]
    [InlineData("BYTES=5-5", 5UL, 5UL)]
    [InlineData("bytes=0-9, 20-29", 0UL, 9UL)]
    public void Spans(string header, ulong start, ulong end)
    {
        Assert.Equal((start, end), ByteRange.Parse(header, Ten));
    }

    [Fact]
    public void The_end_of_the_file_caps_the_span()
    {
        Assert.Equal((Ten - 10, Ten - 1), ByteRange.Parse($"bytes={Ten - 10}-", Ten));
        Assert.Equal((0UL, 99UL), ByteRange.Parse("bytes=-5000", 100));
    }

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("items=0-9")]
    [InlineData("bytes=9-0")]
    [InlineData("bytes=-0")]
    [InlineData("bytes=abc-")]
    [InlineData("bytes=5-x")]
    [InlineData("bytes=10485760-")]
    public void Unsatisfiable_or_absent(string? header)
    {
        Assert.Null(ByteRange.Parse(header, Ten));
    }

    [Fact]
    public void An_empty_file_has_no_ranges()
    {
        Assert.Null(ByteRange.Parse("bytes=0-", 0));
    }
}
