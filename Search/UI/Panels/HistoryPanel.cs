using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;

namespace Search;

/// Every site you have been to, searchable. PORT: Recall.swift (HistoryPanel).
public sealed class HistoryPanel : Plate
{
    public HistoryPanel(Browser browser) : base("History", Nothing.Make("Nothing yet."), () => browser.Recalling = false) { }
}
