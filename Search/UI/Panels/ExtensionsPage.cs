using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;

namespace Search;

/// Settings › Extensions. PORT: ExtensionsUI.swift (ExtensionsPage).
public sealed class ExtensionsPage : StackPanel
{
    public ExtensionsPage(Browser browser) { Children.Add(Nothing.Make("No extensions yet.")); }
}
