using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;

namespace Search;

/// The extensions' buttons, in the strip or the column's foot. PORT: ExtensionsUI.swift.
public sealed class ExtensionSlot : StackPanel
{
    public ExtensionSlot(Browser browser) { Orientation = Orientation.Horizontal; Spacing = Metrics.TabGap; }
}
