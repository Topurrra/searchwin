namespace Search;

// Passwords: offered once a sign-in has actually worked, offered back under
// the field when you click it, kept in Windows' own Credential Manager (see
// Vault) — and brought in from another browser's own CSV export. Bookmarks
// and history come straight from that browser's files (see Import).
public sealed partial class Browser
{
    public sealed record Login(string Host, string User, string Password, DateTime? Used)
    {
        public string Id => Host + "\u0001" + User;
    }

    public sealed record Offer(Login Login, bool Changed);

    private Offer? offering;
    /// A name and password a page has just sent, waiting to be offered a place
    /// in the vault. Held only until you answer.
    public Offer? Offering { get => offering; private set => Set(ref offering, value); }

    /// The accounts kept for the site whose sign-in box has the caret, and
    /// where that box is — a list hangs from it, and a click fills the form.
    /// Nothing is put into a page until you have pointed at it.
    public sealed record Accounts(Guid Tab, Windows.Foundation.Rect Spot, List<Login> Logins);

    private Accounts? suggesting;
    public Accounts? Suggesting { get => suggesting; set => Set(ref suggesting, value); }

    /// The site the list was read for, so a box that only moved — the page
    /// scrolled — moves the list without asking Credential Manager again.
    private string? suggestingHost;
    /// Set once you have picked, so the list doesn't come straight back for
    /// the box you are still in. Cleared when the caret leaves the boxes.
    private Guid? pickedInto;
    /// The list is taken down a beat after the caret leaves, not the same
    /// instant: clicking a row can take the caret out of the page first, and
    /// a list that vanished on the way down would never be clicked.
    private Later? lowering;

    partial void StartPasswords()
    {
        Bridge.Handlers[FormRelay.Name] = (tab, body) => FormRelay.Take(this, tab, body);
    }

    public void KeepOffer()
    {
        if (Offering is not { } offer) return;
        Offering = null;
        var login = offer.Login;
        if (!Vault.Save(login.Host, login.User, login.Password, DateTime.UtcNow))
        {
            Announce("Credential Manager refused it");
            return;
        }
        Relist();
        Announce(offer.Changed ? $"Password updated for {login.Host}" : $"Password saved for {login.Host}");
    }

    public void DropOffer() => Offering = null;

    /// Never for this site. Some sites you sign into on purpose with nothing
    /// you want remembered.
    public void NeverOffer()
    {
        if (Offering is not { } offer) return;
        Vault.NeverFor(offer.Login.Host);
        Offering = null;
        Announce($"Never for {offer.Login.Host}");
    }

    /// One of the accounts in the list, picked by name.
    public async void Choose(Login login)
    {
        lowering?.Cancel();
        var tab = Tabs.FirstOrDefault(t => t.Id == Suggesting?.Tab) ?? Active;
        if (tab == null) return;
        Suggesting = null;
        pickedInto = tab.Id;
        Vault.Touch(login);
        if (!await tab.Fill(login.User, login.Password)) Announce("Couldn't find the sign-in fields anymore");
    }

    public void DropChoice() => Suggesting = null;

    /// A row of the list is being pressed: whatever the page says about its
    /// caret in the meantime, the list stays until the press is over.
    public void HoldChoice() => lowering?.Cancel();

    /// The caret in a sign-in box: the accounts kept for this site hang from
    /// the box, and go when the caret does. Nothing is filled on its own —
    /// the way Safari does it, and what a person expects.
    internal void Field(Tab tab, Windows.Foundation.Rect? spot)
    {
        if (spot is not { } here)
        {
            if (pickedInto == tab.Id) pickedInto = null;
            if (Suggesting?.Tab != tab.Id) return;
            lowering?.Cancel();
            lowering = UI.After(0.2, () => { if (Suggesting?.Tab == tab.Id) Suggesting = null; });
            return;
        }
        lowering?.Cancel();
        if (!Prefs.FillsPasswords || tab.Id != ActiveID || pickedInto == tab.Id || Curtain.Host(tab.Address) is not { } host) return;
        if (Suggesting is { } shown && shown.Tab == tab.Id && suggestingHost == host)
        {
            if (shown.Spot != here) Suggesting = shown with { Spot = here };
            return;
        }
        var known = Vault.LoginsMatching(host).Take(5).ToList();
        suggestingHost = host;
        Suggesting = known.Count == 0 ? null : new Accounts(tab.Id, here, known);
    }

    /// A sign-in that took. The site it was sent from — not the one it landed
    /// on — then the name and the password.
    internal void Credentials(Tab tab, string host, string user, string password)
    {
        if (!Prefs.SavesPasswords || password.Length == 0 || tab.Shy || Vault.IsNever(host)) return;
        // A password manager extension that asked Chrome's way to do the
        // saving itself.
        if (Extensions.Shared.PasswordSavingTakenBy != null) return;
        var known = Vault.LoginsFor(host);
        // Nothing to ask about one that is already known.
        if (known.FirstOrDefault(l => l.User == user && l.Password == password) is { } same)
        {
            Vault.Touch(same);
            return;
        }
        var offer = new Offer(new Login(host, user, password, null), known.Any(l => l.User == user));
        if (Offering == offer) return;
        Offering = offer;
    }

    /// A page that arrived after a password went out: did the sign-in take?
    private void SettleSignIn(Tab tab) => tab.SettleSignIn(this);

    // MARK: - the list of what is kept

    private List<Login> saved = [];
    public List<Login> Saved { get => saved; private set { saved = value; Tell(); Tell(nameof(ShownSites)); } }

    private string hunting = "";
    public string Hunting { get => hunting; set { if (Set(ref hunting, value)) Tell(nameof(ShownSites)); } }

    public sealed record SiteRow(string Host, List<Login> Logins);

    /// Grouped by site, filtered by what has been typed.
    public List<SiteRow> ShownSites
    {
        get
        {
            var needle = Hunting.Trim().ToLowerInvariant();
            var rows = needle.Length == 0 ? Saved : Saved.Where(l => l.Host.Contains(needle) || l.User.ToLowerInvariant().Contains(needle));
            return rows.GroupBy(l => l.Host)
                .OrderBy(g => g.Key, StringComparer.Ordinal)
                .Select(g => new SiteRow(g.Key, g.OrderBy(l => l.User, StringComparer.Ordinal).ToList()))
                .ToList();
        }
    }

    public void Relist() => Saved = Vault.All();

    public void Keep(string host, string user, string password)
    {
        if (!Vault.Save(host, user, password))
        {
            Announce("Credential Manager refused it");
            return;
        }
        Relist();
        Announce($"Kept for {host}");
    }

    public void Forget(Login login)
    {
        Vault.Forget(login.Host, login.User);
        Relist();
    }

    /// Marked so no clipboard history keeps it — Search's own included.
    public void Copy(Login login)
    {
        Announce(QuietCopy.Copy(login.Password) ? "Password copied" : "Couldn't copy — try again");
    }

    // MARK: - bringing things in

    private string? importNote;
    /// What the foot of the passwords panel says after an import: how a
    /// browser's own passwords come over, since its files keep them locked.
    public string? ImportNote { get => importNote; set => Set(ref importNote, value); }

    /// The other browser's history, into this one's. Off the UI thread for
    /// the reading; the merge itself is a moment.
    public void TakePlaces(Chromium.Source source, Action<int> done)
    {
        Task.Run(() => Chromium.Places(source)).ContinueWith(read =>
        {
            var places = read.IsCompletedSuccessfully ? read.Result : [];
            UI.Do(() =>
            {
                foreach (var place in places) History.Take(place.Url, place.Title, place.Count, place.Last);
                History.Settle();
                done(places.Count);
            });
        });
    }

    /// Another browser's bookmarks, folders and all — and, behind them, the
    /// icons it had for those sites, so the menu wears them from the start
    /// instead of a letter each. Returns how many pages came over.
    public int TakeBookmarks(Chromium.Source source)
    {
        var found = Chromium.Bookmarks(source);
        Bookmarks.Take(found, source.Name);
        var count = Search.Bookmarks.CountOf(found);
        Announce(count == 0 ? $"No bookmarks in {source.Name}" : $"{count} bookmarks from {source.Name}");
        var urls = Search.Bookmarks.Urls(found).ToList();
        Task.Run(() => Chromium.Icons(source, urls)).ContinueWith(read =>
        {
            if (!read.IsCompletedSuccessfully) return;
            UI.Do(async () =>
            {
                foreach (var (host, png) in read.Result) await Favicons.Shared.Adopt(png, host);
                Tell(nameof(Bookmarks));
            });
        });
        return count;
    }

    /// Takes in a CSV as Google Password Manager, Chrome or Edge export one.
    /// The file is read once and never copied.
    public async void ImportPasswords()
    {
        string text;
        try
        {
            if (await Pick.File([".csv", ".txt"], "Import", Microsoft.Windows.Storage.Pickers.PickerLocationId.Downloads) is not { } file) return;
            text = await File.ReadAllTextAsync(file);
        }
        catch
        {
            Announce("Couldn't read that file as text");
            return;
        }
        var result = Vault.Take(text);
        Relist();
        Announce(result.Skipped == 0
            ? $"{result.Kept} passwords in Credential Manager"
            : $"{result.Kept} in Credential Manager, {result.Skipped} skipped");
    }
}
