using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Animation;
using Windows.System;

namespace Search;

/// One field, in the middle, and the few places it thinks you mean. Type an
/// address and you go there; type words and you search. Type something that
/// is neither and it shivers and says so.
public sealed partial class Omnibox : Grid
{
    /// The field's own height — the 22 of text and 14 of air above and below
    /// it — so the list can sit below it without being stacked with it.
    private const double FieldHeight = 22 + 14 * 2;

    private readonly Browser browser;
    private readonly Border dim = new();
    private readonly Grid column = new() { Width = Metrics.FieldWidth, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center };
    private readonly Border plate;
    private readonly TextBox field;
    private readonly Border list;
    private readonly StackPanel rows = new();
    private bool over;

    /// The last value pushed in from the browser's side, so a change the
    /// field made itself isn't applied back over what is being typed.
    private string synced = "";
    /// A backspace has to be allowed to actually take a letter off. Without
    /// this the field puts the same letter straight back as a completion and
    /// the address can never be shortened.
    private bool deleting;

    public Omnibox(Browser browser)
    {
        this.browser = browser;

        // The page is still there, just out of the way.
        dim.Background = Palette.Brush(Tone.Ground, 0.74);
        dim.Tapped += (_, _) => browser.Dismiss();
        Motion.Fades(dim);
        Children.Add(dim);

        // Lifted a little above centre: dead centre reads as low.
        column.Margin = new Thickness(0, 0, 0, 60);

        // A slow, almost invisible breath under the field. It is the only thing
        // on an empty tab, and a thing that never moves at all reads as a
        // picture of an app rather than an app.
        var breath = Breath();
        column.Children.Add(breath);

        field = Kit.Field(15.5);
        field.Height = 22;
        var held = Kit.Placeheld(field, "Enter a web address", 15.5);
        plate = new Border
        {
            CornerRadius = new CornerRadius(14),
            Background = Palette.Ground,
            BorderBrush = Palette.Hairline,
            BorderThickness = new Thickness(1),
            Padding = new Thickness(22, 14, 22, 14),
            Height = FieldHeight,
            VerticalAlignment = VerticalAlignment.Top,
            Child = held,
        };
        Kit.Lift(plate, 24);
        column.Children.Add(plate);

        // What it thinks you mean. It lives below the field, so arriving or
        // leaving never moves the field.
        list = new Border
        {
            CornerRadius = new CornerRadius(14),
            Background = Palette.Ground,
            BorderBrush = Palette.Hairline,
            BorderThickness = new Thickness(1),
            Padding = new Thickness(6),
            Width = Metrics.FieldWidth,
            Child = rows,
            Visibility = Visibility.Collapsed,
        };
        Kit.Lift(list, 20);
        // On a canvas, which measures its children at their own size and never
        // clips them: the list hangs below a field that is only as tall as
        // itself.
        var hanger = new Canvas { VerticalAlignment = VerticalAlignment.Top, Height = FieldHeight };
        Canvas.SetTop(list, FieldHeight + 8);
        hanger.Children.Add(list);
        column.Children.Add(hanger);
        column.Height = FieldHeight;
        Children.Add(column);

        field.TextChanged += (_, _) => Typed();
        field.PreviewKeyDown += OnKey;

        browser.OnAny(name =>
        {
            switch (name)
            {
                case nameof(Browser.Offers):
                case nameof(Browser.Picked):
                    Rows();
                    Push();
                    break;
                case nameof(Browser.Completed):
                case nameof(Browser.Typed):
                    Push();
                    break;
                case nameof(Browser.FocusRequest):
                    Claim();
                    break;
                case nameof(Browser.Refusals):
                    if (!browser.FieldShowing || browser.EditingTab != null) break;
                    Motion.Shake(plate);
                    plate.BorderBrush = Palette.Brush(Tone.Red, 0.35);
                    break;
            }
        });
    }

    /// Standing on an empty tab, or raised over a page by Ctrl+L.
    public void Show(bool over)
    {
        this.over = over;
        dim.Opacity = over ? 1 : 0;
        dim.IsHitTestVisible = over;
        Push();
        Rows();
        Claim();
    }

    /// The breath: a shape the size of the field, blurred by 26 — the Mac's
    /// `.blur(radius: 26)` — as a drop shadow the compositor draws, growing and
    /// brightening a little and back, over and over. It runs on the compositor
    /// from start to finish: nothing on the UI thread restarts it, so it never
    /// jumps when the field is shown again, and a real blur has no steps to
    /// flicker between the way faint rings of ink did.
    private FrameworkElement Breath()
    {
        var host = new Grid { IsHitTestVisible = false, Height = FieldHeight, VerticalAlignment = VerticalAlignment.Top };
        Microsoft.UI.Composition.SpriteVisual? sprite = null;
        Microsoft.UI.Composition.DropShadow? shadow = null;

        void Colour()
        {
            // The Mac fills the shape with a twentieth of the ink before
            // blurring it; a shadow's colour is its densest point.
            if (shadow != null) shadow.Color = Palette.ColorOf(Tone.Ink, Palette.Dark ? 0.16 : 0.08);
        }

        host.Loaded += (_, _) =>
        {
            if (sprite != null) return;
            var compositor = Microsoft.UI.Xaml.Hosting.ElementCompositionPreview.GetElementVisual(host).Compositor;
            sprite = compositor.CreateSpriteVisual();
            shadow = compositor.CreateDropShadow();
            shadow.BlurRadius = 26 * 2;
            shadow.Offset = new System.Numerics.Vector3(0, 0, 0);
            Colour();
            sprite.Shadow = shadow;
            // The visual follows the host's size, and scales about its middle.
            var size = compositor.CreateExpressionAnimation("host.Size");
            size.SetReferenceParameter("host", Microsoft.UI.Xaml.Hosting.ElementCompositionPreview.GetElementVisual(host));
            sprite.StartAnimation("Size", size);
            var centre = compositor.CreateExpressionAnimation("Vector3(this.Target.Size.X / 2, this.Target.Size.Y / 2, 0)");
            sprite.StartAnimation("CenterPoint", centre);
            Microsoft.UI.Xaml.Hosting.ElementCompositionPreview.SetElementChildVisual(host, sprite);

            // easeInOut over 2.6 seconds, there and back, for ever.
            var ease = compositor.CreateCubicBezierEasingFunction(new(0.42f, 0), new(0.58f, 1));
            var grow = compositor.CreateVector3KeyFrameAnimation();
            grow.InsertKeyFrame(0, new(0.97f, 0.97f, 1));
            grow.InsertKeyFrame(1, new(1.03f, 1.03f, 1), ease);
            grow.Duration = TimeSpan.FromSeconds(2.6);
            grow.Direction = Microsoft.UI.Composition.AnimationDirection.Alternate;
            grow.IterationBehavior = Microsoft.UI.Composition.AnimationIterationBehavior.Forever;
            sprite.StartAnimation("Scale", grow);
            var brighten = compositor.CreateScalarKeyFrameAnimation();
            brighten.InsertKeyFrame(0, 0.65f);
            brighten.InsertKeyFrame(1, 1f, ease);
            brighten.Duration = TimeSpan.FromSeconds(2.6);
            brighten.Direction = Microsoft.UI.Composition.AnimationDirection.Alternate;
            brighten.IterationBehavior = Microsoft.UI.Composition.AnimationIterationBehavior.Forever;
            shadow.StartAnimation("Opacity", brighten);
        };
        Palette.Turned += Colour;
        return host;
    }

    /// Only when something other than typing changed it — Ctrl+L arriving with
    /// an address, a walk through the list, a submit clearing it.
    ///
    /// Comparing against the field's own text instead would undo every
    /// backspace: deleting leaves the field shorter than what the browser still
    /// considers complete, and the next update would type it back in.
    private void Push()
    {
        if (typing) return;
        var want = browser.Completed;
        if (want == synced) return;
        synced = want;
        Put(want);
        SelectFrom(Math.Min(browser.Typed.Length, want.Length));
    }

    /// The field's text, set from here. WinUI tells of a change a moment
    /// later, not while it is being made — so what was set is remembered, and
    /// the change it causes is recognised as ours rather than as typing.
    private void Put(string text)
    {
        if (field.Text == text) return;
        expected = text;
        field.Text = text;
    }

    private string? expected;
    private bool typing;

    /// The part after the caret, shown as selected, so the next keystroke
    /// replaces it and Enter takes it.
    private void SelectFrom(int start)
    {
        var length = field.Text.Length;
        if (start > length) return;
        field.Select(start, length - start);
    }

    private void Typed()
    {
        var text = field.Text;
        if (expected != null)
        {
            var ours = expected;
            expected = null;
            if (text == ours) return;
        }
        plate.BorderBrush = Palette.Hairline;
        typing = true;
        browser.Typed = text;
        typing = false;
        if (deleting || browser.Ending is not { } ending)
        {
            if (deleting) browser.StopCompleting();
            deleting = false;
            synced = browser.Completed;
            return;
        }
        deleting = false;
        Put(text + ending);
        synced = text + ending;
        SelectFrom(text.Length);
    }

    private void OnKey(object sender, KeyRoutedEventArgs e)
    {
        switch (e.Key)
        {
            case VirtualKey.Enter:
                browser.Submit();
                e.Handled = true;
                break;
            case VirtualKey.Down:
                browser.Walk(1);
                e.Handled = true;
                break;
            case VirtualKey.Up:
                browser.Walk(-1);
                e.Handled = true;
                break;
            case VirtualKey.Tab:
                // While an address is being typed, the list under the field is
                // what there is to move through.
                if (browser.Offers.Count > 0)
                {
                    browser.Walk(Keys.Down(Keys.Shift) ? -1 : 1);
                    e.Handled = true;
                }
                break;
            case VirtualKey.Back:
            case VirtualKey.Delete:
                deleting = true;
                break;
            case VirtualKey.Right:
                // At the end of the line: take what is offered.
                if (browser.Ending != null && field.SelectionStart + field.SelectionLength >= field.Text.Length)
                {
                    browser.AcceptEnding();
                    field.Select(field.Text.Length, 0);
                    e.Handled = true;
                }
                break;
        }
    }

    private void Claim()
    {
        if (Visibility != Visibility.Visible) return;
        UI.Soon(() =>
        {
            field.Focus(FocusState.Programmatic);
            field.SelectAll();
        });
    }

    private void Rows()
    {
        rows.Children.Clear();
        var offers = browser.Offers;
        list.Visibility = offers.Count == 0 ? Visibility.Collapsed : Visibility.Visible;
        for (var i = 0; i < offers.Count; i++)
        {
            var offer = offers[i];
            rows.Children.Add(new OfferRow(offer, browser.Picked == i, () => browser.Take(offer)));
        }
    }

    /// One line of the list. Places you have been come with their titles; the
    /// arrow keys' row is washed, the pointer's only hovered.
    private sealed partial class OfferRow : Press
    {
        public OfferRow(Suggestion offer, bool picked, Action take)
        {
            var ground = Kit.Rounded(9, picked ? Palette.Wash : null);
            Children.Add(ground);
            var line = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 10, Padding = new Thickness(12, 9, 12, 9) };
            switch (offer.Kind)
            {
                case SuggestionKind.Search:
                    line.Children.Add(Icons.Make(Icons.Search, 11));
                    break;
                case SuggestionKind.Command:
                    // Something Search does, rather than somewhere to go.
                    line.Children.Add(Icons.Make("\uE756", 11));
                    break;
                case SuggestionKind.Open:
                    // Already open: naming it takes you back to it rather than
                    // opening a second copy.
                    line.Children.Add(new Microsoft.UI.Xaml.Shapes.Ellipse { Width = 5, Height = 5, Fill = Palette.Brush(Tone.Ink, 0.55), Margin = new Thickness(2, 0, 2, 0), VerticalAlignment = VerticalAlignment.Center });
                    break;
            }
            var key = Kit.Text(offer.Key, 13);
            key.MaxWidth = 300;
            line.Children.Add(key);
            if (offer.Title.Length > 0)
            {
                var title = Kit.Text(offer.Title, 12, Palette.Muted);
                title.MaxWidth = 220;
                line.Children.Add(title);
            }
            Children.Add(line);
            Hovered += on => { if (!picked) ground.Background = on ? Palette.Hover : null; };
            Clicked += _ => take();
        }
    }
}
