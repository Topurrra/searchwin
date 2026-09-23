namespace Search;

// Video that follows you: lifted out of the page into a small window that
// stays above everything.
//
// PORT: see Sources/Search/Float.swift and the lift/land parts of Browser.swift.
public sealed partial class Browser
{
    private Guid? floating;
    /// The tab whose video is currently out in the little window.
    public Guid? Floating { get => floating; private set => Set(ref floating, value); }

    /// Ctrl+Shift+P, for lifting one out by hand.
    public void ToggleFloat() { }

    private void Lift(Tab? tab, bool quietly) { }

    public void Land() { }
}

public static class Float
{
    public static void Start(Browser browser) { }
}
