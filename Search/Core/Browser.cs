using System.Collections.ObjectModel;
using SearchKit.Commands;
using Windows.ApplicationModel.DataTransfer;

namespace Search;

// Everything the window knows: which tabs exist, which one is showing, and
// whether the address field is up. Small enough to read in one sitting, which
// is the point of a browser with no features.
//
// The features it does have each keep their part of this class in a file of
// their own — Browser.Page for what pages ask of the window, Browser.Passwords,
// Browser.Curtain, Browser.Spaces and so on — the way the Mac app splits its
// Browser across extensions.
public sealed partial class Browser : Model
{
    public static Browser Shared { get; private set; } = null!;

    /// Everything there is to set. Held here so the whole window redraws when
    /// one of them changes.
    public Preferences Prefs { get; } = new();
    public History History { get; } = new();

    /// The row of tabs, in order. Changes to the list itself are told as
    /// "Tabs"; a tab's own changes are the tab's to tell.
    public List<Tab> Tabs { get; private set; } = [];

    private Guid? activeID;
    public Guid? ActiveID
    {
        get => activeID;
        set
        {
            var old = activeID;
            if (!Set(ref activeID, value)) return;
            // The tab just left is the tab just looked at. Whether a tab has
            // gone unwatched long enough to sleep is counted from here.
            if (old is { } o) Tabs.FirstOrDefault(t => t.Id == o)?.Touch();
            foreach (var tab in Tabs) tab.Show(tab.Id == value && !tab.IsBlank && !tab.Asleep && !tab.Floating);
            Tell(nameof(Active));
            Tell(nameof(FieldShowing));
        }
    }

    public Tab? Active => Tabs.FirstOrDefault(t => t.Id == ActiveID);

    // MARK: - the panels and modes, each a flag the window draws from

    private bool tuning, welcoming, bookmarking, bookmarksOpen, recalling, hoarding, managing;
    /// The settings panel.
    public bool Tuning { get => tuning; set => Set(ref tuning, value); }
    /// The first-launch walk-through, over everything.
    public bool Welcoming { get => welcoming; set => Set(ref welcoming, value); }
    /// The full list of bookmarks, for taking things out.
    public bool Bookmarking { get => bookmarking; set => Set(ref bookmarking, value); }
    /// The dropdown off the bookmarks button.
    public bool BookmarksOpen { get => bookmarksOpen; set => Set(ref bookmarksOpen, value); }
    /// History.
    public bool Recalling { get => recalling; set => Set(ref recalling, value); }
    /// Downloads.
    public bool Hoarding { get => hoarding; set => Set(ref hoarding, value); }
    /// Passwords.
    public bool Managing { get => managing; set { if (Set(ref managing, value) && value) Relist(); } }

    private bool folded, peeking;
    /// Ctrl+S: the column folded away, and slid out over the page for a look
    /// while it is (see Fold).
    public bool Folded { get => folded; set => Set(ref folded, value); }
    public bool Peeking { get => peeking; set => Set(ref peeking, value); }

    // MARK: - the address field

    private bool editing;
    /// The address field, raised over a page by Ctrl+L. A blank tab shows it
    /// without being asked — there is nothing else for that tab to show.
    public bool Editing { get => editing; set { if (Set(ref editing, value)) Tell(nameof(FieldShowing)); } }

    public bool FieldShowing => Editing || (Active?.IsBlank ?? true);

    private string typed = "";
    /// What is in the field. Every change re-reads the history, because the
    /// list under the field and the ending inside it are both just answers to
    /// this string.
    public string Typed
    {
        get => typed;
        set
        {
            if (typed == value) { return; }
            typed = value;
            Tell();
            Guess();
        }
    }

    private List<Suggestion> offers = [];
    /// What the field is offering, best first.
    public List<Suggestion> Offers { get => offers; private set { offers = value; Tell(); } }

    private string? ending;
    /// The rest of the best match, drawn selected after the caret. Tab takes it.
    public string? Ending { get => ending; private set => Set(ref ending, value); }

    private int? picked;
    /// Which row the arrow keys have walked to, if any.
    public int? Picked { get => picked; set { if (Set(ref picked, value)) Tell(nameof(Completed)); } }

    private int refusals;
    /// Bumped when what was typed isn't an address and can't be searched for.
    public int Refusals { get => refusals; private set => Set(ref refusals, value); }

    private int focusRequest;
    /// Bumped whenever the cursor should go back into the field.
    public int FocusRequest { get => focusRequest; private set => Set(ref focusRequest, value); }

    private bool summoning;
    /// True while the field is a switcher rather than an address bar. Ctrl+K
    /// asks one question — which of the pages I already have open — and
    /// answering it with somewhere you went last week would be answering a
    /// different one.
    public bool Summoning { get => summoning; private set => Set(ref summoning, value); }
    /// True between the first Ctrl+K and letting go of Ctrl.
    public bool Cycling;

    /// Typed plus whatever the field is quietly finishing for you.
    public string Completed
    {
        get
        {
            if (Picked is { } p && p >= 0 && p < Offers.Count) return Offers[p].Key;
            return Typed + (Ending ?? "");
        }
    }

    // MARK: - saying so

    private string? announcement;
    /// A line that rises from the bottom, says one thing, and leaves.
    public string? Announcement { get => announcement; private set => Set(ref announcement, value); }
    private Later? hush;

    public void Announce(string text)
    {
        Announcement = text;
        hush?.Cancel();
        hush = UI.After(1.7, () => Announcement = null);
    }

    // MARK: - beginning and ending

    public Browser()
    {
        Shared = this;
        Commands.Attach(this);
        Folded = Prefs.SideHides;
        Welcoming = !Prefs.Welcomed;

        // An icon that arrives is put on every tab showing that site, not only
        // the one that happened to ask for it.
        Favicons.Shared.Arrived = (host, image) =>
        {
            foreach (var tab in Tabs.Concat(ParkedTabs))
                if (Address.Host(tab.Address) == host) tab.Icon = image;
        };

        StartFeatures();
        StartSpaces();
        RestoreSession();
        Follow();
        WatchForSleep();
    }

    /// The row of tabs the space on screen had last time, or one empty tab.
    public void RestoreSession()
    {
        var saved = Session.Read(SpaceID);
        var restored = new List<Tab>();
        foreach (var entry in saved.Tabs)
        {
            if (!Uri.TryCreate(entry.Url, UriKind.Absolute, out var url)) continue;
            var tab = new Tab();
            Prepare(tab);
            tab.Restore(url, entry.Title, entry.Name);
            tab.Pin = entry.Pin;
            restored.Add(tab);
        }
        if (restored.Count == 0)
        {
            // A blank tab costs nothing until it is asked for its page. Its
            // view — and with it WebView2's processes — is built a moment
            // after the window is up, so that the first address typed finds
            // everything already running, and the first frame never had to
            // share the CPU with it.
            var tab = new Tab();
            Adopt(tab);
            UI.After(1, () => { if (tab.IsBlank && Tabs.Contains(tab)) _ = tab.Web; });
            return;
        }
        Tabs = restored;
        TellTabs();
        var here = Math.Clamp(saved.Active, 0, Tabs.Count - 1);
        ActiveID = Tabs[here].Id;
        // Only the one you were looking at actually loads.
        Tabs[here].Wake();
        Tabs[here].Show(true);
    }

    /// The few settings that something else has to be told about. The rest
    /// are read where they are used.
    private void Follow()
    {
        Prefs.On(nameof(Preferences.UsesSpaces), () =>
        {
            if (Prefs.UsesSpaces) PreloadSpaces(); else LeaveSpaces();
        });
        Prefs.On(nameof(Preferences.Shielded), () =>
        {
            Shield.Shared.Enabled = Prefs.Shielded;
            Announce(Prefs.Shielded ? "Ads and trackers blocked" : "Blocking off — reload to see the difference");
        });
        Prefs.On(nameof(Preferences.Bench), () =>
        {
            if (Prefs.Bench) Search.Bench.Shared.Start(this); else Search.Bench.Shared.Stop();
            Announce(Prefs.Bench ? "Scripts can drive Search — see bench.ps1" : "The bench is closed");
        });
        Prefs.On(nameof(Preferences.Passkeys), () =>
        {
            FormRelay.PasskeysOffered = Prefs.Passkeys;
            // Each tab keeps whatever is hidden on the site it is showing:
            // re-arming with nothing would quietly restore every element this
            // person had taken off, everywhere.
            foreach (var tab in Tabs) tab.Arm(Curtain.Css(Curtain.Host(tab.Address)), force: true);
            Announce(Prefs.Passkeys ? "Passkeys offered again — reload the page" : "Sites will ask for a password instead");
        });
        Prefs.On(nameof(Preferences.Spelling), () =>
        {
            foreach (var tab in Tabs.Concat(ParkedTabs))
            {
                tab.Arm(Curtain.Css(Curtain.Host(tab.Address)), force: true);
                if (tab.Core != null) tab.Run(Spelling.Now(Prefs.Spelling));
            }
            Announce(Prefs.Spelling ? "Spelling checked as you type" : "Spelling left alone");
        });
        Prefs.On(nameof(Preferences.Sidebar), () =>
        {
            // Back to the strip and then to the column again: the column comes
            // back as it rests (see Fold).
            Folded = Prefs.SideHides;
            Peeking = false;
        });
        Prefs.On(nameof(Preferences.SideHides), () =>
        {
            Peeking = false;
            Folded = Prefs.SideHides;
        });
    }

    public void WriteSession(bool now = false)
    {
        var shape = new Session.Shape
        {
            Tabs = Tabs
                .Where(t => !t.Shy && !t.Bench)
                // A sleeping tab holds its address in Pending; asking for it
                // there too means a pin can never be written out of existence
                // by whatever its view happens to be showing.
                .Select(t => (tab: t, url: t.Pending ?? t.Address))
                .Where(p => Address.IsWeb(p.url))
                .Select(p => new Session.Entry { Url = p.url!.AbsoluteUri, Title = p.tab.Title, Pin = p.tab.Pin, Name = p.tab.Name })
                .ToList(),
            Active = Math.Max(0, Tabs.FindIndex(t => t.Id == ActiveID)),
        };
        Session.Write(shape, SpaceID, now);
    }

    private bool remembering;
    private void RememberSession()
    {
        if (remembering) return;
        remembering = true;
        UI.After(1.2, () =>
        {
            remembering = false;
            WriteSession();
        });
    }

    /// The app is quitting. Whatever the debounce above was waiting out, it
    /// stops waiting.
    public void FlushSession()
    {
        WriteSession(now: true);
        Store.Settings.Flush();
    }

    /// The row changed: added, removed, moved.
    public void TellTabs()
    {
        Tell(nameof(Tabs));
        Tell(nameof(PinnedCount));
    }

    // MARK: - tabs

    public int PinnedCount => Tabs.Count(t => t.Pin != null);

    private DateTime lastNewTab;

    public void NewTab(bool repeat = false)
    {
        // Ctrl+T held down repeats. Each press is a new tab, even beside an
        // empty one, but a key left down is one press, not a row of empty tabs
        // for as long as it stays there.
        if (repeat && Active?.IsBlank == true) return;
        lastNewTab = DateTime.UtcNow;
        if (Extensions.Shared.NewTabPage is { } page)
        {
            Open(page, foreground: true);
            Summoning = false;
            RememberSession();
            return;
        }
        var tab = new Tab();
        Adopt(tab);
        Leaving();
        ActiveID = tab.Id;
        Summoning = false;
        Typed = "";
        Editing = false;
        FocusRequest++;
        RememberSession();
    }

    public void Select(Tab tab)
    {
        CancelTabEdit();
        Summoning = false;
        Suggesting = null;
        if (tab.Id == ActiveID) return;
        // Coming back to the tab whose video is out brings it home first, so
        // it is never lifted and landed in the same breath.
        if (Floating == tab.Id) Land();
        Leaving();
        // A tab brought back from last time, or waking from Ctrl+W while
        // pinned, opens the moment you look at it — and only if there was
        // nothing to wake is this the other case, one whose page quietly died
        // while you were elsewhere.
        var woke = tab.Wake();
        ActiveID = tab.Id;
        tab.Touch();
        if (!woke) tab.Revive();
        RememberSession();
        Editing = false;
        Typed = "";
    }

    /// Ctrl+W, or the cross on the tab. Closing the last one leaves a blank
    /// tab behind; closing that blank tab closes the window.
    public void Close(Tab tab)
    {
        var index = Tabs.IndexOf(tab);
        if (index < 0) return;

        // A tab whose page is out in the little window takes the window with it.
        if (Floating == tab.Id) Land();

        // A pinned tab is not closed by Ctrl+W — it is put down. The letter
        // keeps its place, the page is let go, and you land on whatever you
        // were looking at before. Only Unpin takes it out of the row.
        if (tab.Pin != null)
        {
            tab.Rest();
            // Ordinary tabs first: falling back to the most recent tab of any
            // kind meant closing one pin landed you on another pin, and Ctrl+W
            // bounced between the two instead of getting you out of them.
            var others = Tabs.Where(t => t.Id != tab.Id && !t.Asleep).ToList();
            var loose = others.Where(t => t.Pin == null).ToList();
            var back = (loose.Count == 0 ? others : loose).OrderByDescending(t => t.Touched).FirstOrDefault();
            if (back != null) Select(back);
            else if (Tabs.FirstOrDefault(t => t.Id != tab.Id) is { } asleepPin) Select(asleepPin);
            else NewTab();
            // The pin itself is asleep now; nothing of it stays on the stage.
            tab.Show(false);
            TellTabs();
            WriteSession(now: true);
            return;
        }

        if (Tabs.Count == 1)
        {
            if (tab.IsBlank)
            {
                App.CloseWindow();
            }
            else
            {
                var fresh = new Tab();
                Remember(tab, 0);
                tab.Close();
                Prepare(fresh);
                Tabs = [fresh];
                TellTabs();
                ActiveID = fresh.Id;
                Typed = "";
            }
            return;
        }

        Remember(tab, index);
        tab.Close();
        Tabs.RemoveAt(index);
        TellTabs();
        if (ActiveID == tab.Id)
        {
            // The neighbour on the right, or the last one if there is no right
            // — through Select(), same as everywhere else you land on a tab, so
            // one that was never built yet actually wakes up.
            activeID = null;
            Select(Tabs[Math.Min(index, Tabs.Count - 1)]);
        }
        RememberSession();
    }

    /// Everything but this one. Pinned tabs are put down rather than removed
    /// — they are not open pages so much as places kept.
    public void CloseOthers(Tab keep)
    {
        Select(keep);
        foreach (var tab in Tabs.Where(t => t.Id != keep.Id).ToList()) Close(tab);
        Select(keep);
    }

    /// Tabs you closed, newest last, so Ctrl+Shift+T can put them back where
    /// they were and the History menu can offer them by name.
    public List<Ghost> Ghosts { get; } = [];

    public sealed record Ghost(Uri Url, string Title, int Index)
    {
        public Guid Id { get; } = Guid.NewGuid();
        public string Label => Title.Length == 0 ? Search.Address.Pretty(Url) : Title;
    }

    /// Ctrl+Shift+T. Back into the row at the place it left.
    public void Reopen()
    {
        if (Ghosts.Count > 0) Reopen(Ghosts[^1]);
    }

    public void Reopen(Ghost ghost)
    {
        Ghosts.Remove(ghost);
        Tell(nameof(Ghosts));
        var tab = new Tab();
        Prepare(tab);
        Leaving();
        Tabs.Insert(Math.Min(ghost.Index, Tabs.Count), tab);
        TellTabs();
        tab.Go(ghost.Url);
        ActiveID = tab.Id;
        Editing = false;
        Typed = "";
    }

    private void Remember(Tab tab, int index)
    {
        if (tab.Shy || tab.Address is not { } url) return;
        Ghosts.Add(new Ghost(url, tab.Title, index));
        if (Ghosts.Count > 12) Ghosts.RemoveAt(0);
        Tell(nameof(Ghosts));
    }

    /// Dragged from one place in the row to another.
    public void Move(Tab tab, int index)
    {
        var here = Tabs.IndexOf(tab);
        if (here < 0 || index == here || index < 0 || index >= Tabs.Count) return;
        // The pinned block and the loose one don't mix: a letter that wandered
        // into the middle of the titles would stop meaning anything.
        var pinned = PinnedCount;
        if (tab.Pin != null && index >= pinned) return;
        if (tab.Pin == null && index < pinned) return;
        Tabs.RemoveAt(here);
        Tabs.Insert(index, tab);
        TellTabs();
        RememberSession();
    }

    public void Step(int direction)
    {
        var here = Tabs.FindIndex(t => t.Id == ActiveID);
        if (Tabs.Count < 2 || here < 0) return;
        Select(Tabs[(here + direction + Tabs.Count) % Tabs.Count]);
    }

    public void Select(int index)
    {
        if (index >= 0 && index < Tabs.Count) Select(Tabs[index]);
    }

    /// A link opened from a page lands next to the page it came from, not at
    /// the far end of the row — unless it is one of a batch, which keeps the
    /// order it came in.
    public Tab Open(Uri url, bool foreground, bool atEnd = false, bool shy = false)
    {
        var tab = new Tab(shy: shy);
        Prepare(tab);
        var here = atEnd ? -1 : Tabs.FindIndex(t => t.Id == ActiveID);
        Tabs.Insert(here >= 0 ? here + 1 : Tabs.Count, tab);
        TellTabs();
        tab.Go(url);
        if (foreground)
        {
            Leaving();
            ActiveID = tab.Id;
            Editing = false;
            Typed = "";
        }
        else
        {
            // Built and loading, but hidden until looked at.
            tab.Show(false);
        }
        RememberSession();
        return tab;
    }

    /// A page for the bench: at the end of the row, behind whatever you are
    /// looking at, and marked as not yours.
    public Tab BenchOpen(Uri url)
    {
        var tab = new Tab(bench: true);
        Prepare(tab);
        Tabs.Add(tab);
        TellTabs();
        tab.Go(url);
        tab.Show(false);
        return tab;
    }

    /// A link from another app. A blank tab with nothing typed in it takes the
    /// page rather than staying behind as an empty one; otherwise the page
    /// gets a tab of its own, in front.
    public void Arrive(Uri url)
    {
        if (Active is { IsBlank: true } active && Floating != active.Id && Typed.Length == 0)
        {
            active.Go(url);
            active.Show(true);
            Editing = false;
        }
        else
        {
            Open(url, foreground: true);
        }
    }

    /// A bookmark, or a page from a list of them: into the tab you are on, the
    /// way every bookmarks bar has ever worked — into a new one with Ctrl
    /// held, or when the one you are on is busy playing in the float.
    public void Visit(Uri url, bool apart = false)
    {
        if (Active is { } active && Floating != active.Id && !apart)
        {
            Go(active, url);
            Editing = false;
            Typed = "";
        }
        else
        {
            Open(url, foreground: true);
        }
    }

    /// Ctrl+Shift+N. A tab that keeps nothing — its own cookies, its own
    /// sign-ins, no history, and no place in tomorrow's session.
    public void NewShyTab()
    {
        var tab = new Tab(shy: true);
        Adopt(tab);
        Leaving();
        ActiveID = tab.Id;
        Summoning = false;
        Typed = "";
        Editing = false;
        FocusRequest++;
        Announce("A tab that keeps nothing");
    }

    /// Ctrl+D. The same page, beside itself.
    public void Duplicate()
    {
        if (Active?.Address is { } url) Open(url, foreground: true);
    }

    /// Ctrl+Shift+V. What is in the clipboard, if it is a place — or a search.
    public async void PasteAndGo()
    {
        string? text = null;
        try
        {
            var content = Clipboard.GetContent();
            if (content.Contains(StandardDataFormats.Text)) text = await content.GetTextAsync();
        }
        catch { }
        if (text == null || Google.Destination(text.Trim()) is not { } url)
        {
            Refusals++;
            return;
        }
        if ((Active ?? Tabs.FirstOrDefault()) is { } tab) Go(tab, url);
        Editing = false;
        Typed = "";
    }

    /// Ctrl+Shift+C. The address, in the clipboard, and a line that says so.
    public void CopyAddress()
    {
        if (Active?.Address is not { } url) return;
        Copy(url.AbsoluteUri);
        Announce("Address copied");
    }

    public static void Copy(string text)
    {
        var package = new DataPackage();
        package.SetText(text);
        Clipboard.SetContent(package);
        Clipboard.Flush();
    }

    /// Ctrl+P. Edge's own print preview, which is also where "save as PDF"
    /// lives.
    public void PrintPage()
    {
        if (Active is { IsBlank: false } tab) tab.WhenReady(core => core.ShowPrintUI(Microsoft.Web.WebView2.Core.CoreWebView2PrintDialogKind.Browser));
    }

    private void Adopt(Tab tab)
    {
        Prepare(tab);
        Tabs.Add(tab);
        TellTabs();
        if (ActiveID == null) ActiveID = tab.Id;
    }

    /// Sends a tab somewhere and puts its page on the stage if it is the one
    /// on screen — a blank tab has no page showing until it has somewhere to be.
    public void Go(Tab tab, Uri url)
    {
        tab.Go(url);
        if (tab.Id == ActiveID) tab.Show(true);
    }

    /// Stepping away from a tab. A video you were watching does not stop
    /// existing because you went to look something up.
    private void Leaving() => Lift(Active, quietly: true);

    private void Prepare(Tab tab)
    {
        tab.Host = this;
        tab.OnZoom = (_, value) =>
        {
            // The line at the bottom doubles as the zoom read-out.
            var percent = (int)Math.Round(value * 100);
            Announce($"{percent}%");
        };
        tab.PropertyChanged += (_, e) =>
        {
            switch (e.PropertyName)
            {
                // Anywhere a tab lands is worth remembering for next launch.
                case nameof(Tab.Address):
                    RememberSession();
                    if (tab.Id == ActiveID) Tell(nameof(FieldShowing));
                    break;
                // A page's title lands a beat after the page itself, and a
                // history entry that only ever holds an address is half a
                // memory.
                case nameof(Tab.Title):
                    if (!tab.Shy && !tab.Bench && tab.Address is { } url) History.Retitle(url, tab.Title);
                    break;
                case nameof(Tab.Pin):
                case nameof(Tab.Name):
                    RememberSession();
                    break;
            }
        };
        PrepareFeatures(tab);
    }

    /// Put the cursor back in the field, from wherever asked.
    public void AskFocus() => FocusRequest++;

    // MARK: - pinning

    private Guid? editingPin;
    /// The pinned tab whose letter is being typed over, in place. There is no
    /// dialog: pinning happens at once, with a letter guessed from the
    /// address, and that letter arrives selected so the next keystroke
    /// replaces it.
    public Guid? EditingPin { get => editingPin; set => Set(ref editingPin, value); }

    public void PinTab(Tab tab)
    {
        if (tab.Pin == null)
        {
            tab.Pin = tab.Monogram;
            // Pinned tabs live at the head of the row, in the order they were
            // pinned, so their letters never move under your hand.
            var here = Tabs.IndexOf(tab);
            var home = Math.Max(0, PinnedCount - 1);
            if (here >= 0 && here != home)
            {
                Tabs.RemoveAt(here);
                Tabs.Insert(Math.Min(home, Tabs.Count), tab);
            }
            TellTabs();
        }
        WriteSession(now: true);
    }

    /// Change Letter, or a double-click on the square itself.
    public void EditLetter(Tab tab)
    {
        if (tab.Pin != null) EditingPin = tab.Id;
    }

    /// Typed into the square. Empty leaves the letter as it was — a pinned tab
    /// with nothing on it would be a blank square you could never identify.
    public void Letter(string typed, Tab tab)
    {
        var first = typed.Trim();
        if (first.Length == 0) return;
        tab.Pin = char.ToUpperInvariant(first[0]).ToString();
    }

    public void EndPinEdit()
    {
        if (EditingPin == null) return;
        EditingPin = null;
        WriteSession(now: true);
    }

    public void Unpin(Tab tab)
    {
        if (EditingPin == tab.Id) EditingPin = null;
        tab.Pin = null;
        // Back out of the pinned block, to the head of the loose tabs.
        var here = Tabs.IndexOf(tab);
        var home = PinnedCount;
        if (here >= 0 && here != home)
        {
            Tabs.RemoveAt(here);
            Tabs.Insert(Math.Min(home, Tabs.Count), tab);
        }
        TellTabs();
        WriteSession(now: true);
    }

    // MARK: - the address, in the tab itself

    private Guid? editingTab;
    /// Clicking the tab you are already on turns it into the address, short
    /// form, ready to be changed.
    public Guid? EditingTab { get => editingTab; private set => Set(ref editingTab, value); }
    public string TabDraft { get; set; } = "";
    /// Set while that field is being used to name the tab rather than to go
    /// somewhere: the same field, the same keys, a different thing at the end.
    public bool RenamingTab { get; private set; }

    public void BeginTabEdit(Tab tab)
    {
        if (tab.Address is not { } url) { Edit(); return; }
        RenamingTab = false;
        TabDraft = Address.Pretty(url);
        EditingTab = tab.Id;
    }

    /// Rename. The name the tab is wearing arrives selected, so typing
    /// replaces it; emptying the field gives the page its own title back.
    public void BeginTabRename(Tab tab)
    {
        RenamingTab = true;
        TabDraft = tab.Label;
        EditingTab = tab.Id;
    }

    public void CommitTabEdit()
    {
        if (EditingTab is not { } id || Tabs.FirstOrDefault(t => t.Id == id) is not { } tab) return;
        if (RenamingTab)
        {
            var typed = TabDraft.Trim();
            tab.Name = typed.Length == 0 ? null : typed;
            CancelTabEdit();
            WriteSession(now: true);
            return;
        }
        if (Google.Destination(TabDraft) is not { } url)
        {
            // Stay put and say so, rather than quietly throwing the edit away.
            Refusals++;
            return;
        }
        EditingTab = null;
        Go(tab, url);
    }

    public void CancelTabEdit()
    {
        if (EditingTab == null && !RenamingTab) return;
        EditingTab = null;
        RenamingTab = false;
        TabDraft = "";
    }

    // MARK: - guessing

    /// Ctrl+K again, with Ctrl still down: one step further down the list.
    public void StepSummon()
    {
        Cycling = true;
        Walk(1);
    }

    /// Ctrl let go of: take whatever the walk landed on.
    public void LandSummon()
    {
        if (!Cycling) return;
        Cycling = false;
        if (Picked == null) return;
        Submit();
    }

    /// Ctrl+K. Only what is open, nothing else.
    public void Summon()
    {
        Reviewing = false;
        CancelTabEdit();
        Summoning = true;
        typed = "";
        Tell(nameof(Typed));
        Guess();
        Editing = true;
        FocusRequest++;
    }

    private void Guess()
    {
        if (Summoning)
        {
            Offers = OpenPages(Typed);
            Ending = null;
            // The most recent page is already chosen, so Ctrl+K then Enter is
            // the whole gesture.
            Picked = Offers.Count == 0 ? null : 0;
            Tell(nameof(Completed));
            return;
        }

        if (Typed.Trim().Length == 0)
        {
            Offers = [];
            Ending = null;
            Picked = null;
            Tell(nameof(Completed));
            return;
        }

        // `>…` is a command and `!…` a bang: the list is what they could mean,
        // the first already chosen so Enter does it.
        switch (FieldInput.Read(Typed, Commands.Bangs))
        {
            case FieldInput.ToCommand command:
                Offers = [.. Commands.Registry.Find(command.Query, 6).Select(match => new Suggestion(
                    match.Argument.Length > 0 ? $"{match.Command.Title} “{match.Argument}”" : match.Command.Title,
                    match.Command.Keys ?? match.Command.Group ?? "",
                    Commands.Address(match.Command, match.Argument),
                    SuggestionKind.Command))];
                Ending = null;
                Picked = Offers.Count == 0 ? null : 0;
                Tell(nameof(Completed));
                return;
            case FieldInput.ToBang bang:
                var there = bang.Query.Length == 0 ? bang.Bang.Home() : bang.Bang.For(bang.Query);
                Offers = there == null ? [] : [new Suggestion(bang.Query.Length == 0 ? bang.Bang.Name : bang.Query, bang.Bang.Name, there, SuggestionKind.Search)];
                Ending = null;
                Picked = Offers.Count == 0 ? null : 0;
                Tell(nameof(Completed));
                return;
        }

        // Three places and, if it can't be a place, a search. No open pages:
        // Ctrl+K exists for those.
        var list = History.Suggestions(Typed, 3);
        // Last in the list, and only when what was typed cannot be a place.
        if (Address.Url(Typed) == null && Google.Url(Typed) is { } asked)
            list.Add(new Suggestion(Typed, Google.Name, asked, SuggestionKind.Search));
        Offers = list;
        Ending = History.Completion(Typed, Offers.Where(o => o.Kind != SuggestionKind.Open));
        // A row that was picked stops being the right row the moment the
        // question changes.
        Picked = null;
        Tell(nameof(Completed));
    }

    /// What is open, most recently looked at first, filtered by what has been
    /// typed. On an empty field this is the whole point of the summon: it is
    /// the tab strip, except you read it only when you ask for it.
    private List<Suggestion> OpenPages(string typed)
    {
        var needle = typed.Trim().ToLowerInvariant();
        return Tabs
            .Where(t => t.Id != ActiveID && !t.IsBlank)
            .Where(t => needle.Length == 0 || t.Label.ToLowerInvariant().Contains(needle)
                || (t.Address != null && Address.Pretty(t.Address).Contains(needle)))
            .OrderByDescending(t => t.Touched)
            .Take(needle.Length == 0 ? 6 : 3)
            .Where(t => t.Address != null)
            .Select(t => new Suggestion(t.Label, Address.Pretty(t.Address!), t.Address!, SuggestionKind.Open, t.Id))
            .ToList();
    }

    /// A row clicked in the list, taken directly rather than through the
    /// keyboard's selection.
    public void Take(Suggestion offer)
    {
        Summoning = false;
        if (Commands.From(offer.Url) is { } chosen)
        {
            Editing = false;
            Typed = "";
            Picked = null;
            Commands.Run(chosen.Command, chosen.Argument);
            return;
        }
        if (offer.Tab is { } id && Tabs.FirstOrDefault(t => t.Id == id) is { } tab) Select(tab);
        else if ((Active ?? Tabs.FirstOrDefault()) is { } here) Go(here, offer.Url);
        Editing = false;
        Typed = "";
        Picked = null;
    }

    /// A backspace means the ending was not wanted. Recomputing it on the very
    /// next keystroke is right; putting it back on this one is what makes a
    /// field impossible to shorten.
    public void StopCompleting() => Ending = null;

    /// Tab, or the right arrow at the end of the line: take what is offered.
    public void AcceptEnding()
    {
        if (string.IsNullOrEmpty(Ending)) return;
        Typed += Ending;
    }

    /// The arrow keys walk the list, and walking off the top lets go of it.
    public void Walk(int step)
    {
        if (Offers.Count == 0) return;
        if (Picked is not { } here) Picked = step > 0 ? 0 : Offers.Count - 1;
        else
        {
            var next = here + step;
            Picked = next < 0 || next >= Offers.Count ? null : next;
        }
    }

    /// Ctrl+L. The current address comes up selected, so typing over it
    /// replaces it and Escape puts it back.
    public void Edit()
    {
        Summoning = false;
        Typed = Active?.Address?.AbsoluteUri ?? "";
        Editing = true;
        FocusRequest++;
    }

    public void Dismiss()
    {
        Summoning = false;
        Cycling = false;
        // A blank tab has nothing behind the field to go back to.
        if (Active?.IsBlank != false) return;
        Editing = false;
        Typed = "";
    }

    /// Enter. A row picked from the list wins; otherwise what the field was
    /// finishing for you wins; otherwise what you actually typed. If none of
    /// those is a place, nothing happens and the field says so.
    public void Submit()
    {
        // A page already open is switched to, not opened again.
        if (Picked is { } p && p < Offers.Count && Offers[p].Tab is { } id && Tabs.FirstOrDefault(t => t.Id == id) is { } open)
        {
            Summoning = false;
            Select(open);
            Editing = false;
            Typed = "";
            return;
        }

        // The switcher proposes nothing but pages you have open. It still has
        // to accept an address typed into it: a Return that quietly does
        // nothing is the worst answer either field could give.
        if (Summoning)
        {
            Summoning = false;
            if (Typed.Trim().Length == 0)
            {
                Editing = false;
                return;
            }
        }

        // A command runs; a command that matches nothing is refused rather
        // than searched for.
        if (Picked is { } c && c < Offers.Count && Commands.From(Offers[c].Url) is { } chosen)
        {
            Editing = false;
            Typed = "";
            Commands.Run(chosen.Command, chosen.Argument);
            return;
        }
        if (FieldInput.Read(Typed, Commands.Bangs) is FieldInput.ToCommand)
        {
            Refusals++;
            return;
        }

        Uri? target;
        if (Picked is { } q && q < Offers.Count) target = Offers[q].Url;
        else if (Ending != null) target = Address.Url(Completed);
        else target = Google.Destination(Typed);

        if (target == null)
        {
            Refusals++;
            return;
        }
        if ((Active ?? Tabs.FirstOrDefault()) is { } tab) Go(tab, target);
        Editing = false;
        Typed = "";
    }

    // MARK: - the page

    /// Ctrl+Shift+R. The article, and nothing that was arranged around it.
    public void ToggleReader()
    {
        if (Active is not { } tab) return;
        Search.Reader.Toggle(tab, worked => { if (!worked) Announce("Nothing to read on this page"); });
    }

    public void Reload() => Active?.Reload();
    public void Back() => Active?.Back();
    public void Forward() => Active?.Forward();

    /// Ctrl+Shift+M. Whatever is making noise in this tab stops making noise.
    public void PauseMedia()
    {
        if (Active is not { } tab) return;
        tab.Run("document.querySelectorAll('video, audio').forEach(function (m) { try { m.pause(); } catch (e) {} });");
        Announce("Paused");
    }

    // MARK: - looking for something on the page

    private bool finding;
    public bool Finding { get => finding; set => Set(ref finding, value); }
    private string needle = "";
    public string Needle { get => needle; set { if (Set(ref needle, value)) Look(true, restart: true); } }
    private bool missed;
    /// Set when the page doesn't hold what was asked for.
    public bool Missed { get => missed; private set => Set(ref missed, value); }
    private int findFocus;
    public int FindFocus { get => findFocus; private set => Set(ref findFocus, value); }
    private string matches = "";
    /// "3 of 12", beside the field, while there is something found.
    public string Matches { get => matches; private set => Set(ref matches, value); }

    public void OpenFind()
    {
        if (Active?.IsBlank != false) return;
        Finding = true;
        FindFocus++;
    }

    public void CloseFind()
    {
        if (!Finding) return;
        Finding = false;
        Needle = "";
        Missed = false;
        Matches = "";
        try { Active?.Core?.Find.Stop(); } catch { }
    }

    /// WebView2 finds for itself, highlighting every match; the bar is ours.
    public async void Look(bool forward, bool restart = false)
    {
        if (Active?.Core is not { } core || Needle.Length == 0)
        {
            Missed = false;
            Matches = "";
            try { Active?.Core?.Find.Stop(); } catch { }
            return;
        }
        try
        {
            var find = core.Find;
            if (restart || find.MatchCount <= 0)
            {
                var options = core.Environment.CreateFindOptions();
                options.FindTerm = Needle;
                options.IsCaseSensitive = false;
                options.SuppressDefaultFindDialog = true;
                options.ShouldHighlightAllMatches = true;
                await find.StartAsync(options);
            }
            else if (forward) find.FindNext();
            else find.FindPrevious();
            Missed = find.MatchCount == 0;
            // WebView2 counts its matches from one.
            Matches = find.MatchCount > 0 ? $"{Math.Max(1, find.ActiveMatchIndex)} of {find.MatchCount}" : "";
        }
        catch { }
    }

    // MARK: - what is kept, and getting rid of it

    /// The last few places, for the History menu.
    public List<Trace> RecentlyVisited => History.Everything().Take(8).ToList();

    public void ClearHistory()
    {
        History.Forget();
        Announce("History cleared");
    }

    /// Cookies, caches, local storage — everything a site left on this
    /// machine, in every space. Clearing it signs you out of everything, which
    /// is the point.
    public async void ClearSites()
    {
        foreach (var core in Tabs.Select(t => t.Core).OfType<Microsoft.Web.WebView2.Core.CoreWebView2>().DistinctBy(c => c.Profile.ProfileName))
            try { await core.Profile.ClearBrowsingDataAsync(); } catch { }
        Announce("Signed out of everything");
    }

    /// Only what was fetched to draw pages, not what identifies you.
    public async void ClearCache()
    {
        foreach (var core in Tabs.Select(t => t.Core).OfType<Microsoft.Web.WebView2.Core.CoreWebView2>().DistinctBy(c => c.Profile.ProfileName))
            try
            {
                await core.Profile.ClearBrowsingDataAsync(
                    Microsoft.Web.WebView2.Core.CoreWebView2BrowsingDataKinds.DiskCache
                    | Microsoft.Web.WebView2.Core.CoreWebView2BrowsingDataKinds.CacheStorage);
            }
            catch { }
        Announce("Cache cleared");
    }

    /// Everything a site has been allowed or refused, for the day you want to
    /// change your mind.
    public void ForgetCaptureChoices()
    {
        foreach (var key in Store.Settings.Keys.Where(k => k.StartsWith("capture.")).ToList())
            Store.Settings.Remove(key);
        Announce("Camera and microphone choices forgotten");
    }

    // MARK: - bookmarks

    public Bookmarks Bookmarks { get; } = new();

    /// Ctrl+Shift+B. The page you are on, at the end of the list.
    public void BookmarkCurrent()
    {
        if (Active is not { Address: { } url } tab) return;
        if (Bookmarks.Contains(url))
        {
            Announce("Already a bookmark");
            return;
        }
        Bookmarks.Add(url, tab.Title);
        Announce("Bookmarked");
    }

    // MARK: - layout

    /// Ctrl+Shift+S. The same tabs, down the left or across the top.
    public void ToggleSidebar() => Prefs.Sidebar = !Prefs.Sidebar;

    /// Ctrl+S. The column out of the way, or back.
    public void ToggleFold()
    {
        if (!Prefs.Sidebar) return;
        Peeking = false;
        Folded = !Folded;
    }

    public void Peek(bool @out) => Peeking = @out;
}
