using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.Windows.AppLifecycle;

namespace Search;

public partial class App : Application
{
    public static MainWindow? Window { get; private set; }
    /// The window's content, for anything that has to measure against it.
    public static Microsoft.UI.Xaml.Controls.Grid Root { get; set; } = null!;
    public static OverlappedPresenter? Presenter { get; set; }
    /// The window was maximised, restored, resized.
    public static Action? WindowChanged;

    public App()
    {
        InitializeComponent();
        UnhandledException += (_, e) =>
        {
            Links.Trouble(e.Exception);
            e.Handled = true;
        };
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        UI.Queue = Microsoft.UI.Dispatching.DispatcherQueue.GetForCurrentThread();
        // The Fluent styles every control draws with: rounded dialogs, text
        // fields, menus, tooltips. Made here, by type, so a native build has
        // them too (see App.xaml) — once the app is up, before any window.
        try { Resources.MergedDictionaries.Add(new Microsoft.UI.Xaml.Controls.XamlControlsResources()); }
        catch (Exception e) { Links.Trouble(e); }
        // The engine starts now, while the window is still being drawn, so the
        // first address typed navigates instead of waiting for it.
        // Light or dark decided before anything is drawn or started, so the
        // first frame and the engine's blank pages are already the right one.
        Palette.SetDark(Store.Settings.String("look") switch
        {
            "dark" => true,
            "light" => false,
            _ => new Windows.UI.ViewManagement.UISettings().GetColorValue(Windows.UI.ViewManagement.UIColorType.Background).R < 128,
        });
        _ = Web.Environment;
        Window = new MainWindow();
        Window.Activate();
        Links.Ready(Window.Browser);
    }

    public static void CloseWindow() => Window?.Close();

    private static bool full;

    /// F11, and a page's own full-screen button.
    public static void ToggleFullScreen() => FullScreen(!full);

    public static void FullScreen(bool on)
    {
        if (Window is not { } window || full == on) return;
        full = on;
        if (on) window.AppWindow.SetPresenter(AppWindowPresenterKind.FullScreen);
        else
        {
            Presenter = Overlapped();
            window.AppWindow.SetPresenter(Presenter);
        }
        WindowChanged?.Invoke();
    }

    /// The window as it normally is: no title bar and none of Windows' own
    /// buttons — the strip is the title bar, and the three buttons are the
    /// app's (see WindowButtons). The border and its corners stay.
    public static OverlappedPresenter Overlapped()
    {
        var presenter = OverlappedPresenter.Create();
        presenter.SetBorderAndTitleBar(true, false);
        presenter.PreferredMinimumWidth = 640;
        presenter.PreferredMinimumHeight = 420;
        return presenter;
    }
}

/// One window, one process. A second launch — a link clicked in Mail, a
/// shortcut double-clicked while Search is open — hands its addresses to the
/// one already running and goes away.
public static class Program
{
    [STAThread]
    public static int Main(string[] args)
    {
        WinRT.ComWrappersSupport.InitializeComWrappers();
        var key = "search" + (Store.World is { } w ? "-" + w : "");
        var main = AppInstance.FindOrRegisterForKey(key);
        if (!main.IsCurrent)
        {
            var activation = AppInstance.GetCurrent().GetActivatedEventArgs();
            main.RedirectActivationToAsync(activation).AsTask().Wait();
            return 0;
        }
        main.Activated += (_, e) => Links.Activated(e);
        Links.Take(args);
        Application.Start(p =>
        {
            var context = new Microsoft.UI.Dispatching.DispatcherQueueSynchronizationContext(
                Microsoft.UI.Dispatching.DispatcherQueue.GetForCurrentThread());
            SynchronizationContext.SetSynchronizationContext(context);
            _ = new App();
        });
        return 0;
    }
}
