namespace Search;

/// Updates. The Mac app fetches signed builds from Office Commun once a day;
/// there is no such server for Windows builds yet, so this only knows its own
/// version and checks nothing.
public sealed class Updater
{
    public static readonly Updater Shared = new();
    public static string Version => typeof(Updater).Assembly.GetName().Version?.ToString(3) ?? "1.0.0";
    public void CheckIfDue(Action<string> say) { }
}
