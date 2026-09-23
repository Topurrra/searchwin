using Windows.System;

namespace Search;

/// Every key the browser answers to, in one place.
///
/// The Mac's Command is Windows' Ctrl, so ⌘T is Ctrl+T, ⌘L is Ctrl+L and so
/// on down the list; its Control, for walking the tabs and the spaces, is
/// Ctrl+Tab and Alt. Where Windows has its own key for the same thing — F5,
/// Alt+Left, Ctrl+Tab, F12 — that works too, so nobody's hands have to learn
/// this browser.
///
/// A page takes most of the keyboard while it has focus, so these are caught
/// before the page sees them, from the window and from the page alike.
public static class Shortcuts
{
    public static Browser Browser { get; set; } = null!;

    /// Answers whether the key was the browser's. `repeat` is a key held down.
    public static bool Take(VirtualKey key, bool ctrl, bool shift, bool alt, bool repeat, bool inPage)
    {
        var b = Browser;

        // Escape puts the page back. On a blank tab there is no page to put
        // back, so it belongs to whatever else wants it.
        if (key == VirtualKey.Escape && !ctrl && !alt)
        {
            if (b.EditingTab != null) { b.CancelTabEdit(); return true; }
            if (b.MakingSpace) { b.MakingSpace = false; return true; }
            if (b.Tuning) { b.Tuning = false; return true; }
            if (b.Bookmarking) { b.Bookmarking = false; return true; }
            if (b.Managing) { b.Managing = false; return true; }
            if (b.Recalling) { b.Recalling = false; return true; }
            if (b.Hoarding) { b.Hoarding = false; return true; }
            if (b.Suggesting != null) { b.DropChoice(); return true; }
            if (b.Veiling) { b.ToggleHiding(); return true; }
            if (b.Reviewing) { b.Reviewing = false; return true; }
            if (b.Finding) { b.CloseFind(); return true; }
            // One step at a time: the list first, then the field.
            if (b.Picked != null) { b.Picked = null; return true; }
            if (b.Editing && b.Active?.IsBlank == false) { b.Dismiss(); return true; }
            // A page still coming stops, as Esc does in every browser.
            if (inPage && b.Active is { Loading: true } loading) { loading.Stop(); return false; }
            return false;
        }

        // Tab is the page's: it moves between a form's fields and a page's
        // links, as in every browser. Ctrl+Tab walks the row and comes round
        // to the first again, Ctrl+Shift+Tab the other way.
        if (key == VirtualKey.Tab && !alt)
        {
            if (ctrl) { b.Step(shift ? -1 : 1); return true; }
            return false;
        }
        if (ctrl && !alt && key is VirtualKey.PageDown or VirtualKey.PageUp)
        {
            b.Step(key == VirtualKey.PageDown ? 1 : -1);
            return true;
        }

        // Windows' own keys for what every browser does.
        if (!ctrl && !alt && !shift)
        {
            switch (key)
            {
                case VirtualKey.F5: b.Reload(); return true;
                case VirtualKey.F6: b.Edit(); return true;
                case VirtualKey.F11: App.ToggleFullScreen(); return true;
                case VirtualKey.F12: Inspector.Toggle(b); return true;
                case VirtualKey.F3: if (b.Finding) { b.Look(true); return true; } b.OpenFind(); return true;
            }
        }
        if (shift && !ctrl && !alt && key == VirtualKey.F3) { b.Look(false); return true; }
        if (alt && !ctrl && !shift)
        {
            switch (key)
            {
                case VirtualKey.Left: b.Back(); return true;
                case VirtualKey.Right: b.Forward(); return true;
                case VirtualKey.D: b.Edit(); return true;
                case VirtualKey.Home: return false;
            }
            // Alt+1 to Alt+9 go to that space, when there are spaces.
            if (b.Prefs.UsesSpaces && Digit(key) is { } n && n > 0)
            {
                b.SwitchSpace(n - 1);
                return true;
            }
        }

        // An extension's own shortcut, before ours — none of ours use its keys.
        if ((ctrl || alt) && Extensions.Shared.Take(key, ctrl, shift, alt)) return true;

        // Ctrl+Alt: the inspector's three, and passwords.
        if (ctrl && alt)
        {
            switch (key)
            {
                case VirtualKey.I: Inspector.Toggle(b); return true;
                case VirtualKey.J: Inspector.Console(b); return true;
                case VirtualKey.C: Inspector.Element(b); return true;
                case VirtualKey.L: b.Managing = true; return true;
            }
            return false;
        }

        if (!ctrl || alt) return false;

        // Ctrl+1 through Ctrl+9 by the key, not the character it types — on
        // AZERTY the top row types &, é, "… unless shift is held. The ninth is
        // the last tab, however many.
        if (!shift && Digit(key) is { } number)
        {
            if (number == 0) b.ResetZoom();
            else b.Select(number == 9 ? b.Tabs.Count - 1 : number - 1);
            return true;
        }

        switch (key)
        {
            case VirtualKey.T when !shift: b.NewTab(repeat); return true;
            case VirtualKey.T when shift: b.Reopen(); return true;
            case VirtualKey.C when shift: b.CopyAddress(); return true;
            case VirtualKey.D when !shift: b.Duplicate(); return true;
            case VirtualKey.N when shift: b.NewShyTab(); return true;
            case VirtualKey.N when !shift: b.NewTab(repeat); return true;
            case VirtualKey.Y when !shift: b.Recalling = !b.Recalling; return true;
            case VirtualKey.H when !shift: b.Recalling = !b.Recalling; return true;
            case VirtualKey.J: b.Hoarding = !b.Hoarding; return true;
            // Paste and go — but not over a page's own field, where Ctrl+Shift+V
            // is Windows' paste without formatting.
            case VirtualKey.V when shift && !(inPage && b.Active?.Typing == true): b.PasteAndGo(); return true;
            case VirtualKey.P when !shift: b.PrintPage(); return true;
            case VirtualKey.F when !shift: b.OpenFind(); return true;
            case VirtualKey.G: b.Look(!shift); return true;
            case VirtualKey.M when shift: b.PauseMedia(); return true;
            case VirtualKey.P when shift: b.ToggleFloat(); return true;
            case VirtualKey.K when !shift:
                // Held down, Ctrl+K walks the list a step at a time; letting go
                // of Ctrl takes wherever it stopped.
                if (b.Editing && b.Offers.Count > 0 && b.Summoning) b.StepSummon();
                else b.Summon();
                return true;
            case VirtualKey.S when shift: b.ToggleSidebar(); return true;
            case VirtualKey.S when !shift:
                // The strip has nothing to fold; Ctrl+S stays the page's.
                if (!b.Prefs.Sidebar) return false;
                b.ToggleFold();
                return true;
            case VirtualKey.B when shift: b.BookmarkCurrent(); return true;
            case (VirtualKey)188 when !shift: b.Tuning = !b.Tuning; return true; // Ctrl+,
            case VirtualKey.H when shift: b.ToggleHiding(); return true;
            case VirtualKey.U when shift: b.Reviewing = !b.Reviewing; return true;
            case VirtualKey.Z when !shift:
                // Only while pointing. Everywhere else undo belongs to the page.
                if (!b.Veiling) return false;
                b.UndoHiding();
                return true;
            // Ctrl+= arrives as the "=" key or the numpad's plus; both mean bigger.
            case (VirtualKey)187: case VirtualKey.Add: b.Zoom(1.1); return true;
            case (VirtualKey)189: case VirtualKey.Subtract: b.Zoom(1 / 1.1); return true;
            case VirtualKey.W when !shift: if (b.Active is { } tab) b.Close(tab); return true;
            case VirtualKey.F4 when !shift: if (b.Active is { } t4) b.Close(t4); return true;
            case VirtualKey.L when !shift: b.Edit(); return true;
            case VirtualKey.R when !shift: b.Reload(); return true;
            case VirtualKey.R when shift: b.ToggleReader(); return true;
            case VirtualKey.I when shift: Inspector.Toggle(b); return true;
            case (VirtualKey)219: if (shift) b.Step(-1); else b.Back(); return true;   // Ctrl+[
            case (VirtualKey)221: if (shift) b.Step(1); else b.Forward(); return true; // Ctrl+]
        }
        return false;
    }

    /// The top row's keys and the numpad's, by where they sit.
    private static int? Digit(VirtualKey key)
    {
        if (key >= VirtualKey.Number0 && key <= VirtualKey.Number9) return key - VirtualKey.Number0;
        if (key >= VirtualKey.NumberPad0 && key <= VirtualKey.NumberPad9) return key - VirtualKey.NumberPad0;
        return null;
    }

    /// Ctrl let go of ends a Ctrl+K walk, wherever it stopped.
    public static void Released(VirtualKey key)
    {
        if (key is VirtualKey.Control or VirtualKey.LeftControl or VirtualKey.RightControl) Browser.LandSummon();
    }
}

/// The Web Inspector, on the keys Chrome and Edge use: WebView2's own
/// DevTools, in a window of their own.
public static class Inspector
{
    public static void Toggle(Browser browser)
    {
        if (browser.Active is { IsBlank: false } tab) tab.WhenReady(core => core.OpenDevToolsWindow());
    }

    public static void Console(Browser browser) => Toggle(browser);

    public static void Element(Browser browser) => Toggle(browser);
}
