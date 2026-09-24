namespace SearchKit.Web;

/// An HTTP `Range: bytes=…` request, as a player seeking in a local video
/// sends it, turned into the bytes to answer with — at most one chunk, so a
/// two-hour film never sits in memory whole.
public static class ByteRange
{
    public const ulong Chunk = 4 * 1024 * 1024;

    /// `bytes=START-[END]` or `bytes=-SUFFIX` → the inclusive span to send,
    /// capped at `chunk` bytes. Null when there's no range, or it can't be
    /// satisfied (the whole file, or a 416, is then the caller's call). Only
    /// the first of several ranges is answered.
    public static (ulong Start, ulong End)? Parse(string? header, ulong size, ulong chunk = Chunk)
    {
        if (size == 0 || chunk == 0 || header is null || !header.StartsWith("bytes=", StringComparison.OrdinalIgnoreCase))
            return null;
        var spec = header[6..].Split(',')[0].Trim();
        var dash = spec.IndexOf('-');
        if (dash < 0) return null;
        ulong start, end;
        if (dash == 0)
        {
            if (!ulong.TryParse(spec[1..], out var suffix) || suffix == 0) return null;
            start = suffix >= size ? 0 : size - suffix;
            end = size - 1;
        }
        else
        {
            if (!ulong.TryParse(spec[..dash], out start) || start >= size) return null;
            if (dash == spec.Length - 1) end = size - 1;
            else if (ulong.TryParse(spec[(dash + 1)..], out var last)) end = Math.Min(last, size - 1);
            else return null;
            if (end < start) return null;
        }
        return (start, Math.Min(end, start + chunk - 1));
    }
}
