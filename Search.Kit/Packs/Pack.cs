using System.Text.Json.Nodes;
using System.Text.RegularExpressions;

namespace SearchKit.Packs;

/// Something Search can add on request (Settings › Packs), described by the
/// manifest Search ships with (packs.json): what it is, which exact build,
/// where it's downloaded from and the SHA-256 it must have. Nothing is
/// checked for updates in the background: a new version arrives with a new
/// Search, whose manifest names it.
public sealed record Pack(
    string Id,
    string Name,
    // The build's own version, also the folder it's unpacked into.
    string Version,
    // Bytes to download.
    long Size,
    string Sha256,
    Uri Download,
    // SPDX, e.g. LGPL-3.0-or-later.
    string Licence,
    // Who built it and how, in a sentence.
    string Build,
    Uri BuildPage,
    // The exact source the build came from.
    Uri Source,
    // What having it does, in a sentence.
    string Enables,
    // The folder at the top of the archive everything sits in, if any.
    string Folder,
    // The files kept from the archive (relative, `/`-separated); nothing
    // else is unpacked.
    IReadOnlyList<string> Files);

public static partial class PackManifest
{
    /// The packs this Search knows, from the manifest built into it.
    public static IReadOnlyList<Pack> Shipped => shipped.Value;

    private static readonly Lazy<IReadOnlyList<Pack>> shipped = new(() =>
    {
        using var stream = typeof(PackManifest).Assembly.GetManifestResourceStream("Packs/packs.json")
            ?? throw new InvalidOperationException("packs.json isn't built in");
        using var reader = new StreamReader(stream);
        return Parse(reader.ReadToEnd());
    });

    public static Pack? Find(string id) => Shipped.FirstOrDefault(p => p.Id == id);

    /// `{"packs": [...]}`. A manifest is Search's own, so anything wrong in
    /// it is refused whole rather than half-used.
    public static IReadOnlyList<Pack> Parse(string json)
    {
        var root = JsonNode.Parse(json) as JsonObject ?? throw Bad("not an object");
        var packs = new List<Pack>();
        foreach (var node in root["packs"] as JsonArray ?? throw Bad("no packs"))
        {
            if (node is not JsonObject item) throw Bad("a pack isn't an object");
            string Text(string key) => item[key] is JsonValue v && v.TryGetValue<string>(out var s) && s.Length > 0
                ? s : throw Bad($"{key} is missing");
            Uri Link(string key) => Uri.TryCreate(Text(key), UriKind.Absolute, out var u) && u.Scheme == Uri.UriSchemeHttps
                ? u : throw Bad($"{key} isn't an https address");

            var id = Text("id");
            if (!Name().IsMatch(id)) throw Bad($"id {id}");
            var version = Text("version");
            if (!Name().IsMatch(version)) throw Bad($"{id}: version {version}");
            var sha = Text("sha256").ToLowerInvariant();
            if (!Sha().IsMatch(sha)) throw Bad($"{id}: sha256");
            var size = item["size"] is JsonValue sv && sv.TryGetValue<long>(out var n) && n > 0 ? n : throw Bad($"{id}: size");
            var folder = item["folder"] is JsonValue fv && fv.TryGetValue<string>(out var f) ? f : "";
            if (folder.Length > 0 && !Safe(folder)) throw Bad($"{id}: folder");
            var files = (item["files"] as JsonArray ?? throw Bad($"{id}: files"))
                .Select(file => file is JsonValue v && v.TryGetValue<string>(out var s) && Safe(s) ? s : throw Bad($"{id}: a file"))
                .ToList();
            if (files.Count == 0) throw Bad($"{id}: no files");
            if (packs.Any(p => p.Id == id)) throw Bad($"{id} twice");
            packs.Add(new Pack(id, Text("name"), version, size, sha, Link("url"), Text("licence"), Text("build"),
                Link("buildPage"), Link("source"), Text("enables"), folder, files));
        }
        return packs;
    }

    /// A relative path that stays where it's put: no drive, no root, no
    /// climbing out.
    private static bool Safe(string path) =>
        path.Length > 0 && !path.Contains('\\') && !path.Contains(':') && !path.StartsWith('/')
        && path.Split('/').All(part => part.Length > 0 && part != "." && part != "..");

    private static FormatException Bad(string what) => new($"packs.json: {what}");

    [GeneratedRegex("^[A-Za-z0-9][A-Za-z0-9._-]*$")]
    private static partial Regex Name();

    [GeneratedRegex("^[0-9a-f]{64}$")]
    private static partial Regex Sha();
}
