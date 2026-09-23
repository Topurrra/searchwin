using Microsoft.Web.WebView2.Core;

namespace Search;

// Where each feature hooks into the browser's life: once at launch, once per
// tab when it is made, once per page when its engine starts. Each feature
// implements the partial methods it needs in its own file, so this one never
// has to change when a feature does.
public sealed partial class Browser
{
    private void StartFeatures()
    {
        FormRelay.PasskeysOffered = Prefs.Passkeys;
        StartShield();
        StartCurtain();
        StartPasswords();
        StartExtensions();
        StartFloat();
        StartReader();
        StartBench();
        Updater.Shared.CheckIfDue(Announce);
    }

    private void PrepareFeatures(Tab tab)
    {
        PrepareCurtain(tab);
        PreparePasswords(tab);
        PrepareExtensions(tab);
    }

    private void AttachFeatures(Tab tab, CoreWebView2 core)
    {
        AttachShield(tab, core);
        AttachCurtain(tab, core);
        AttachPasswords(tab, core);
        AttachExtensions(tab, core);
        AttachFloat(tab, core);
    }

    partial void StartShield();
    partial void StartCurtain();
    partial void StartPasswords();
    partial void StartExtensions();
    partial void StartFloat();
    partial void StartReader();
    partial void StartBench();

    partial void PrepareCurtain(Tab tab);
    partial void PreparePasswords(Tab tab);
    partial void PrepareExtensions(Tab tab);

    partial void AttachShield(Tab tab, CoreWebView2 core);
    partial void AttachCurtain(Tab tab, CoreWebView2 core);
    partial void AttachPasswords(Tab tab, CoreWebView2 core);
    partial void AttachExtensions(Tab tab, CoreWebView2 core);
    partial void AttachFloat(Tab tab, CoreWebView2 core);
}
