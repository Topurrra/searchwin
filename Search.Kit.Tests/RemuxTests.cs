using System.Text.Json.Nodes;
using SearchKit.Media;

namespace SearchKit.Tests;

public class RemuxTests
{
    /// ffprobe's answer, cut to the fields the plan reads.
    private static JsonNode Probe(string? video, string? audio, int channels = 2, params string[] subtitles)
    {
        var streams = new JsonArray();
        if (video != null)
            streams.Add(new JsonObject { ["index"] = streams.Count, ["codec_type"] = "video", ["codec_name"] = video, ["width"] = 1920, ["height"] = 1080 });
        if (audio != null)
            streams.Add(new JsonObject { ["index"] = streams.Count, ["codec_type"] = "audio", ["codec_name"] = audio, ["channels"] = channels });
        foreach (var codec in subtitles)
            streams.Add(new JsonObject
            {
                ["index"] = streams.Count, ["codec_type"] = "subtitle", ["codec_name"] = codec,
                ["tags"] = new JsonObject { ["language"] = "eng" },
            });
        return new JsonObject { ["streams"] = streams, ["format"] = new JsonObject { ["duration"] = "60.000000" } };
    }

    private static string After(IReadOnlyList<string> args, string flag) => args[args.ToList().IndexOf(flag) + 1];

    [Theory]
    // MKV as it usually comes: H.264 copied, AC-3 5.1 made AAC stereo.
    [InlineData("h264", "ac3", 6, false, "copy", "aac", true)]
    [InlineData("h264", "dts", 2, false, "copy", "aac", false)]
    [InlineData("vp9", "opus", 2, false, "copy", "copy", false)]
    [InlineData("hevc", "aac", 2, false, "copy", "copy", false)]
    // AVI's MPEG-4 part 2 and WMV's VC-1: Chromium can't, so Windows' H.264.
    [InlineData("mpeg4", "mp3", 2, false, "h264_mf", "copy", false)]
    [InlineData("wmv3", "wmav2", 2, false, "h264_mf", "aac", false)]
    // A copy that didn't play (HEVC on a PC without it): asked again.
    [InlineData("hevc", "aac", 2, true, "h264_mf", "copy", false)]
    public void Chromium_gets_what_it_decodes_copied_and_the_rest_converted(
        string video, string audio, int channels, bool transcode, string videoCodec, string audioCodec, bool downmix)
    {
        var plan = Remux.Plan(Probe(video, audio, channels), @"C:\in\film.mkv", @"C:\out", transcode)!;

        Assert.Equal(videoCodec, After(plan.Arguments, "-c:v"));
        Assert.Equal(audioCodec, After(plan.Arguments, "-c:a"));
        Assert.Equal(downmix, plan.Arguments.Contains("-ac"));
        Assert.Equal(Path.Combine(@"C:\out", "media.mp4"), plan.Arguments[^1]);
        Assert.Equal(60, plan.Duration);
    }

    // An MP4 with AC-3 plays silent rather than failing, so it's caught
    // before it plays, not after.
    [Theory]
    [InlineData("h264", "aac", true)]
    [InlineData("vp9", "vorbis", true)]
    [InlineData("h264", "ac3", false)]
    [InlineData("prores", "pcm_s16le", false)]
    [InlineData(null, "alac", false)]
    public void A_file_in_a_container_webview2_reads_plays_as_it_is_only_if_it_decodes_every_stream(string? video, string audio, bool plays) =>
        Assert.Equal(plays, Remux.Plays(Probe(video, audio)));

    [Fact]
    public void Audio_alone_and_a_cover_picture_is_audio_only()
    {
        var probe = Probe(null, "wmav2");
        ((JsonArray)probe["streams"]!).Add(new JsonObject
        {
            ["index"] = 1, ["codec_type"] = "video", ["codec_name"] = "mjpeg",
            ["disposition"] = new JsonObject { ["attached_pic"] = 1 },
        });

        var plan = Remux.Plan(probe, @"C:\in\song.wma", @"C:\out")!;

        Assert.False(plan.HasVideo);
        Assert.DoesNotContain("-c:v", plan.Arguments);
        Assert.Equal("aac", After(plan.Arguments, "-c:a"));
    }

    [Fact]
    public void Text_subtitles_become_webvtt_tracks_and_picture_ones_are_left()
    {
        var plan = Remux.Plan(Probe("h264", "aac", 2, "subrip", "hdmv_pgs_subtitle", "ass"), @"C:\in\film.mkv", @"C:\out")!;

        Assert.Equal([2, 4], plan.Subtitles.Select(s => s.Stream));
        Assert.All(plan.Subtitles, s => Assert.Equal("eng", s.Language));
        Assert.Contains(Path.Combine(@"C:\out", "sub-0.vtt"), plan.Arguments);
        Assert.Contains(Path.Combine(@"C:\out", "sub-1.vtt"), plan.Arguments);
        Assert.DoesNotContain(Path.Combine(@"C:\out", "sub-2.vtt"), plan.Arguments);
        Assert.DoesNotContain("0:3", plan.Arguments);
    }

    [Fact]
    public void Nothing_to_play_is_no_plan() =>
        Assert.Null(Remux.Plan(Probe(null, null, 2, "subrip"), @"C:\in\subs.mkv", @"C:\out"));

    [Fact]
    public void Subtitle_files_beside_a_video_are_the_ones_named_after_it()
    {
        var found = Remux.Beside(@"C:\films\film.mkv",
            ["film.mkv", "film.srt", "film.en.ass", "film2.srt", "filmography.srt", "other.vtt", "film.nfo"]);

        Assert.Equal(["en", "SRT"], found.Select(s => s.Label));
        Assert.Equal(["film.en.ass", "film.srt"], found.Select(s => s.Beside));
    }

    [Theory]
    [InlineData("out_time_us=1500000", 1.5)]
    [InlineData("out_time_ms=61000000", 61.0)]
    [InlineData("out_time=00:00:01.500000", null)]
    [InlineData("out_time_us=N/A", null)]
    [InlineData("progress=continue", null)]
    public void Progress_is_read_from_ffmpegs_microseconds(string line, double? seconds) =>
        Assert.Equal(seconds, Remux.Progress(line));

    [Fact]
    public void The_cache_keeps_the_most_recently_played_that_fit_and_nothing_stale()
    {
        var now = new DateTime(2026, 9, 25, 12, 0, 0, DateTimeKind.Utc);
        var cached = new[]
        {
            ("today", 3L, now),
            ("yesterday", 3L, now.AddDays(-1)),
            ("small-but-older", 1L, now.AddDays(-2)),
            ("ancient", 1L, now.AddDays(-60)),
        };

        var gone = Remux.Evict(cached, cap: 6, now, TimeSpan.FromDays(30));

        Assert.Equal(["small-but-older", "ancient"], gone);
        Assert.Equal(["today", "yesterday", "small-but-older", "ancient"], Remux.Evict(cached, cap: 2, now, TimeSpan.FromDays(30)));
    }

    [Fact]
    public void A_changed_file_is_a_new_cache_entry()
    {
        var written = new DateTime(2026, 9, 1, 0, 0, 0, DateTimeKind.Utc);
        var key = Remux.Key(@"C:\films\film.mkv", 100, written, "remux");

        Assert.Equal(key, Remux.Key(@"c:\FILMS\film.mkv", 100, written, "remux"));
        Assert.NotEqual(key, Remux.Key(@"C:\films\film.mkv", 101, written, "remux"));
        Assert.NotEqual(key, Remux.Key(@"C:\films\film.mkv", 100, written.AddSeconds(1), "remux"));
        Assert.NotEqual(key, Remux.Key(@"C:\films\film.mkv", 100, written, "transcode"));
    }
}
