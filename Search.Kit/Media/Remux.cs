using System.Globalization;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json.Nodes;

namespace SearchKit.Media;

/// A subtitle track the player shows: a text track inside the file, or a
/// file beside it (`film.srt`, `film.en.ass`), turned into WebVTT.
public sealed record Subtitle(string Label, string Language, int? Stream = null, string? Beside = null);

/// How the FFmpeg pack turns a file WebView2 can't play into one it can:
/// what ffprobe said about it in, FFmpeg's arguments out. Everything that
/// Chromium decodes is copied as it is (near-instant, no loss); audio it
/// can't decode (AC-3, DTS, WMA…) becomes AAC; video only when it must
/// (or when asked, after a copy didn't play), with Windows' own H.264
/// encoder — the pack carries no x264.
public sealed record Remux(
    IReadOnlyList<string> Arguments,
    double Duration,
    bool HasVideo,
    bool CopiesVideo,
    bool CopiesAudio,
    IReadOnlyList<Subtitle> Subtitles)
{
    /// Video Chromium decodes, in MP4. HEVC only where the PC can: when it
    /// can't, the player asks again with `transcode`.
    private static readonly HashSet<string> Video = new(StringComparer.Ordinal) { "h264", "hevc", "vp8", "vp9", "av1" };

    /// Audio Chromium decodes, in MP4.
    private static readonly HashSet<string> Audio = new(StringComparer.Ordinal) { "aac", "mp3", "opus", "flac" };

    /// Subtitles FFmpeg can write as WebVTT (text, not pictures).
    private static readonly HashSet<string> Text = new(StringComparer.Ordinal) { "subrip", "srt", "ass", "ssa", "webvtt", "mov_text", "text" };

    /// The name of the playable file in the output folder, and of each
    /// subtitle track's (`sub-<n>.vtt`, in the order of `Subtitles`).
    public const string Media = "media.mp4";

    public static string SubtitleFile(int n) => $"sub-{n.ToString(CultureInfo.InvariantCulture)}.vtt";

    /// The plan for `input`, from `ffprobe -print_format json -show_format
    /// -show_streams`. Output goes into `folder`. Null when there's nothing
    /// in it to play.
    public static Remux? Plan(JsonNode? probe, string input, string folder, bool transcode = false)
    {
        var streams = (probe?["streams"] as JsonArray ?? []).OfType<JsonObject>().ToList();
        JsonObject? video = streams.FirstOrDefault(s => Str(s, "codec_type") == "video" && Int(s["disposition"], "attached_pic") != 1);
        var audios = streams.Where(s => Str(s, "codec_type") == "audio").ToList();
        var audio = audios.FirstOrDefault(s => Int(s["disposition"], "default") == 1) ?? audios.FirstOrDefault();
        if (video == null && audio == null) return null;

        var args = new List<string> { "-hide_banner", "-nostdin", "-loglevel", "error", "-y", "-progress", "pipe:1", "-nostats", "-i", input };
        var copiesVideo = false;
        if (video != null)
        {
            args.AddRange(["-map", $"0:{Int(video, "index")}"]);
            var codec = Str(video, "codec_name");
            copiesVideo = !transcode && Video.Contains(codec);
            if (copiesVideo)
            {
                args.AddRange(["-c:v", "copy"]);
                // The tag Chromium looks for in MP4.
                if (codec == "hevc") args.AddRange(["-tag:v", "hvc1"]);
            }
            else
            {
                var pixels = Math.Max(1, Int(video, "width")) * (long)Math.Max(1, Int(video, "height"));
                var bitrate = Math.Clamp(pixels * 4, 1_000_000L, 12_000_000L);
                args.AddRange(["-c:v", "h264_mf", "-b:v", bitrate.ToString(CultureInfo.InvariantCulture), "-pix_fmt", "yuv420p"]);
            }
        }
        var copiesAudio = false;
        if (audio != null)
        {
            args.AddRange(["-map", $"0:{Int(audio, "index")}"]);
            copiesAudio = Audio.Contains(Str(audio, "codec_name"));
            if (copiesAudio) args.AddRange(["-c:a", "copy"]);
            else
            {
                args.AddRange(["-c:a", "aac", "-b:a", "192k"]);
                if (Int(audio, "channels") > 2) args.AddRange(["-ac", "2"]);
            }
        }
        args.AddRange(["-sn", "-dn", "-movflags", "+faststart", "-f", "mp4", Path.Combine(folder, Media)]);

        var subtitles = new List<Subtitle>();
        foreach (var stream in streams.Where(s => Str(s, "codec_type") == "subtitle" && Text.Contains(Str(s, "codec_name"))))
        {
            var index = Int(stream, "index");
            var language = Str(stream["tags"], "language");
            var title = Str(stream["tags"], "title");
            var label = title.Length > 0 ? title : language.Length > 0 ? language : $"Subtitles {subtitles.Count + 1}";
            args.AddRange(["-map", $"0:{index}", "-c:s", "webvtt", "-f", "webvtt", Path.Combine(folder, SubtitleFile(subtitles.Count))]);
            subtitles.Add(new Subtitle(label, language, Stream: index));
        }

        var duration = double.TryParse(Str(probe?["format"], "duration"), NumberStyles.Float, CultureInfo.InvariantCulture, out var d) ? d : 0;
        return new Remux(args, duration, video != null, copiesVideo, copiesAudio, subtitles);
    }

    /// Whether WebView2 plays a file in a container it reads (MP4, WebM,
    /// MOV…) as it is. Chromium doesn't fail on audio it can't decode — an
    /// MP4 with AC-3 plays silent — so the streams are checked first.
    public static bool Plays(JsonNode? probe)
    {
        var streams = (probe?["streams"] as JsonArray ?? []).OfType<JsonObject>().ToList();
        var video = streams.FirstOrDefault(s => Str(s, "codec_type") == "video" && Int(s["disposition"], "attached_pic") != 1);
        var audios = streams.Where(s => Str(s, "codec_type") == "audio").ToList();
        var audio = audios.FirstOrDefault(s => Int(s["disposition"], "default") == 1) ?? audios.FirstOrDefault();
        var videoPlays = video == null || Video.Contains(Str(video, "codec_name")) || Str(video, "codec_name") == "theora";
        var audioCodec = audio == null ? "" : Str(audio, "codec_name");
        var audioPlays = audio == null || Audio.Contains(audioCodec) || audioCodec == "vorbis" || audioCodec.StartsWith("pcm_", StringComparison.Ordinal);
        return videoPlays && audioPlays;
    }

    /// Subtitle files beside `video` among `siblings` (names in its folder):
    /// `film.srt`, `film.en.srt`, `film.English.ass`, `film.vtt`.
    public static IReadOnlyList<Subtitle> Beside(string video, IEnumerable<string> siblings)
    {
        var stem = Path.GetFileNameWithoutExtension(video[(video.LastIndexOfAny(['\\', '/']) + 1)..]);
        var found = new List<Subtitle>();
        foreach (var name in siblings.Order(StringComparer.OrdinalIgnoreCase))
        {
            var ext = Path.GetExtension(name).TrimStart('.').ToLowerInvariant();
            if (ext is not ("srt" or "ass" or "ssa" or "vtt")) continue;
            var bare = Path.GetFileNameWithoutExtension(name);
            if (!bare.StartsWith(stem, StringComparison.OrdinalIgnoreCase)) continue;
            var rest = bare[stem.Length..];
            if (rest.Length > 0 && rest[0] != '.') continue;
            var language = rest.TrimStart('.');
            found.Add(new Subtitle(language.Length > 0 ? language : ext.ToUpperInvariant(), language, Beside: name));
        }
        return found;
    }

    /// Seconds FFmpeg has written so far, from a `-progress` line
    /// (`out_time_us=…`, in microseconds; `out_time_ms` is too, despite
    /// its name). Null for any other line.
    public static double? Progress(string line)
    {
        foreach (var key in (ReadOnlySpan<string>)["out_time_us=", "out_time_ms="])
            if (line.StartsWith(key, StringComparison.Ordinal)
                && long.TryParse(line.AsSpan(key.Length), NumberStyles.Integer, CultureInfo.InvariantCulture, out var us) && us >= 0)
                return us / 1_000_000.0;
        return null;
    }

    /// The cache folder's name for one file as it is now: another version
    /// of the file (size, time written) or another way of making it is
    /// another folder.
    public static string Key(string path, long size, DateTime written, string how) =>
        Convert.ToHexStringLower(SHA256.HashData(Encoding.UTF8.GetBytes(
            $"{path.ToLowerInvariant()}|{size}|{written.ToUniversalTime().Ticks}|{how}")))[..24];

    /// Which cached files to delete: anything unused for `maxAge`, then the
    /// least recently used until the rest fits in `cap` bytes.
    public static IReadOnlyList<string> Evict(IEnumerable<(string Name, long Bytes, DateTime Used)> cached, long cap, DateTime now, TimeSpan maxAge)
    {
        var gone = new List<string>();
        var total = 0L;
        var full = false;
        foreach (var (name, bytes, used) in cached.OrderByDescending(c => c.Used))
        {
            full |= total + bytes > cap;
            if (full || now - used > maxAge) gone.Add(name);
            else total += bytes;
        }
        return gone;
    }

    private static string Str(JsonNode? node, string key) =>
        node?[key] is JsonValue v && v.TryGetValue<string>(out var s) ? s : "";

    private static int Int(JsonNode? node, string key) =>
        node?[key] is JsonValue v && v.TryGetValue<int>(out var i) ? i : 0;
}
