using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;

namespace Search;

/// Everything there is to set. PORT: Settings.swift.
public sealed class SettingsPanel : Plate
{
    public SettingsPanel(Browser browser) : base("Settings", new ExtensionsPage(browser), () => browser.Tuning = false, width: 660) { }
}
