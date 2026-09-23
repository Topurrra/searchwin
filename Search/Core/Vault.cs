using System.Globalization;
using System.Runtime.InteropServices;
using System.Text;
using Login = Search.Browser.Login;

namespace Search;

// Where passwords live: Windows' own Credential Manager, under this app's own
// name, as generic credentials keyed by site and account. Nothing is written
// to disk by this app in any other form, and nothing is ever logged.
//
// This is the same coffer Edge's Windows sign-ins are in, but not the same
// drawer: Edge and Chrome keep their passwords in files of their own, locked
// to themselves. So these are Search's — in the system's vault, unlocked with
// your Windows account, shown with Windows Hello.
public static class Vault
{
    /// What every item of ours is named with. A test run names its own, so a
    /// password saved while trying something never sits among the real ones.
    private static readonly string Prefix = Store.World is { } w ? $"Search ({w}):" : "Search:";

    // MARK: - reading

    /// What is kept for a host, exactly. See `LoginsMatching` for the version
    /// that also looks across a site's subdomains.
    public static List<Login> LoginsFor(string host) =>
        Rows().Where(l => l.Host == host).ToList();

    /// Credential Manager matches a name exactly, and a sign-in rarely lives
    /// on the page you saved it from — accounts.example.com asks, and the
    /// password was kept for example.com. So the site is matched as a site:
    /// the host first, then anything sharing its registrable domain.
    public static List<Login> LoginsMatching(string host)
    {
        var domain = Registrable(host);
        var rows = Rows();
        var exact = rows.Where(l => l.Host == host);
        var wider = rows.Where(l => l.Host != host && Registrable(l.Host) == domain);
        return exact.Concat(wider).OrderByDescending(l => l.Used ?? DateTime.MinValue).ToList();
    }

    /// Everything this app holds, for the list. Read on demand and never kept
    /// in a property.
    public static List<Login> All() =>
        Rows().OrderBy(l => l.Host, StringComparer.Ordinal).ThenBy(l => l.User, StringComparer.Ordinal).ToList();

    /// Every item of ours, secrets and all: Credential Manager, unlike the
    /// keychain, hands over the list and the secrets in one call.
    private static List<Login> Rows()
    {
        var found = new List<Login>();
        if (!CredEnumerate(Prefix + "*", 0, out var count, out var list))
        {
            // Nothing kept reads as "not found"; anything else is worth a
            // line in the debug log, because the panel will only say "nothing".
            var error = Marshal.GetLastWin32Error();
            if (error != NotFound) System.Diagnostics.Debug.WriteLine($"Vault: list failed ({error})");
            return found;
        }
        try
        {
            for (var i = 0; i < count; i++)
            {
                var item = Marshal.PtrToStructure<Credential>(Marshal.ReadIntPtr(list, i * IntPtr.Size));
                if (Read(item) is { } login) found.Add(login);
            }
        }
        finally { CredFree(list); }
        return found;
    }

    private static Login? Read(Credential item)
    {
        if (item.Type != Generic) return null;
        var target = Marshal.PtrToStringUni(item.TargetName) ?? "";
        if (!target.StartsWith(Prefix, StringComparison.Ordinal)) return null;
        // The name says which site; the account rides in the credential's
        // own user name, whole, whatever characters it has.
        var rest = target[Prefix.Length..];
        var colon = rest.IndexOf(':');
        var host = colon < 0 ? rest : rest[..colon];
        var user = Marshal.PtrToStringUni(item.UserName) ?? "";
        if (host.Length == 0 || item.CredentialBlob == IntPtr.Zero || item.CredentialBlobSize == 0) return null;
        var bytes = new byte[item.CredentialBlobSize];
        Marshal.Copy(item.CredentialBlob, bytes, 0, bytes.Length);
        var password = Encoding.Unicode.GetString(bytes);
        Array.Clear(bytes);
        // Credential Manager has no "last used" of its own; it rides in the
        // comment.
        DateTime? used = double.TryParse(Marshal.PtrToStringUni(item.Comment), NumberStyles.Float, CultureInfo.InvariantCulture, out var stamp)
            ? DateTime.UnixEpoch.AddSeconds(stamp)
            : null;
        return new Login(host, user, password, used);
    }

    private static string Target(string host, string user) => $"{Prefix}{host}:{user}";

    // MARK: - writing

    public static bool Save(string host, string user, string password, DateTime? used = null)
    {
        if (host.Length == 0 || password.Length == 0) return false;
        // Saving over an item without saying when it was used keeps the date
        // it had, the way updating a keychain item leaves its comment alone.
        var comment = used is { } when
            ? (when - DateTime.UnixEpoch).TotalSeconds.ToString("0.###", CultureInfo.InvariantCulture)
            : Comment(host, user);

        var secret = Encoding.Unicode.GetBytes(password);
        if (secret.Length > MaxBlob) return false;
        var target = Marshal.StringToCoTaskMemUni(Target(host, user));
        var name = Marshal.StringToCoTaskMemUni(user);
        var note = comment == null ? IntPtr.Zero : Marshal.StringToCoTaskMemUni(comment);
        var blob = Marshal.AllocCoTaskMem(Math.Max(1, secret.Length));
        try
        {
            Marshal.Copy(secret, 0, blob, secret.Length);
            var item = new Credential
            {
                Type = Generic,
                TargetName = target,
                Comment = note,
                CredentialBlobSize = (uint)secret.Length,
                CredentialBlob = blob,
                // This machine only: nothing here is meant to follow you
                // anywhere, the same rule as the rest of what Search keeps.
                Persist = LocalMachine,
                UserName = name,
            };
            return CredWrite(ref item, 0);
        }
        finally
        {
            Array.Clear(secret);
            for (var i = 0; i < secret.Length; i++) Marshal.WriteByte(blob, i, 0);
            Marshal.FreeCoTaskMem(blob);
            Marshal.FreeCoTaskMem(target);
            Marshal.FreeCoTaskMem(name);
            if (note != IntPtr.Zero) Marshal.FreeCoTaskMem(note);
        }
    }

    private static string? Comment(string host, string user)
    {
        if (!CredRead(Target(host, user), Generic, 0, out var found)) return null;
        try { return Marshal.PtrToStringUni(Marshal.PtrToStructure<Credential>(found).Comment); }
        finally { CredFree(found); }
    }

    /// It was just used to sign in. Lists put it first from now on.
    public static void Touch(Login login) => Save(login.Host, login.User, login.Password, DateTime.UtcNow);

    public static void Forget(string host, string user) => CredDelete(Target(host, user), Generic, 0);

    // MARK: - sites that asked not to be asked

    private const string NeverKey = "passwords.never";

    /// The sites told never to offer to save. A fresh set on every read:
    /// change it by setting it whole, or through `NeverFor`.
    public static HashSet<string> Never
    {
        get => [.. Store.Settings.Strings(NeverKey)];
        set => Store.Settings.Set(NeverKey, value.Order(StringComparer.Ordinal).ToList());
    }

    public static void NeverFor(string host)
    {
        var never = Never;
        if (never.Add(host)) Never = never;
    }

    public static bool IsNever(string host)
    {
        var never = Never;
        return never.Contains(host) || never.Contains(Registrable(host));
    }

    // MARK: - showing one

    /// A password is shown only to the person the PC belongs to. Windows
    /// Hello — a face, a finger, the PIN — whatever Windows itself takes.
    public static async Task<bool> Prove(string reason)
    {
        try
        {
            var availability = await Windows.Security.Credentials.UI.UserConsentVerifier.CheckAvailabilityAsync();
            if (availability != Windows.Security.Credentials.UI.UserConsentVerifierAvailability.Available)
            {
                // No way to ask at all — Windows Hello was never set up. Then
                // there is nothing to prove.
                return true;
            }
            // A desktop window has to be named, or the question comes up
            // behind it.
            var window = App.Window is { } w ? WinRT.Interop.WindowNative.GetWindowHandle(w) : IntPtr.Zero;
            var result = await Windows.Security.Credentials.UI.UserConsentVerifierInterop.RequestVerificationForWindowAsync(window, reason);
            return result == Windows.Security.Credentials.UI.UserConsentVerificationResult.Verified;
        }
        catch
        {
            // Asking itself failed — no Hello service at all on this edition.
            // The same as never having set it up.
            return true;
        }
    }

    // MARK: - the site behind a host

    private static readonly HashSet<string> Seconds = ["co", "com", "org", "net", "gov", "gouv", "ac", "edu", "asso", "or", "ne"];

    /// example.com for www.example.com and accounts.example.com; bbc.co.uk
    /// stays bbc.co.uk. The handful of two-part endings that matter here are
    /// listed; a full public suffix list would be a library for a corner.
    public static string Registrable(string host)
    {
        var labels = host.ToLowerInvariant().Split('.', StringSplitOptions.RemoveEmptyEntries);
        if (labels.Length <= 2) return string.Join('.', labels);
        if (Seconds.Contains(labels[^2]) && labels[^1].Length == 2) return string.Join('.', labels[^3..]);
        return string.Join('.', labels[^2..]);
    }

    public static string Host(string text)
    {
        var value = text.Trim();
        if (!value.Contains("://")) value = "https://" + value;
        if (!Uri.TryCreate(value, UriKind.Absolute, out var url) || string.IsNullOrEmpty(url.Host)) return "";
        var host = url.IdnHost.ToLowerInvariant();
        return host.StartsWith("www.") ? host[4..] : host;
    }

    // MARK: - taking in an export

    /// A CSV as Google Password Manager, Chrome or Edge write it: name, url,
    /// username, password, note. Read once, put in Credential Manager, and the
    /// file is yours to delete — this never keeps a copy of it.
    public static (int Kept, int Skipped) Take(string csv)
    {
        var rows = Parse(csv);
        if (rows.Count == 0) return (0, 0);

        var header = rows[0].Select(h => h.Trim().ToLowerInvariant()).ToList();
        rows.RemoveAt(0);
        int Column(params string[] names) => header.FindIndex(names.Contains);
        var urlAt = Column("url", "login_uri", "website", "site");
        var userAt = Column("username", "login_username", "user", "email");
        var passAt = Column("password", "login_password");
        if (urlAt < 0 || userAt < 0 || passAt < 0) return (0, rows.Count);

        int kept = 0, skipped = 0;
        var widest = Math.Max(urlAt, Math.Max(userAt, passAt));
        foreach (var row in rows)
        {
            if (row.Count <= widest)
            {
                skipped++;
                continue;
            }
            var host = Host(row[urlAt]);
            var password = row[passAt];
            if (host.Length == 0 || password.Length == 0)
            {
                skipped++;
                continue;
            }
            if (Save(host, row[userAt], password)) kept++; else skipped++;
        }
        return (kept, skipped);
    }

    /// Quoted fields, doubled quotes inside them, and newlines inside those —
    /// all three turn up in a real export.
    private static List<List<string>> Parse(string text)
    {
        var rows = new List<List<string>>();
        var row = new List<string>();
        var field = new StringBuilder();
        var quoted = false;

        void EndRow()
        {
            row.Add(field.ToString());
            field.Clear();
            if (row.Any(f => f.Length > 0)) rows.Add(row);
            row = [];
        }

        for (var i = 0; i < text.Length; i++)
        {
            var c = text[i];
            if (quoted)
            {
                if (c == '"')
                {
                    if (i + 1 < text.Length && text[i + 1] == '"')
                    {
                        field.Append('"');
                        i++;
                    }
                    else quoted = false;
                }
                else field.Append(c);
                continue;
            }
            switch (c)
            {
                case '"': quoted = true; break;
                case ',': row.Add(field.ToString()); field.Clear(); break;
                case '\r':
                    if (i + 1 < text.Length && text[i + 1] == '\n') i++;
                    EndRow();
                    break;
                case '\n': EndRow(); break;
                case '﻿' when i == 0: break; // the mark some exports start with
                default: field.Append(c); break;
            }
        }
        EndRow();
        return rows;
    }

    // MARK: - Credential Manager

    private const uint Generic = 1;          // CRED_TYPE_GENERIC
    private const uint LocalMachine = 2;     // CRED_PERSIST_LOCAL_MACHINE
    private const int NotFound = 1168;       // ERROR_NOT_FOUND
    private const int MaxBlob = 5 * 512;     // CRED_MAX_CREDENTIAL_BLOB_SIZE

    [StructLayout(LayoutKind.Sequential)]
    private struct Credential
    {
        public uint Flags;
        public uint Type;
        public IntPtr TargetName;
        public IntPtr Comment;
        public System.Runtime.InteropServices.ComTypes.FILETIME LastWritten;
        public uint CredentialBlobSize;
        public IntPtr CredentialBlob;
        public uint Persist;
        public uint AttributeCount;
        public IntPtr Attributes;
        public IntPtr TargetAlias;
        public IntPtr UserName;
    }

    [DllImport("advapi32.dll", EntryPoint = "CredWriteW", SetLastError = true)]
    private static extern bool CredWrite(ref Credential credential, uint flags);

    [DllImport("advapi32.dll", EntryPoint = "CredReadW", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool CredRead(string target, uint type, uint flags, out IntPtr credential);

    [DllImport("advapi32.dll", EntryPoint = "CredEnumerateW", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool CredEnumerate(string filter, uint flags, out int count, out IntPtr credentials);

    [DllImport("advapi32.dll", EntryPoint = "CredDeleteW", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool CredDelete(string target, uint type, uint flags);

    [DllImport("advapi32.dll")]
    private static extern void CredFree(IntPtr buffer);
}
