using System.IO.Compression;
using System.Security.Cryptography;
using SearchKit.Packs;

namespace SearchKit.Tests;

public sealed class PackTests : IDisposable
{
    private readonly string root = Path.Combine(Path.GetTempPath(), "search-packs-" + Guid.NewGuid().ToString("N"));
    private readonly PackStore store;

    public PackTests() => store = new PackStore(root);

    public void Dispose()
    {
        if (Directory.Exists(root)) Directory.Delete(root, recursive: true);
    }

    /// An archive as the publisher ships it: everything in one top folder,
    /// with more in it than the pack keeps.
    private static byte[] Archive(string version, bool withoutProbe = false)
    {
        using var memory = new MemoryStream();
        using (var zip = new ZipArchive(memory, ZipArchiveMode.Create, leaveOpen: true))
        {
            void add(string name, string text)
            {
                using var writer = new StreamWriter(zip.CreateEntry("ffmpeg-build/" + name).Open());
                writer.Write(text);
            }
            add("bin/ffmpeg.exe", "ffmpeg " + version);
            if (!withoutProbe) add("bin/ffprobe.exe", "ffprobe " + version);
            add("bin/ffplay.exe", "not kept");
            add("LICENSE.txt", "LGPL");
        }
        return memory.ToArray();
    }

    private static Pack PackFor(string version, byte[] archive) => new(
        "ffmpeg", "FFmpeg", version, archive.Length, Convert.ToHexStringLower(SHA256.HashData(archive)),
        new Uri("https://example.org/ffmpeg.zip"), "LGPL-3.0-or-later", "a build", new Uri("https://example.org/build"),
        new Uri("https://example.org/source"), "plays things", "ffmpeg-build",
        ["bin/ffmpeg.exe", "bin/ffprobe.exe", "LICENSE.txt"]);

    private async Task<string> Install(Pack pack, byte[] bytes) => await store.InstallAsync(pack, new MemoryStream(bytes));

    private string[] Leftovers() =>
        [.. Directory.EnumerateFileSystemEntries(Path.Combine(root, "ffmpeg")).Select(Path.GetFileName).Where(n => n!.StartsWith('.'))!];

    [Fact]
    public async Task A_verified_download_is_unpacked_into_its_version_folder_keeping_only_the_listed_files()
    {
        var bytes = Archive("1");
        var pack = PackFor("1.0", bytes);

        var folder = await Install(pack, bytes);

        Assert.Equal(Path.Combine(root, "ffmpeg", "1.0"), folder);
        Assert.Equal(PackState.Ready, store.State(pack));
        Assert.Equal("ffmpeg 1", File.ReadAllText(Path.Combine(folder, "bin", "ffmpeg.exe")));
        Assert.True(File.Exists(Path.Combine(folder, "LICENSE.txt")));
        Assert.False(File.Exists(Path.Combine(folder, "bin", "ffplay.exe")));
        Assert.Empty(Leftovers());
    }

    public enum Spoil { Hash, Short, Long, MissingFile }

    [Theory]
    [InlineData(Spoil.Hash)]
    [InlineData(Spoil.Short)]
    [InlineData(Spoil.Long)]
    [InlineData(Spoil.MissingFile)]
    public async Task A_download_that_is_not_the_manifests_file_changes_nothing(Spoil spoil)
    {
        var first = Archive("1");
        await Install(PackFor("1.0", first), first);

        var good = Archive("2", withoutProbe: spoil == Spoil.MissingFile);
        var pack = PackFor("2.0", good);
        var sent = spoil switch
        {
            Spoil.Hash => [.. good.Select((b, i) => i == good.Length / 2 ? (byte)(b ^ 1) : b)],
            Spoil.Short => good[..^10],
            Spoil.Long => [.. good, 0],
            _ => good,
        };

        await Assert.ThrowsAsync<PackVerifyException>(() => Install(pack, sent));

        Assert.Equal("1.0", store.Installed("ffmpeg"));
        Assert.Equal("ffmpeg 1", File.ReadAllText(Path.Combine(root, "ffmpeg", "1.0", "bin", "ffmpeg.exe")));
        Assert.Empty(Leftovers());
    }

    [Fact]
    public async Task An_update_takes_the_old_versions_place_once_it_is_in()
    {
        var old = Archive("1");
        await Install(PackFor("1.0", old), old);
        var bytes = Archive("2");
        var pack = PackFor("2.0", bytes);
        Assert.Equal(PackState.Outdated, store.State(pack));

        await Install(pack, bytes);

        Assert.Equal(PackState.Ready, store.State(pack));
        Assert.False(Directory.Exists(Path.Combine(root, "ffmpeg", "1.0")));
        Assert.Equal("ffmpeg 2", File.ReadAllText(Path.Combine(store.Folder("ffmpeg")!, "bin", "ffmpeg.exe")));
    }

    [Fact]
    public async Task What_a_crash_left_halfway_is_not_an_installed_pack_and_the_next_install_sweeps_it()
    {
        var half = Path.Combine(root, "ffmpeg", ".unpack-deadbeef", "bin");
        Directory.CreateDirectory(half);
        File.WriteAllText(Path.Combine(half, "ffmpeg.exe"), "half");
        var bytes = Archive("1");
        var pack = PackFor("1.0", bytes);
        Assert.Equal(PackState.Missing, store.State(pack));

        await Install(pack, bytes);

        Assert.Empty(Leftovers());
    }

    [Fact]
    public async Task Removing_a_pack_leaves_nothing_of_it()
    {
        var bytes = Archive("1");
        var pack = PackFor("1.0", bytes);
        await Install(pack, bytes);

        store.Remove("ffmpeg");

        Assert.Equal(PackState.Missing, store.State(pack));
        Assert.Empty(Directory.EnumerateFileSystemEntries(root));
    }

    [Fact]
    public void The_manifest_built_into_Search_loads()
    {
        var ffmpeg = PackManifest.Find("ffmpeg");
        Assert.NotNull(ffmpeg);
        Assert.Contains("bin/ffmpeg.exe", ffmpeg.Files);
        Assert.Contains("bin/ffprobe.exe", ffmpeg.Files);
    }

    [Theory]
    [InlineData("version", "\"../../evil\"")]
    [InlineData("folder", "\"C:/Windows\"")]
    [InlineData("files", "[\"../../../evil.exe\"]")]
    [InlineData("files", "[\"bin\\\\..\\\\evil.exe\"]")]
    [InlineData("url", "\"http://example.org/plain.zip\"")]
    [InlineData("sha256", "\"abc\"")]
    public void A_manifest_entry_that_could_unpack_outside_its_folder_or_skip_verification_is_refused(string key, string value)
    {
        var fields = new Dictionary<string, string>
        {
            ["id"] = "\"ffmpeg\"", ["name"] = "\"FFmpeg\"", ["version"] = "\"1.0\"", ["size"] = "10",
            ["sha256"] = "\"" + new string('a', 64) + "\"", ["url"] = "\"https://example.org/f.zip\"",
            ["licence"] = "\"LGPL\"", ["build"] = "\"b\"", ["buildPage"] = "\"https://example.org/b\"",
            ["source"] = "\"https://example.org/s\"", ["enables"] = "\"e\"", ["folder"] = "\"top\"",
            ["files"] = "[\"bin/ffmpeg.exe\"]",
        };
        var good = "{\"packs\":[{" + string.Join(",", fields.Select(f => $"\"{f.Key}\":{f.Value}")) + "}]}";
        Assert.Single(PackManifest.Parse(good));

        fields[key] = value;
        var bad = "{\"packs\":[{" + string.Join(",", fields.Select(f => $"\"{f.Key}\":{f.Value}")) + "}]}";

        Assert.Throws<FormatException>(() => PackManifest.Parse(bad));
    }
}
