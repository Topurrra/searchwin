using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;

namespace Search;

/// What you have downloaded. PORT: Recall.swift (DownloadsPanel).
public sealed class DownloadsPanel : Plate
{
    public DownloadsPanel(Browser browser) : base("Downloads", Nothing.Make("Nothing yet."), () => browser.Hoarding = false) { }
}
