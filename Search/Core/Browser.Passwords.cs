namespace Search;

// Passwords: offered once a sign-in has actually worked, offered back under
// the field when you click it, kept in Windows' own Credential Manager.
//
// PORT: see Sources/Search/Vault.swift, Forms.swift, Passwords.swift,
// Accounts.swift and the passwords part of Browser.swift.
public sealed partial class Browser
{
    public sealed record Login(string Host, string User, string Password, DateTime? Used);

    public sealed record Offer(Login Login, bool Changed);

    private Offer? offering;
    /// A name and password a page has just sent, waiting to be offered a place
    /// in the vault. Held only until you answer.
    public Offer? Offering { get => offering; private set => Set(ref offering, value); }

    public sealed record Accounts(Guid Tab, Windows.Foundation.Rect Spot, List<Login> Logins);

    private Accounts? suggesting;
    /// The accounts kept for the site whose sign-in box has the caret, and
    /// where that box is.
    public Accounts? Suggesting { get => suggesting; set => Set(ref suggesting, value); }

    public void KeepOffer() { Offering = null; }
    public void DropOffer() { Offering = null; }
    public void NeverOffer() { Offering = null; }
    public void DropChoice() { Suggesting = null; }
    public void Choose(Login login) { Suggesting = null; }
    public void Relist() { }

    private void SettleSignIn(Tab tab) { }
}

/// The sign-in relay: where the sign-in boxes are, what was sent from them.
public static class FormRelay
{
    public static bool PasskeysOffered { get; set; } = true;
    public const string Script = "";
    public const string WithoutPasskeys = "";
    public static void Start(Browser browser) { }
}
