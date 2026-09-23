using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Windows.System;
using Login = Search.Browser.Login;

namespace Search;

/// Every password kept, by site. The same white-and-hairline panel as the
/// rest, and the same rule: a password is never shown until you have proved
/// you are you, and never for longer than it takes to read it.
///
/// PORT: Passwords.swift, and the import foot of it (Import.swift).
public sealed partial class PasswordsPanel : Plate
{
    private readonly Browser browser;
    private readonly StackPanel body = new() { Spacing = 14 };
    private readonly Hunt hunt = new("Search sites and accounts");
    private string? open;
    private bool adding;

    public PasswordsPanel(Browser browser) : this(browser, new StackPanel()) { }

    private PasswordsPanel(Browser browser, StackPanel host)
        : base("Passwords", host, () => browser.Managing = false, Foot(browser), width: 620)
    {
        this.browser = browser;
        host.Children.Add(body);

        hunt.Changed += text => { browser.Hunting = text; Repaint(); };
        browser.On(nameof(Browser.Saved), Repaint);

        Fill();
        UI.Soon(() => hunt.Field.Focus(FocusState.Programmatic));
    }

    private void Fill()
    {
        body.Children.Clear();

        var top = new Grid { ColumnSpacing = 10 };
        top.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        top.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        // The same field every time, so what is typed in it survives a
        // redraw — let go of by the row it was in before being put in this one.
        (hunt.Parent as Panel)?.Children.Remove(hunt);
        top.Children.Add(hunt);
        var add = new Pill(adding ? "Cancel" : "Add", ToggleAdd, filled: !adding);
        Grid.SetColumn(add, 1);
        top.Children.Add(add);
        body.Children.Add(top);

        if (adding) body.Children.Add(new AddForm(browser, () => ToggleAdd()));

        var sites = browser.ShownSites;
        if (sites.Count == 0)
        {
            var card = new Card();
            card.Add(Nothing.Make(browser.Saved.Count == 0
                ? "Nothing kept yet. Sign in somewhere and say yes, or bring yours in below."
                : "Nothing matches."));
            body.Children.Add(card);
            return;
        }

        var list = new StackPanel();
        var box = new Card();
        for (var i = 0; i < sites.Count; i++)
            box.Add(new SiteRow(browser, sites[i], open == sites[i].Host, host => { open = open == host ? null : host; Repaint(); }));
        list.Children.Add(box);
        var scroll = new ScrollViewer
        {
            Content = list,
            MaxHeight = 400,
            HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled,
            VerticalScrollBarVisibility = ScrollBarVisibility.Auto,
        };
        body.Children.Add(scroll);
    }

    private void Repaint() => Fill();

    private void ToggleAdd()
    {
        adding = !adding;
        Fill();
    }

    /// The foot: where to bring passwords in from, and how many are kept. Built
    /// static so the base's constructor has it; it keeps its own live pieces.
    private static UIElement Foot(Browser browser)
    {
        var stack = new StackPanel { Spacing = 10 };

        var importRow = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8, VerticalAlignment = VerticalAlignment.Center };
        importRow.Children.Add(Kit.Text("Bring in from", 12, Palette.Muted));
        foreach (var source in Chromium.Installed())
            importRow.Children.Add(new Pill(source.Name, () => Bring(browser, source)));
        importRow.Children.Add(new Pill("CSV file…", browser.ImportPasswords));
        stack.Children.Add(importRow);

        var note = Kit.Text("", 11.5, Palette.Muted);
        note.TextWrapping = TextWrapping.Wrap;
        note.MaxLines = 3;
        void Count() => note.Text = browser.ImportNote ?? (browser.Saved.Count == 1 ? "1 password" : $"{browser.Saved.Count} passwords");
        browser.On(nameof(Browser.ImportNote), Count);
        browser.On(nameof(Browser.Saved), Count);
        Count();
        stack.Children.Add(note);

        var caveat = Kit.Text(
            "Windows hands over that browser's bookmarks and history. Passwords a browser keeps locked to itself: export them as a CSV from its own settings and import that.",
            11.5, Palette.Muted);
        caveat.TextWrapping = TextWrapping.Wrap;
        stack.Children.Add(caveat);
        return stack;
    }

    /// A browser's bookmarks and history straight away; its passwords the way
    /// it hands them over — its own CSV. Said plainly, rather than half-failing
    /// on the locked ones.
    private static void Bring(Browser browser, Chromium.Source source)
    {
        browser.TakeBookmarks(source);
        browser.TakePlaces(source, _ => { });
        browser.ImportNote = Chromium.ExportHint(source) + ".";
    }

    // MARK: - a site, and its accounts once opened

    private sealed partial class SiteRow : StackPanel
    {
        public SiteRow(Browser browser, Browser.SiteRow site, bool open, Action<string> toggle)
        {
            var head = new Press { Padding = new Thickness(14, 10, 14, 10) };
            head.Background = open ? Palette.Brush(Tone.Wash, 0.6) : Palette.Clear;
            var line = new Grid { ColumnSpacing = 10 };
            line.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            line.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            line.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            line.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });

            var mark = new Mark(16);
            mark.Show(Favicons.Shared.Cached(site.Host), site.Host.Length > 0 ? char.ToUpperInvariant(site.Host[0]).ToString() : "•");
            line.Children.Add(mark);

            var host = Kit.Text(site.Host, 13);
            Grid.SetColumn(host, 1);
            line.Children.Add(host);

            var aside = site.Logins.Count > 1
                ? Kit.Text($"{site.Logins.Count} accounts", 11.5, Palette.Muted)
                : !open && site.Logins.Count == 1 && site.Logins[0].User.Length > 0
                    ? Kit.Text(site.Logins[0].User, 11.5, Palette.Muted)
                    : null;
            if (aside != null) { Grid.SetColumn(aside, 2); aside.Margin = new Thickness(4, 0, 0, 0); line.Children.Add(aside); }

            var chevron = Icons.Make(open ? Icons.Down : Icons.Right, 9, Palette.Muted);
            Grid.SetColumn(chevron, 3);
            line.Children.Add(chevron);
            head.Children.Add(line);
            head.Hovered += on => { if (!open) head.Background = on ? Palette.Hover : Palette.Clear; };
            head.Clicked += _ => toggle(site.Host);
            Children.Add(head);

            if (!open) return;
            var accounts = new StackPanel { Padding = new Thickness(30, 4, 6, 4) };
            foreach (var login in site.Logins) accounts.Children.Add(new AccountRow(browser, login));
            Children.Add(accounts);
        }
    }

    /// One account: the name, the password as dots, and the three things to
    /// do with it. Show asks Windows who you are first.
    private sealed partial class AccountRow : Press
    {
        private readonly Login login;
        private readonly TextBlock secret;
        private readonly StackPanel actions;
        private readonly Toggle show;
        private bool shown;
        private Later? hide;

        public AccountRow(Browser browser, Login login)
        {
            this.login = login;
            Padding = new Thickness(10, 7, 10, 7);
            CornerRadius = new CornerRadius(8);
            var line = new Grid { ColumnSpacing = 10 };
            line.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(140) });
            line.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            line.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });

            var name = Kit.Text(login.User.Length == 0 ? "No username" : login.User, 12, login.User.Length == 0 ? Palette.Faint : Palette.Ink);
            line.Children.Add(name);

            secret = Kit.Text(Dots(), 10, Palette.Muted);
            secret.FontFamily = new Microsoft.UI.Xaml.Media.FontFamily("Consolas, Cascadia Mono");
            Grid.SetColumn(secret, 1);
            line.Children.Add(secret);

            actions = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6, VerticalAlignment = VerticalAlignment.Center, Visibility = Visibility.Collapsed };
            show = new Toggle("Show", Flip);
            actions.Children.Add(show);
            actions.Children.Add(new Toggle("Copy", () => browser.Copy(login)));
            actions.Children.Add(new Toggle("Remove", () => browser.Forget(login), Palette.Brush(Tone.Red, 0.75)));
            Grid.SetColumn(actions, 2);
            line.Children.Add(actions);
            Children.Add(line);

            Hovered += on => { Background = on ? Palette.Hover : Palette.Clear; if (!shown) actions.Visibility = on ? Visibility.Visible : Visibility.Collapsed; };
            Unloaded += (_, _) => Conceal();
        }

        private void Flip()
        {
            if (shown) Conceal(); else Reveal();
        }

        private string Dots() => new('•', Math.Clamp(login.Password.Length, 6, 12));

        private async void Reveal()
        {
            if (!await Vault.Prove($"Show the password for {login.Host}")) return;
            shown = true;
            actions.Visibility = Visibility.Visible;
            secret.Text = login.Password;
            secret.Foreground = Palette.Ink;
            secret.FontSize = 12.5;
            show.Text = "Hide";
            // Long enough to read or type across, and not a minute more.
            hide?.Cancel();
            hide = UI.After(15, Conceal);
        }

        private void Conceal()
        {
            hide?.Cancel();
            shown = false;
            secret.Text = Dots();
            secret.Foreground = Palette.Muted;
            secret.FontSize = 10;
            show.Text = "Show";
            if (!IsHovering) actions.Visibility = Visibility.Collapsed;
        }
    }

    /// A small text action inside a row — Show/Hide, Copy, Remove — with a
    /// label that can change (Kit's Quick can't, and lives in a shared file).
    private sealed partial class Toggle : Press
    {
        private readonly TextBlock label;

        public Toggle(string title, Action act, Microsoft.UI.Xaml.Media.Brush? tint = null)
        {
            var ground = Kit.Rounded(10, Palette.Wash);
            label = Kit.Text(title, 11.5, tint ?? Palette.Ink);
            label.Margin = new Thickness(8, 4, 8, 4);
            ground.Child = label;
            Children.Add(ground);
            VerticalAlignment = VerticalAlignment.Center;
            Hovered += on => ground.Background = on ? Palette.Hover : Palette.Wash;
            Clicked += _ => act();
        }

        public string Text { get => label.Text; set => label.Text = value; }
    }

    /// Typing one in by hand. The site, the name, the password, and Save, in
    /// the same hairline box the list uses.
    private sealed partial class AddForm : Grid
    {
        public AddForm(Browser browser, Action done)
        {
            CornerRadius = new CornerRadius(11);
            Background = Palette.Ground;
            BorderBrush = Palette.Hairline;
            BorderThickness = new Thickness(1);
            var pad = new StackPanel { Spacing = 8, Padding = new Thickness(14) };
            var site = Boxed("Site", out var siteField);
            var user = Boxed("Username", out var userField);
            var top = new Grid { ColumnSpacing = 8 };
            top.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            top.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            top.Children.Add(site);
            Grid.SetColumn(user, 1);
            top.Children.Add(user);
            pad.Children.Add(top);

            var pass = new PasswordBox
            {
                PlaceholderText = "Password",
                Background = Palette.Wash,
                BorderThickness = new Thickness(0),
                CornerRadius = new CornerRadius(9),
                Padding = new Thickness(10, 7, 10, 7),
                FontFamily = new Microsoft.UI.Xaml.Media.FontFamily("Consolas, Cascadia Mono"),
            };
            var bottom = new Grid { ColumnSpacing = 8 };
            bottom.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            bottom.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            bottom.Children.Add(pass);
            void Keep()
            {
                var host = Vault.Host(siteField.Text);
                if (host.Length == 0 || pass.Password.Length == 0) return;
                browser.Keep(host, userField.Text.Trim(), pass.Password);
                done();
            }
            var save = new Pill("Save", Keep, filled: true);
            Grid.SetColumn(save, 1);
            bottom.Children.Add(save);
            pass.KeyDown += (_, e) => { if (e.Key == VirtualKey.Enter) { Keep(); e.Handled = true; } };
            pad.Children.Add(bottom);

            Children.Add(pad);
            UI.Soon(() => siteField.Focus(FocusState.Programmatic));
        }

        private static Grid Boxed(string placeholder, out TextBox field)
        {
            field = Kit.Field(12.5);
            var held = Kit.Placeheld(field, placeholder, 12.5);
            held.Background = Palette.Wash;
            held.CornerRadius = new CornerRadius(9);
            held.Padding = new Thickness(10, 7, 10, 7);
            return held;
        }
    }
}
