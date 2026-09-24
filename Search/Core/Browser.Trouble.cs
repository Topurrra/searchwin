using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.Web.WebView2.Core;
using Windows.Networking.Connectivity;

namespace Search;

// Search's own pages and questions, in place of the engine's. A page that
// didn't load gets Search's page, not Edge's "Hmmm… can't reach this page";
// a bad certificate gets Search's warning, not Edge's; a page's alert, confirm
// and prompt, and a site asking for a name and password, get Search's dialogs.
public sealed partial class Browser
{
    /// Sites you chose to open despite a bad certificate, until you quit.
    private readonly HashSet<string> trustedAnyway = new(StringComparer.OrdinalIgnoreCase);
    private bool watchingNetwork;

    private void AttachOwnPages(Tab tab, CoreWebView2 core)
    {
        core.Settings.AreDefaultScriptDialogsEnabled = false;
        core.ScriptDialogOpening += (_, e) => PageDialog(tab, e);
        core.BasicAuthenticationRequested += (_, e) => SignInDialog(tab, e);
        core.ServerCertificateErrorDetected += (_, e) =>
        {
            // Deny, and the navigation fails with a certificate status: our
            // page shows instead of the engine's. Allowed only once you've
            // said so for this site.
            var url = Uri.TryCreate(e.RequestUri, UriKind.Absolute, out var u) ? u : null;
            if (url != null && trustedAnyway.Contains(url.Host))
            {
                e.Action = CoreWebView2ServerCertificateErrorAction.AlwaysAllow;
                return;
            }
            e.Action = CoreWebView2ServerCertificateErrorAction.Cancel;
            // Cancelling reads as "cancelled" to the navigation, so the
            // warning goes up here, where the reason is known.
            tab.Fail(new PageTrouble(TroubleKind.Insecure, PageTrouble.HostOf(url)));
        };
        WatchNetwork();
    }

    private async void Fail(Tab tab, CoreWebView2 core, CoreWebView2WebErrorStatus status)
    {
        tab.Uncover();
        // Cancelled is not a failure: it's what a redirect, a stopped load, or
        // a second Enter in quick succession looks like from here. Nor is a
        // page that turned into a download.
        if (status is CoreWebView2WebErrorStatus.OperationCanceled or CoreWebView2WebErrorStatus.ConnectionAborted) return;
        if (status == CoreWebView2WebErrorStatus.Unknown)
        {
            // "Unknown" is also what a download or a stop looks like. It's a
            // failure only if the engine put its own error page up.
            string where;
            try { where = await core.ExecuteScriptAsync("location.protocol"); }
            catch { return; }
            if (where != "\"chrome-error:\"") return;
        }
        // The page that failed, as the engine tried it; the tab's address can
        // already be the next one.
        var tried = Uri.TryCreate(core.Source, UriKind.Absolute, out var source) && Address.IsWeb(source) ? source : tab.Address;
        tab.Fail(new PageTrouble(Kind(status), PageTrouble.HostOf(tried)));
    }

    private static TroubleKind Kind(CoreWebView2WebErrorStatus status)
    {
        var online = true;
        try
        {
            online = NetworkInformation.GetInternetConnectionProfile()?.GetNetworkConnectivityLevel()
                == NetworkConnectivityLevel.InternetAccess;
        }
        catch { }
        if (!online || status == CoreWebView2WebErrorStatus.Disconnected) return TroubleKind.Offline;
        return status switch
        {
            CoreWebView2WebErrorStatus.HostNameNotResolved => TroubleKind.NoSuchSite,
            CoreWebView2WebErrorStatus.Timeout => TroubleKind.TooSlow,
            CoreWebView2WebErrorStatus.ServerUnreachable or CoreWebView2WebErrorStatus.CannotConnect
                or CoreWebView2WebErrorStatus.ConnectionReset => TroubleKind.Refused,
            CoreWebView2WebErrorStatus.CertificateCommonNameIsIncorrect or CoreWebView2WebErrorStatus.CertificateExpired
                or CoreWebView2WebErrorStatus.ClientCertificateContainsErrors or CoreWebView2WebErrorStatus.CertificateRevoked
                or CoreWebView2WebErrorStatus.CertificateIsInvalid => TroubleKind.Insecure,
            CoreWebView2WebErrorStatus.ValidAuthenticationCredentialsRequired
                or CoreWebView2WebErrorStatus.ValidProxyAuthenticationRequired => TroubleKind.Blocked,
            _ => TroubleKind.Broken,
        };
    }

    // MARK: - what the trouble page offers

    public void TryAgain()
    {
        if (Active is { Failure: not null } tab) Retry(tab);
    }

    /// Back to the address the tab is for. Not a reload: when the engine
    /// never got there (a refused certificate), what it would reload is
    /// whatever it showed before.
    private void Retry(Tab tab)
    {
        if (tab.Address is { } url && Address.IsWeb(url)) Go(tab, url);
        else tab.Reload();
    }

    /// "Search the web for it", for a name that led nowhere.
    public void SearchForTrouble()
    {
        if (Active is not { Failure: { } trouble } tab) return;
        if (Google.Url(tab.Address?.Host ?? trouble.Host) is { } asked) Go(tab, asked);
    }

    /// Continue to a site with a bad certificate, having been told. For this
    /// site, until Search quits.
    public void TrustAnyway()
    {
        if (Active is not { Failure.Kind: TroubleKind.Insecure } tab || tab.Address is not { } url) return;
        trustedAnyway.Add(url.Host);
        Go(tab, url);
    }

    public void LeaveTrouble()
    {
        if (Active is not { } tab) return;
        if (tab.CanGoBack) tab.Back();
        else Close(tab);
    }

    /// Offline pages come back on their own when the connection does.
    private void WatchNetwork()
    {
        if (watchingNetwork) return;
        watchingNetwork = true;
        NetworkInformation.NetworkStatusChanged += _ => UI.Do(() =>
        {
            bool online;
            try
            {
                online = NetworkInformation.GetInternetConnectionProfile()?.GetNetworkConnectivityLevel()
                    == NetworkConnectivityLevel.InternetAccess;
            }
            catch { return; }
            if (!online) return;
            foreach (var tab in Tabs.Where(t => t.Failure?.Kind == TroubleKind.Offline).ToList())
            {
                if (tab.Id == ActiveID) Retry(tab);
                else tab.Stale = true;
            }
        });
    }

    // MARK: - a page's own questions

    private static readonly SemaphoreSlim dialogTurn = new(1, 1);

    /// alert(), confirm(), prompt() and "leave this page?", in Search's dialog.
    private async void PageDialog(Tab tab, CoreWebView2ScriptDialogOpeningEventArgs e)
    {
        using var deferral = e.GetDeferral();
        var host = Uri.TryCreate(e.Uri, UriKind.Absolute, out var url) ? Address.Pretty(new Uri(url.GetLeftPart(UriPartial.Authority))) : "This page";
        var field = e.Kind == CoreWebView2ScriptDialogKind.Prompt
            ? new TextBox { Text = e.DefaultText ?? "", Width = 320 }
            : null;
        var words = new TextBlock { Text = e.Message ?? "", TextWrapping = TextWrapping.Wrap, MaxWidth = 360 };
        var body = new StackPanel { Spacing = 12, Children = { words } };
        if (field != null) body.Children.Add(field);
        var dialog = new ContentDialog
        {
            Title = e.Kind == CoreWebView2ScriptDialogKind.Beforeunload ? "Leave this page?" : $"{host} says",
            Content = e.Kind == CoreWebView2ScriptDialogKind.Beforeunload
                ? new TextBlock { Text = "Changes you made may not be saved.", TextWrapping = TextWrapping.Wrap, MaxWidth = 360 }
                : body,
            PrimaryButtonText = e.Kind == CoreWebView2ScriptDialogKind.Beforeunload ? "Leave" : "OK",
            DefaultButton = ContentDialogButton.Primary,
        };
        if (e.Kind != CoreWebView2ScriptDialogKind.Alert)
            dialog.CloseButtonText = e.Kind == CoreWebView2ScriptDialogKind.Beforeunload ? "Stay" : "Cancel";
        if (field != null) field.Loaded += (_, _) => { field.Focus(FocusState.Programmatic); field.SelectAll(); };
        if (await Show(dialog))
        {
            if (field != null) e.ResultText = field.Text;
            e.Accept();
        }
    }

    /// A site asking for a name and password (HTTP authentication).
    private async void SignInDialog(Tab tab, CoreWebView2BasicAuthenticationRequestedEventArgs e)
    {
        using var deferral = e.GetDeferral();
        var host = Uri.TryCreate(e.Uri, UriKind.Absolute, out var url) ? url.Host : "This site";
        var name = new TextBox { PlaceholderText = "Name", Width = 300 };
        var password = new PasswordBox { PlaceholderText = "Password", Width = 300 };
        var detail = new TextBlock
        {
            Text = string.IsNullOrWhiteSpace(e.Challenge) ? $"{host} asks you to sign in." : $"{host} asks you to sign in: {e.Challenge}",
            TextWrapping = TextWrapping.Wrap,
            MaxWidth = 300,
            Opacity = 0.75,
        };
        name.Loaded += (_, _) => name.Focus(FocusState.Programmatic);
        var dialog = new ContentDialog
        {
            Title = "Sign in",
            Content = new StackPanel { Spacing = 10, Children = { detail, name, password } },
            PrimaryButtonText = "Sign In",
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Primary,
        };
        if (await Show(dialog))
        {
            e.Response.UserName = name.Text;
            e.Response.Password = password.Password;
        }
        else
        {
            e.Cancel = true;
        }
    }

    /// One dialog at a time: a second waits for the first.
    private static async Task<bool> Show(ContentDialog dialog)
    {
        if (App.Root?.XamlRoot is not { } root) return false;
        await dialogTurn.WaitAsync();
        try
        {
            dialog.XamlRoot = root;
            dialog.RequestedTheme = App.Root.RequestedTheme;
            return await dialog.ShowAsync() == ContentDialogResult.Primary;
        }
        catch
        {
            return false;
        }
        finally
        {
            dialogTurn.Release();
        }
    }
}
