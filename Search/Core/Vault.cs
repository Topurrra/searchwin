namespace Search;

/// Passwords, in Windows' own Credential Manager. PORT: Sources/Search/Vault.swift.
public static class Vault
{
    /// The sites told never to offer to save.
    public static HashSet<string> Never { get; set; } = [];
}
