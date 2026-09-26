using System.IO.Compression;
using System.Security.Cryptography;

namespace SearchKit.Packs;

public enum PackState
{
    /// Not on this PC.
    Missing,
    /// The version this Search's manifest names.
    Ready,
    /// Another version: still used, and Settings offers the update.
    Outdated,
}

/// The download didn't match the manifest: nothing was unpacked, and what
/// was installed before is still there.
public sealed class PackVerifyException(string message) : Exception(message);

/// The packs on this PC, one folder each: `<root>\<id>\<version>\`. A
/// version folder only ever appears whole — it's unpacked beside it under
/// a dot name and renamed into place once everything in it is — so any
/// version folder there is a complete one. Dot folders are work in
/// progress, or left behind by a crash, and are swept.
public sealed class PackStore(string root)
{
    public string Root => root;

    /// The version installed, whichever it is, or null.
    public string? Installed(string id)
    {
        var dir = new DirectoryInfo(Path.Combine(root, id));
        if (!dir.Exists) return null;
        return dir.EnumerateDirectories()
            .Where(d => !d.Name.StartsWith('.'))
            .OrderByDescending(d => d.CreationTimeUtc)
            .FirstOrDefault()?.Name;
    }

    public PackState State(Pack pack) => Installed(pack.Id) switch
    {
        null => PackState.Missing,
        var version when version == pack.Version => PackState.Ready,
        _ => PackState.Outdated,
    };

    /// Where the installed version is, or null.
    public string? Folder(string id) => Installed(id) is { } version ? Path.Combine(root, id, version) : null;

    /// Takes the download as it arrives, checks it is exactly the file the
    /// manifest names (its size and SHA-256), unpacks the manifest's files
    /// and only then puts them in place, removing any other version. A
    /// download that doesn't match changes nothing. Returns the folder.
    public async Task<string> InstallAsync(Pack pack, Stream download, Action<long>? progress = null, CancellationToken cancel = default)
    {
        SweepRemoved();
        var home = Path.Combine(root, pack.Id);
        Directory.CreateDirectory(home);
        Sweep(home);
        var token = Guid.NewGuid().ToString("N")[..8];
        var archive = Path.Combine(home, $".download-{token}.zip");
        var staging = Path.Combine(home, $".unpack-{token}");
        try
        {
            await Receive(pack, download, archive, progress, cancel).ConfigureAwait(false);
            Unpack(pack, archive, staging, cancel);
            var target = Path.Combine(home, pack.Version);
            // The same version again (a repair): the old copy steps aside,
            // and comes back if the new one can't take its place.
            var aside = Path.Combine(home, $".old-{token}");
            var again = Directory.Exists(target);
            if (again) Directory.Move(target, aside);
            try { Directory.Move(staging, target); }
            catch
            {
                if (again) Directory.Move(aside, target);
                throw;
            }
            foreach (var other in Directory.EnumerateDirectories(home))
                if (!string.Equals(other, target, StringComparison.OrdinalIgnoreCase)) TryDelete(other);
            return target;
        }
        finally
        {
            TryDelete(archive);
            TryDelete(staging);
        }
    }

    /// Takes the pack off this PC. Renamed out of the way first, so a
    /// delete that stops halfway (a file still in use) never leaves
    /// something that looks installed.
    public void Remove(string id)
    {
        SweepRemoved();
        var home = Path.Combine(root, id);
        if (!Directory.Exists(home)) return;
        var gone = Path.Combine(root, $".removed-{id}-{Guid.NewGuid().ToString("N")[..8]}");
        Directory.Move(home, gone);
        TryDelete(gone);
    }

    private static async Task Receive(Pack pack, Stream download, string archive, Action<long>? progress, CancellationToken cancel)
    {
        using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        long received = 0;
        await using (var file = new FileStream(archive, FileMode.CreateNew, FileAccess.Write, FileShare.None, 1 << 16, useAsync: true))
        {
            var buffer = new byte[1 << 16];
            int read;
            while ((read = await download.ReadAsync(buffer, cancel).ConfigureAwait(false)) > 0)
            {
                received += read;
                if (received > pack.Size) throw new PackVerifyException($"The {pack.Name} download was bigger than it should be, so it wasn't used.");
                hash.AppendData(buffer, 0, read);
                await file.WriteAsync(buffer.AsMemory(0, read), cancel).ConfigureAwait(false);
                progress?.Invoke(received);
            }
        }
        if (received != pack.Size)
            throw new PackVerifyException($"The {pack.Name} download stopped short ({received:N0} of {pack.Size:N0} bytes).");
        if (Convert.ToHexStringLower(hash.GetHashAndReset()) != pack.Sha256)
            throw new PackVerifyException($"The {pack.Name} download isn't the file Search expects (its SHA-256 differs), so it wasn't used.");
    }

    private static void Unpack(Pack pack, string archive, string staging, CancellationToken cancel)
    {
        using var zip = ZipFile.OpenRead(archive);
        var prefix = pack.Folder.Length > 0 ? pack.Folder + "/" : "";
        foreach (var file in pack.Files)
        {
            cancel.ThrowIfCancellationRequested();
            var entry = zip.GetEntry(prefix + file)
                ?? throw new PackVerifyException($"The {pack.Name} download is missing {file}.");
            var to = Path.Combine(staging, file.Replace('/', Path.DirectorySeparatorChar));
            Directory.CreateDirectory(Path.GetDirectoryName(to)!);
            entry.ExtractToFile(to);
        }
    }

    /// Whatever an earlier install left behind.
    private static void Sweep(string home)
    {
        foreach (var dir in Directory.EnumerateDirectories(home, ".*")) TryDelete(dir);
        foreach (var file in Directory.EnumerateFiles(home, ".*")) TryDelete(file);
    }

    /// A crashed or interrupted removal leaves its renamed folder at the
    /// root, outside the per-pack install sweep.
    private void SweepRemoved()
    {
        if (!Directory.Exists(root)) return;
        foreach (var dir in Directory.EnumerateDirectories(root, ".removed-*")) TryDelete(dir);
    }

    private static void TryDelete(string path)
    {
        try
        {
            if (Directory.Exists(path)) Directory.Delete(path, recursive: true);
            else if (File.Exists(path)) File.Delete(path);
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException) { }
    }
}
