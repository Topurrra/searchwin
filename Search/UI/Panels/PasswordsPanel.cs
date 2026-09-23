using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;

namespace Search;

/// The passwords kept, by site. PORT: Passwords.swift, Import.swift.
public sealed class PasswordsPanel : Plate
{
    public PasswordsPanel(Browser browser) : base("Passwords", Nothing.Make("None kept yet."), () => browser.Managing = false) { }
}
