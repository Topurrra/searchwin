using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;

namespace Search;

/// The accounts kept for the site, hanging from its sign-in box. PORT: Accounts.swift.
public sealed class AccountList : Grid
{
    public AccountList(Browser browser) { IsHitTestVisible = false; }
}
