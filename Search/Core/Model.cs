using System.ComponentModel;
using System.Runtime.CompilerServices;

namespace Search;

/// Something the window is drawn from. Setting a property that actually
/// changes says so, by name, and whoever drew from it redraws — the part of
/// SwiftUI's @Published this app leans on, and nothing more.
public abstract partial class Model : INotifyPropertyChanged
{
    public event PropertyChangedEventHandler? PropertyChanged;

    protected bool Set<T>(ref T field, T value, [CallerMemberName] string name = "")
    {
        if (Same(field, value)) return false;
        field = value;
        Tell(name);
        return true;
    }

    // Uri's own equality ignores the fragment (RFC 3986: never sent to a
    // server, so "the same resource" as far as Uri.Equals is concerned).
    // Search's own pages live entirely in the fragment
    // (tools.search/index.html#/play?path=<file>), so a plain
    // EqualityComparer<Uri>.Default would see the player move from one file
    // to the next as no change at all. See SearchKit.Web.UriEquality.
    private static bool Same<T>(T field, T value) =>
        field is Uri a && value is Uri b ? SearchKit.Web.UriEquality.SameAddress(a, b) : EqualityComparer<T>.Default.Equals(field, value);

    protected void Tell([CallerMemberName] string name = "") =>
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(name));

    /// Listens for one property by name, for as long as `owner` wants it.
    public void On(string name, Action act) =>
        PropertyChanged += (_, e) => { if (e.PropertyName == name || e.PropertyName == "") act(); };

    public void OnAny(Action<string> act) =>
        PropertyChanged += (_, e) => act(e.PropertyName ?? "");
}
