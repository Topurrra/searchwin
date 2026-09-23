using System.Runtime.InteropServices;
using Microsoft.UI.Input;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Windows.Foundation;

namespace Search;

/// Anything in the chrome that can be pressed: a tab, a door, a row. One
/// piece of pointer handling for all of them, so they agree on what a click
/// is, what a drag is, and that neither waits for the other.
///
/// A click is answered on release, at once — never after the system's
/// double-click delay. Something that also wants a double click says so and
/// then gets only that (see the Mac's OneClick for why never both).
public class Press : Grid
{
    /// A plain click (clicks = 1) or, for something that asked for them,
    /// a double one (clicks = 2).
    public event Action<int>? Clicked;
    public event Action? MiddleClicked;
    public event Action<bool>? Hovered;
    /// A drag begun: where the pointer went down, in the element's own space.
    public event Action<Point>? DragBegan;
    /// Where the drag has got to, relative to where it began, in the
    /// coordinate space of `DragSpace` (the element's own if unset).
    public event Action<Point>? DragMoved;
    public event Action? DragEnded;

    /// Whether this answers a double click instead of a single one.
    public bool WantsDouble { get; set; }
    /// Whether dragging does anything. Without it a press that wanders is
    /// still a click.
    public bool Draggable { get; set; }
    /// Where drag distances are measured. The row's space, not the tab's: a
    /// tab that has just been moved to a new slot would otherwise report the
    /// drag from where it now is, and shuttle between two slots.
    public UIElement? DragSpace { get; set; }

    public bool IsHovering { get; private set; }

    private Point? down;
    private Point downInSpace;
    private bool dragging;
    private bool middle;
    private DateTime lastClick;
    private Point lastClickAt;

    [DllImport("user32.dll")] private static extern uint GetDoubleClickTime();

    public Press()
    {
        Background = Palette.Clear; // hit-testable everywhere, not just on its ink
        PointerEntered += (_, e) => SetHover(true);
        PointerExited += (_, e) => { if (!dragging) SetHover(false); };
        PointerCanceled += (_, _) => Reset();
        PointerCaptureLost += (_, _) => { if (dragging) EndDrag(); Reset(); };
        PointerPressed += OnPressed;
        PointerMoved += OnMoved;
        PointerReleased += OnReleased;
    }

    private void SetHover(bool on)
    {
        if (IsHovering == on) return;
        IsHovering = on;
        Hovered?.Invoke(on);
    }

    private void OnPressed(object sender, PointerRoutedEventArgs e)
    {
        var point = e.GetCurrentPoint(this);
        var props = point.Properties;
        if (props.IsMiddleButtonPressed)
        {
            middle = true;
            CapturePointer(e.Pointer);
            e.Handled = true;
            return;
        }
        if (!props.IsLeftButtonPressed) return;
        down = point.Position;
        downInSpace = e.GetCurrentPoint(DragSpace ?? this).Position;
        dragging = false;
        CapturePointer(e.Pointer);
        e.Handled = true;
    }

    private void OnMoved(object sender, PointerRoutedEventArgs e)
    {
        if (down is not { } start || !Draggable) return;
        var here = e.GetCurrentPoint(DragSpace ?? this).Position;
        var dx = here.X - downInSpace.X;
        var dy = here.Y - downInSpace.Y;
        if (!dragging)
        {
            // A little slack, so a shaky click is still a click.
            if (Math.Abs(dx) < 5 && Math.Abs(dy) < 5) return;
            dragging = true;
            DragBegan?.Invoke(start);
        }
        DragMoved?.Invoke(new Point(dx, dy));
        e.Handled = true;
    }

    private void OnReleased(object sender, PointerRoutedEventArgs e)
    {
        var point = e.GetCurrentPoint(this).Position;
        var inside = point.X >= 0 && point.Y >= 0 && point.X <= ActualWidth && point.Y <= ActualHeight;
        if (middle)
        {
            middle = false;
            ReleasePointerCapture(e.Pointer);
            // On the release, not the press, and only if still over it: a
            // middle button pressed by mistake can be taken back.
            if (inside) MiddleClicked?.Invoke();
            e.Handled = true;
            return;
        }
        if (down == null) return;
        var wasDragging = dragging;
        Reset();
        ReleasePointerCapture(e.Pointer);
        e.Handled = true;
        if (wasDragging)
        {
            EndDrag();
            if (!inside) SetHover(false);
            return;
        }
        if (!inside) return;

        var now = DateTime.UtcNow;
        var isDouble = (now - lastClick).TotalMilliseconds <= GetDoubleClickTime()
            && Math.Abs(point.X - lastClickAt.X) < 6 && Math.Abs(point.Y - lastClickAt.Y) < 6;
        lastClick = isDouble ? DateTime.MinValue : now;
        lastClickAt = point;
        if (WantsDouble)
        {
            if (isDouble) Clicked?.Invoke(2);
        }
        else
        {
            Clicked?.Invoke(1);
        }
    }

    private void EndDrag()
    {
        dragging = false;
        DragEnded?.Invoke();
    }

    private void Reset()
    {
        down = null;
        dragging = false;
    }

    /// The pointer, as the arrow or the resize cursor.
    public void SetCursor(InputSystemCursorShape shape) => ProtectedCursor = InputSystemCursor.Create(shape);
}
