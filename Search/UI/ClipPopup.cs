using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media.Imaging;
using SearchKit.Field;
using Windows.System;

namespace Search;

/// Ctrl+Shift+V: what you copied, newest first with the pinned ones on top,
/// a field to narrow it, and Enter to put one where the caret was. Built
/// like the field's list — the same plate, the same rows, the same wash on
/// the row the arrows are on — so it reads as the same kind of thing.
///
/// A secret shows only what kind of secret it is until its Show is clicked,
/// and is never found by what it says.
public sealed partial class ClipPopup : Grid
{
    /// Rows drawn at most; typing finds the rest.
    private const int Most = 30;

    private readonly Browser browser;
    private readonly Hunt hunt = new("Search what you copied");
    private readonly StackPanel list = new() { Spacing = 1 };
    private readonly ScrollViewer scroller;
    private readonly TextBlock foot = Kit.Text("", 11, Palette.Muted);
    private readonly HashSet<long> shown = [];
    private List<ClipEntry> rows = [];
    private int picked;
    /// Whether the arrows have moved: until they have, the newest entry is
    /// the one Enter takes, whatever arrives while the list is up.
    private bool walked;
    /// The pointer is over the list, and when a key was last pressed in it:
    /// what arrives then waits, so rows don't move under the pointer or
    /// under a finger about to press Enter.
    private bool pointerOver;
    private long keyedAt;
    private bool stale;
    private bool waiting;

    /// Pictures' thumbnails, by entry: decoded once, not on every redraw,
    /// and let go of once the entry is gone (removed, cleared, history off).
    private static readonly Dictionary<long, BitmapImage> thumbnails = [];

    /// How many thumbnails are cached right now — a bench readout, so a test
    /// world can see the cache actually drop when its entries do.
    public static int CachedThumbnails => thumbnails.Count;

    static ClipPopup() => ClipHistory.Kept += history => UI.Do(() => Prune(history));

    private static void Prune(List<ClipEntry> history)
    {
        if (thumbnails.Count == 0) return;
        var kept = history.Select(e => e.Id).ToHashSet();
        foreach (var gone in thumbnails.Keys.Where(id => !kept.Contains(id)).ToList()) thumbnails.Remove(gone);
    }

    public ClipPopup(Browser browser)
    {
        this.browser = browser;
        Width = 560;
        HorizontalAlignment = HorizontalAlignment.Center;
        VerticalAlignment = VerticalAlignment.Top;
        Margin = new Thickness(0, 86, 0, 0);
        CornerRadius = new CornerRadius(14);
        Background = Palette.Ground;
        BorderBrush = Palette.Hairline;
        BorderThickness = new Thickness(1);
        Kit.Lift(this, 40);

        var stack = new StackPanel();
        hunt.Margin = new Thickness(8, 8, 8, 6);
        stack.Children.Add(hunt);
        if (!browser.Prefs.ClipboardTold) stack.Children.Add(Told());
        scroller = Parts.Scroller(list, 400, bar: true);
        scroller.Padding = new Thickness(6, 0, 6, 6);
        stack.Children.Add(scroller);
        stack.Children.Add(new Rule(0));
        foot.Margin = new Thickness(14, 8, 14, 9);
        foot.TextWrapping = TextWrapping.Wrap;
        stack.Children.Add(foot);
        Children.Add(stack);

        hunt.Changed += _ =>
        {
            keyedAt = Environment.TickCount64;
            picked = 0;
            walked = false;
            Draw();
        };
        hunt.Field.PreviewKeyDown += OnKey;
        PointerEntered += (_, _) => pointerOver = true;
        PointerExited += (_, _) =>
        {
            pointerOver = false;
            WhenStill();
        };
        void changed() => _ = Reload();
        Loaded += (_, _) =>
        {
            ClipHistory.Changed += changed;
            UI.Soon(() => hunt.Field.Focus(FocusState.Programmatic));
        };
        Unloaded += (_, _) => ClipHistory.Changed -= changed;

        Draw();
        _ = Reload();
    }

    /// The rows as the engine has them now — drawn once the list is still.
    private async Task Reload()
    {
        await ClipHistory.List();
        if (!IsLoaded && Parent == null) return;
        stale = true;
        WhenStill();
    }

    /// Redrawn for what arrived, unless the pointer rests on the list or a
    /// key was just pressed; then a moment later, or when the pointer leaves.
    private void WhenStill()
    {
        if (!stale || waiting) return;
        if (ClipGuard.MayRedraw(pointerOver, Environment.TickCount64 - keyedAt))
        {
            stale = false;
            Draw();
            return;
        }
        if (pointerOver) return;
        waiting = true;
        UI.After(ClipGuard.StillMs / 1000.0, () =>
        {
            waiting = false;
            if (Parent != null) WhenStill();
        });
    }

    /// The one-time line: what Search keeps, where it's listed, and where to
    /// turn it off.
    private FrameworkElement Told()
    {
        browser.Prefs.ClipboardTold = true;
        var line = new Grid { ColumnSpacing = 10, Margin = new Thickness(14, 0, 10, 8) };
        line.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        line.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var words = Kit.Text("Search keeps what you copy, encrypted on this PC: here with Ctrl+Shift+V, and clip: in the field. Turn it off in Settings › Clipboard.", 11.5, Palette.Muted);
        words.TextWrapping = TextWrapping.Wrap;
        words.TextTrimming = TextTrimming.None;
        line.Children.Add(words);
        var settings = new Quick("Settings", () =>
        {
            browser.HideClipboard(handBack: false);
            Store.Settings.Set("settings.page", "clipboard");
            browser.Tuning = true;
        });
        settings.VerticalAlignment = VerticalAlignment.Center;
        Grid.SetColumn(settings, 1);
        line.Children.Add(settings);
        return line;
    }

    /// Each drawn row's way of showing it's the one picked, or not.
    private readonly List<Action<bool>> marks = [];

    /// How long the last drawing of the list took, for the bench.
    public double DrawMs { get; private set; }

    private void Draw()
    {
        var clock = System.Diagnostics.Stopwatch.StartNew();
        marks.Clear();
        var was = walked && picked < rows.Count ? rows[picked].Id : (long?)null;
        var all = ClipList.Filter(ClipHistory.Last, hunt.Field.Text);
        rows = all.Count > Most ? all.GetRange(0, Most) : all;
        // The row the arrows were on stays the one they're on.
        var at = was is { } id ? rows.FindIndex(e => e.Id == id) : -1;
        if (at >= 0) picked = at;
        picked = Math.Clamp(picked, 0, Math.Max(0, rows.Count - 1));
        list.Children.Clear();
        if (rows.Count == 0)
        {
            list.Children.Add(Nothing.Make(ClipHistory.Paused ? "Paused — nothing new is being kept"
                : hunt.Field.Text.Trim().Length > 0 ? "Nothing copied matches that"
                : "Nothing copied yet. What you copy from now on shows up here."));
        }
        for (var i = 0; i < rows.Count; i++) list.Children.Add(Row(rows[i], i));
        if (all.Count > rows.Count) list.Children.Add(Nothing.Make($"{all.Count - rows.Count} more — type to find them"));
        foot.Text = (browser.ClipAim switch
        {
            "field" or "page" => "Enter pastes as plain text",
            _ => "Enter copies",
        }) + " · Shift+Enter goes there or searches · Ctrl+P pins · Shift+Delete removes";
        DrawMs = clock.Elapsed.TotalMilliseconds;
    }

    private UIElement Row(ClipEntry entry, int index)
    {
        var row = new Press { Height = 44 };
        var ground = Kit.Rounded(9, index == picked ? Palette.Wash : null);
        row.Children.Add(ground);
        var line = new Grid { ColumnSpacing = 10, Padding = new Thickness(10, 0, 8, 0) };
        line.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(28) });
        line.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        line.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });

        line.Children.Add(Mark(entry));

        var words = new StackPanel { Spacing = 1, VerticalAlignment = VerticalAlignment.Center };
        var reveal = entry.Sensitive && shown.Contains(entry.Id);
        words.Children.Add(Kit.Text(reveal ? entry.Shown : entry.Face, 13, entry.Sensitive && !reveal ? Palette.Muted : null));
        words.Children.Add(Kit.Text(Detail(entry), 11, Palette.Muted));
        Grid.SetColumn(words, 1);
        line.Children.Add(words);

        // Show (a secret), pin, remove: there while the row is pointed at or
        // walked to, gone otherwise, so the list reads as a list. Made the
        // first time they're wanted: a list of plain rows draws in half the time.
        StackPanel? tools = null;
        void offer(bool on)
        {
            if (on && tools == null)
            {
                tools = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 2, VerticalAlignment = VerticalAlignment.Center };
                if (entry.Sensitive)
                    tools.Children.Add(new Quick(reveal ? "Hide" : "Show", () =>
                    {
                        if (!shown.Remove(entry.Id)) shown.Add(entry.Id);
                        Draw();
                    }));
                tools.Children.Add(new Door(entry.Pinned ? Icons.Unpin : Icons.Pin, entry.Pinned ? "Unpin   Ctrl+P" : "Pin   Ctrl+P",
                    () => ClipHistory.Pin(entry.Id, !entry.Pinned), 24) { On = entry.Pinned });
                tools.Children.Add(new Door(Icons.Delete, "Remove   Shift+Delete", () => ClipHistory.Delete(entry.Id), 24));
                Grid.SetColumn(tools, 2);
                line.Children.Add(tools);
            }
            if (tools != null) tools.Opacity = on ? 1 : 0;
        }
        offer(index == picked);
        row.Children.Add(line);

        row.Hovered += on =>
        {
            if (index == picked) return;
            ground.Background = on ? Palette.Hover : null;
            offer(on);
        };
        row.Clicked += _ => browser.PasteClip(entry);
        marks.Add(on =>
        {
            ground.Background = on ? Palette.Wash : row.IsHovering ? Palette.Hover : null;
            offer(on || row.IsHovering);
        });
        return row;
    }

    /// A picture's own thumbnail; a glyph for everything else.
    private static FrameworkElement Mark(ClipEntry entry)
    {
        if (entry.Image && Thumbnail(entry) is { } thumbnail)
            return new Image
            {
                Source = thumbnail,
                Height = 28,
                Width = 28,
                Stretch = Microsoft.UI.Xaml.Media.Stretch.UniformToFill,
                VerticalAlignment = VerticalAlignment.Center,
            };
        // Pinned wears the pin; a secret the lock, whether pinned or not.
        var glyph = Icons.Make(entry.Sensitive ? Icons.Lock : entry.Pinned ? Icons.Pin : entry.Image ? Icons.Picture : Icons.Clipboard, 12,
            entry.Pinned && !entry.Sensitive ? Palette.Ink : null);
        glyph.HorizontalAlignment = HorizontalAlignment.Center;
        glyph.VerticalAlignment = VerticalAlignment.Center;
        return glyph;
    }

    /// Decoded once per entry and kept while the entry is; looked for on
    /// disk only the first time.
    private static BitmapImage? Thumbnail(ClipEntry entry)
    {
        if (thumbnails.TryGetValue(entry.Id, out var known)) return known;
        if (entry.Thumbnail.Length == 0 || !File.Exists(entry.Thumbnail)) return null;
        if (thumbnails.Count >= 64) Prune(ClipHistory.Last);
        return thumbnails[entry.Id] = new BitmapImage(new Uri(entry.Thumbnail)) { DecodePixelHeight = 64 };
    }

    /// The label a pinned entry was given; otherwise the app it came from
    /// and when.
    private static string Detail(ClipEntry entry)
    {
        if (entry.Label.Length > 0) return entry.Label;
        var app = entry.From.EndsWith(".exe", StringComparison.OrdinalIgnoreCase) ? entry.From[..^4] : entry.From;
        var when = entry.CapturedMs > 0 ? When.Said(DateTimeOffset.FromUnixTimeMilliseconds(entry.CapturedMs).UtcDateTime) : "";
        var parts = new[] { app, when, entry.Sensitive ? "forgotten in a few minutes" : "" }.Where(p => p.Length > 0);
        return string.Join(" · ", parts);
    }

    private void OnKey(object sender, KeyRoutedEventArgs e)
    {
        var shift = Keys.Down(Keys.Shift);
        var ctrl = Keys.Down(Keys.Control);
        keyedAt = Environment.TickCount64;
        switch (e.Key)
        {
            case VirtualKey.Down:
            case VirtualKey.Up:
                Walk(e.Key == VirtualKey.Down ? 1 : -1);
                e.Handled = true;
                break;
            case VirtualKey.Tab:
                Walk(shift ? -1 : 1);
                e.Handled = true;
                break;
            case VirtualKey.Enter:
                if (picked < rows.Count) browser.PasteClip(rows[picked], go: shift);
                e.Handled = true;
                break;
            case VirtualKey.P when ctrl:
                if (picked < rows.Count) ClipHistory.Pin(rows[picked].Id, !rows[picked].Pinned);
                e.Handled = true;
                break;
            case VirtualKey.Delete when shift:
                if (picked < rows.Count) ClipHistory.Delete(rows[picked].Id);
                e.Handled = true;
                break;
        }
    }

    private void Walk(int step)
    {
        if (rows.Count == 0 || marks.Count != rows.Count) return;
        walked = true;
        // Only the two rows that change are touched, not the whole list.
        marks[picked](false);
        picked = (picked + step + rows.Count) % rows.Count;
        marks[picked](true);
        if (list.Children[picked] is FrameworkElement row) row.StartBringIntoView(new BringIntoViewOptions { AnimationDesired = false });
    }

    /// What the bench sees: the rows as drawn — a secret as its kind unless
    /// shown — the one picked, and where Enter puts it.
    public (IReadOnlyList<(string Row, long CapturedMs)> Rows, int Picked) Seen =>
        ([.. rows.Select(e => (e.Sensitive && !shown.Contains(e.Id) ? e.Face : e.Shown, e.CapturedMs))], picked);

    public void Type(string text) => hunt.Field.Text = text;

    public void Enter(bool go = false)
    {
        if (picked < rows.Count) browser.PasteClip(rows[picked], go);
    }

    public void Step(int step) => Walk(step);
}
