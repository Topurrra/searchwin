namespace Search;

/// The Chrome Web Store's own "Add to Search" button. PORT: StoreRelay.swift, Store.swift.
public static class StoreRelay
{
    public const string Script = "";
}

public sealed partial class Browser
{
    /// The store page's button, told what is installed. PORT: StoreRelay.swift.
    private void TellStore(Tab tab) { }
}
