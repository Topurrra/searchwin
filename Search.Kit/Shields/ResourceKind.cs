namespace SearchKit.Shields;

/// What a request is for. The caller (the browser's `WebResourceRequested`
/// handler) already knows this from WebView2's own resource-context enum;
/// this is Shields' own copy so the filter engine doesn't depend on WebView2
/// at all, which is what keeps it testable off Windows.
public enum ResourceKind
{
    Document,
    Subdocument,
    Script,
    Image,
    Stylesheet,
    XmlHttpRequest,
    Media,
    Font,
    Other,
}

/// The same nine kinds as flags, so a rule can say "only these" ($script) or
/// "all but these" — and so "no type option at all" is simply `All`, one
/// value instead of a null check at every call site.
[Flags]
public enum ResourceKinds
{
    None = 0,
    Document = 1 << 0,
    Subdocument = 1 << 1,
    Script = 1 << 2,
    Image = 1 << 3,
    Stylesheet = 1 << 4,
    XmlHttpRequest = 1 << 5,
    Media = 1 << 6,
    Font = 1 << 7,
    Other = 1 << 8,
    All = Document | Subdocument | Script | Image | Stylesheet | XmlHttpRequest | Media | Font | Other,
}
