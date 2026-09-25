using System.Text.Json.Nodes;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using SearchKit.Field;

namespace Search;

// Ctrl+Shift+V: what you copied, in a list over the page (ClipPopup), and
// Enter puts the one you pick where the caret was — in a page's text box, in
// the address field, in any of the browser's own fields. Always as plain
// text: the history keeps text, not the formatting it came with, so this is
// also Windows' "paste without formatting". A picture goes in as a picture,
// through a real paste. With the caret nowhere, the entry goes back on the
// clipboard for a Ctrl+V wherever it's wanted.
public sealed partial class Browser
{
    private bool clipping;
    /// The list is up.
    public bool Clipping { get => clipping; private set => Set(ref clipping, value); }

    /// Where Enter puts it, decided as the list opens: the box with the
    /// caret, or the page.
    private TextBox? clipBox;
    private Tab? clipPage;

    /// Where a paste goes now: "field", "page", or "clipboard" (nowhere to
    /// type, so it's copied). For the list's foot and the bench.
    public string ClipAim => AimOf(clipBox, clipPage);

    /// Ctrl+Shift+V. Again closes it.
    public void ShowClipboard()
    {
        if (Clipping)
        {
            HideClipboard();
            return;
        }
        clipBox = null;
        clipPage = null;
        var focused = App.Root?.XamlRoot is { } root ? FocusManager.GetFocusedElement(root) : null;
        if (focused is TextBox box) clipBox = box;
        // A page's keys never reach this process, so with nothing of ours
        // focused the caret is in the page, if anywhere.
        else if (focused is null or WebView2 && Active is { IsBlank: false, Built: not null } tab) clipPage = tab;
        Clipping = true;
    }

    /// The list put away; `handBack` returns the caret to where it was.
    public void HideClipboard(bool handBack = true)
    {
        if (!Clipping) return;
        Clipping = false;
        if (!handBack) return;
        if (clipBox is { } box) UI.Soon(() => box.Focus(FocusState.Programmatic));
        else if (clipPage is { } tab && tab == Active) UI.Soon(() => tab.Built?.Focus(FocusState.Programmatic));
    }

    /// Enter on an entry. `go` (Shift+Enter) takes its text as the field
    /// would — a place, or a search — instead of pasting it.
    public async void PasteClip(ClipEntry entry, bool go = false)
    {
        var box = clipBox;
        var page = clipPage;
        HideClipboard(handBack: !go);
        if (go)
        {
            GoWith(entry.Image ? null : entry.Text);
            return;
        }
        try
        {
            if (entry.Image)
            {
                // Only a real paste carries a picture: it goes on the
                // clipboard, and the page is handed Ctrl+V.
                if (!await ClipHistory.CopyBack(entry.Id)) { Announce("Couldn't copy that"); return; }
                if (page != null && page == Active) UI.After(0.05, () => Keystroke(page, paste: true));
                else Announce("Picture copied — Ctrl+V to paste it");
                LastPaste = new("picture", AimOf(box, page));
                return;
            }
            // A secret isn't put back on the clipboard as a side effect: it
            // goes where it was asked for and nowhere else.
            var typing = box != null || page?.Typing == true;
            if (!entry.Sensitive || !typing) _ = ClipHistory.CopyBack(entry.Id);
            if (box != null)
            {
                UI.Soon(() => Insert(box, entry.Text));
            }
            else if (page != null && page == Active)
            {
                UI.After(0.05, () => Keystroke(page, text: entry.Text));
                if (!page.Typing) Announce("Copied");
            }
            else
            {
                Announce("Copied — Ctrl+V puts it where you want it");
            }
            LastPaste = new(entry.Sensitive ? "secret" : "text", AimOf(box, page));
        }
        catch (Exception error)
        {
            Links.Trouble(error);
        }
    }

    /// What the last paste was and where it went, for the bench.
    public (string Kind, string Aim)? LastPaste { get; private set; }

    private static string AimOf(TextBox? box, Tab? page) => box != null ? "field" : page != null ? "page" : "clipboard";

    /// Into one of the browser's own fields, over its selection, the caret
    /// after it. A one-line field takes the text on one line.
    private static void Insert(TextBox box, string text)
    {
        if (!box.AcceptsReturn) text = string.Join(' ', text.Split(['\r', '\n'], StringSplitOptions.RemoveEmptyEntries));
        var start = Math.Min(box.SelectionStart, box.Text.Length);
        var end = Math.Min(start + box.SelectionLength, box.Text.Length);
        box.Text = box.Text[..start] + text + box.Text[end..];
        box.Select(start + text.Length, 0);
    }

    /// Into the page, as input the page can't tell from a person's: the text
    /// as typed (Chromium's own insertText, into whichever frame has the
    /// caret, with undo and every input event), or Ctrl+V for a picture.
    private async void Keystroke(Tab page, string? text = null, bool paste = false)
    {
        if (page.Core is not { } core) return;
        try
        {
            if (text != null)
            {
                await core.CallDevToolsProtocolMethodAsync("Input.insertText", new JsonObject { ["text"] = text }.ToJsonString());
                return;
            }
            if (!paste) return;
            foreach (var type in new[] { "rawKeyDown", "keyUp" })
            {
                var key = new JsonObject
                {
                    ["type"] = type,
                    ["modifiers"] = 2,
                    ["windowsVirtualKeyCode"] = 86,
                    ["key"] = "v",
                    ["code"] = "KeyV",
                };
                if (type == "rawKeyDown") key["commands"] = new JsonArray("paste");
                await core.CallDevToolsProtocolMethodAsync("Input.dispatchKeyEvent", key.ToJsonString());
            }
        }
        catch (Exception error)
        {
            Log.Write($"clipboard: paste into page: {error.Message}");
            Announce("Couldn't paste there — it's on the clipboard");
        }
    }
}
