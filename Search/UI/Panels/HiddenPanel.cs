using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;

namespace Search;

/// What is hidden on this site. PORT: Hidden.swift.
public sealed class HiddenPanel : Plate
{
    public HiddenPanel(Browser browser) : base("Hidden here", Nothing.Make("Nothing hidden on this site."), () => browser.Reviewing = false, width: 320) { }
}
