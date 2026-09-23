using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Input;

namespace Search;

/// Spaces in the column: two fingers sideways go from one to the next, as
/// in Arc. Past the last space the column offers to make a new one, in
/// place (see NewSpaceCard). The space's icon at the column's foot turns
/// over as it goes (see SpaceDot).
///
/// Windows hands a touchpad's sideways swipe — and a mouse wheel tilted —
/// to the app as horizontal wheel turns, with no beginning or end to the
/// gesture and no glide after it. So a gesture here is a run of those turns
/// over the column, ended by a short quiet; and unlike the Mac the rows do
/// not follow the fingers across — the space changes once the swipe is
/// long enough, and the dot turns over to say so.
public sealed class SpaceSwipe
{
    public static readonly SpaceSwipe Shared = new();

    private Browser? browser;
    private enum Axis { Undecided, Across, Along }
    private Axis axis = Axis.Undecided;
    private bool tracking;
    /// The turns still arriving after a swipe that was taken, which are
    /// taken too rather than starting another.
    private bool gliding;
    private double gatheredX, gatheredY;
    private Later? quiet;

    /// How far the fingers have to go for the next space to come.
    public const double Enough = 50;
    /// A notch of a wheel is 120; a touchpad's swipe comes as many small
    /// turns adding up to several notches. Half a point a unit makes one
    /// notch of a tilted wheel a space, and a short swipe a space too.
    private const double PerUnit = 0.5;
    /// How long without a turn ends a gesture.
    private const double Pause = 0.14;

    /// The column listens from the moment it is made; the gesture is only
    /// taken while there are spaces and the column is out.
    public void Attach(UIElement column, Browser browser)
    {
        this.browser = browser;
        column.AddHandler(UIElement.PointerWheelChangedEvent, new PointerEventHandler((_, e) => Wheel(column, e)), true);
    }

    public void Start(Browser browser) => this.browser ??= browser;

    private void Wheel(UIElement column, PointerRoutedEventArgs e)
    {
        if (browser is not { } b || !b.Prefs.UsesSpaces || !b.Prefs.Sidebar || (b.Folded && !b.Peeking)) return;
        var props = e.GetCurrentPoint(column).Properties;
        var delta = props.MouseWheelDelta * PerUnit;
        if (gliding)
        {
            if (props.IsHorizontalMouseWheel) e.Handled = true;
            Settle(() => gliding = false);
            return;
        }
        // Only a gesture that sets off sideways; one that starts as a scroll
        // of the rows stays theirs until it has been quiet a moment.
        if (!tracking)
        {
            if (!props.IsHorizontalMouseWheel) return;
            Began();
        }
        // A wheel turned to the right shows what is to the right, as fingers
        // to the left do on a touchpad: the travel is the other way round.
        var taken = props.IsHorizontalMouseWheel ? Moved(-delta, 0) : Moved(0, -delta);
        if (taken) e.Handled = true;
        Settle(() => Ended());
    }

    private void Settle(Action then)
    {
        quiet?.Cancel();
        quiet = UI.After(Pause, then);
    }

    // MARK: - the gesture, apart from where its turns come from (the bench drives these)

    public void Began()
    {
        tracking = true;
        axis = Axis.Undecided;
        gatheredX = 0;
        gatheredY = 0;
    }

    /// True while the gesture is this one's to take.
    public bool Moved(double dx, double dy)
    {
        if (!tracking || browser is not { } b) return false;
        gatheredX += dx;
        gatheredY += dy;
        if (axis == Axis.Undecided)
        {
            if (Math.Abs(gatheredX) + Math.Abs(gatheredY) <= 6) return false;
            axis = Math.Abs(gatheredX) > Math.Abs(gatheredY) * 1.5 ? Axis.Across : Axis.Along;
        }
        if (axis != Axis.Across) return false;
        b.SwipeTravel = Resisted(gatheredX, b);
        return true;
    }

    public void Ended(bool cancelled = false)
    {
        var was = tracking;
        tracking = false;
        if (!was || browser is not { } b || axis != Axis.Across) return;
        b.SwipeTravel = 0;
        var here = Here(b);
        // Fingers to the left bring what is to the right.
        var target = cancelled || Math.Abs(gatheredX) < Enough ? here : here + (gatheredX < 0 ? 1 : -1);
        if (target == here || target < 0 || target > b.Spaces.Count) return;
        gliding = true;
        Settle(() => gliding = false);
        Slide(b, target, here);
    }

    /// Where the space on screen sits among them: one past the last while
    /// the card for a new one is up.
    private static int Here(Browser b) =>
        b.MakingSpace ? b.Spaces.Count : Math.Max(0, b.Spaces.FindIndex(s => s.Id == b.SpaceID));

    /// Nothing that way: the travel gives a little, and no more.
    private static double Resisted(double travel, Browser b)
    {
        var here = Here(b);
        var blocked = (travel > 0 && here == 0) || (travel < 0 && here == b.Spaces.Count);
        return blocked ? travel / 4 : travel;
    }

    /// The next space on screen, or one past the last the card for a new one.
    private static void Slide(Browser b, int target, int here)
    {
        b.SpaceStep = target > here ? 1 : -1;
        if (target == b.Spaces.Count)
        {
            b.MakingSpace = true;
        }
        else
        {
            b.MakingSpace = false;
            b.SwitchSpace(b.Spaces[target].Id);
        }
    }
}
