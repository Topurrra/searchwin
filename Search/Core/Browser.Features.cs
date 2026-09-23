using Microsoft.Web.WebView2.Core;

namespace Search;

// Where each feature hooks into the browser's life: once at launch, once per
// tab when it is made, once per page when its engine starts. One line each,
// so the features stay in their own files.
public sealed partial class Browser
{
    private void StartFeatures()
    {
        FormRelay.PasskeysOffered = Prefs.Passkeys;
        Shield.Shared.Enabled = Prefs.Shielded;
        Shield.Shared.Start();
        Curtain.Start(this);
        FormRelay.Start(this);
        StoreRelay.Start(this);
        Extensions.Shared.Start(this);
        Float.Start(this);
        if (Prefs.Bench) Search.Bench.Shared.Start(this);
        Updater.Shared.CheckIfDue(Announce);
    }

    private void PrepareFeatures(Tab tab) { }

    private void AttachFeatures(Tab tab, CoreWebView2 core) { }
}
