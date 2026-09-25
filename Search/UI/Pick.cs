using Microsoft.Windows.Storage.Pickers;

namespace Search;

/// Windows' own folder, open and save dialogs, through the App SDK's pickers.
/// The older Windows.Storage pickers fail in an app without a package: the
/// dialog shows, then the choice comes back as E_FAIL. Each gives a path, or
/// null (an empty list) when the dialog is cancelled.
public static class Pick
{
    public static async Task<string?> Folder(string? commit = null, PickerLocationId start = PickerLocationId.Unspecified)
    {
        if (App.Window is not { } window) return null;
        var picker = new FolderPicker(window.AppWindow.Id) { SuggestedStartLocation = start };
        if (commit != null) picker.CommitButtonText = commit;
        return (await picker.PickSingleFolderAsync())?.Path;
    }

    public static async Task<string?> File(IEnumerable<string> types, string? commit = null, PickerLocationId start = PickerLocationId.Unspecified)
    {
        if (Opener(types, commit, start) is not { } picker) return null;
        return (await picker.PickSingleFileAsync())?.Path;
    }

    public static async Task<IReadOnlyList<string>> Files(IEnumerable<string> types)
    {
        if (Opener(types, null, PickerLocationId.Unspecified) is not { } picker) return [];
        return [.. (await picker.PickMultipleFilesAsync()).Select(f => f.Path)];
    }

    /// Where to save a file of one of `types` (".txt"), suggesting `name`.
    public static async Task<string?> Save(IList<string> types, string? name)
    {
        if (App.Window is not { } window) return null;
        var picker = new FileSavePicker(window.AppWindow.Id);
        picker.FileTypeChoices.Add("File", types);
        if (!string.IsNullOrEmpty(name)) picker.SuggestedFileName = name;
        return (await picker.PickSaveFileAsync())?.Path;
    }

    private static FileOpenPicker? Opener(IEnumerable<string> types, string? commit, PickerLocationId start)
    {
        if (App.Window is not { } window) return null;
        var picker = new FileOpenPicker(window.AppWindow.Id) { SuggestedStartLocation = start };
        if (commit != null) picker.CommitButtonText = commit;
        foreach (var type in types) picker.FileTypeFilter.Add(type);
        if (picker.FileTypeFilter.Count == 0) picker.FileTypeFilter.Add("*");
        return picker;
    }
}
