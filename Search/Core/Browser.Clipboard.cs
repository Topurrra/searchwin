using System.Text.Json;
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
//
// A paste goes only into the page the list was opened over: if that tab has
// since gone to another site or another document, the entry is copied
// instead, and the line says so. A secret goes only into the page itself,
// never into a frame inside it, and is never searched for.
public sealed partial class Browser
{
    private bool clipping;
    /// The list is up.
    public bool Clipping { get => clipping; private set => Set(ref clipping, value); }

    /// Where Enter puts it, decided as the list opens: the box with the
    /// caret, or the page — and which page it was then.
    private TextBox? clipBox;
    private Tab? clipPage;
    private ClipGuard.Page clipMark;

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
        clipMark = default;
        var focused = App.Root?.XamlRoot is { } root ? FocusManager.GetFocusedElement(root) : null;
        if (focused is TextBox box) clipBox = box;
        // A page's keys never reach this process, so with nothing of ours
        // focused the caret is in the page, if anywhere.
        else if (focused is null or WebView2 && Active is { IsBlank: false, Built: not null } tab)
        {
            clipPage = tab;
            clipMark = MarkOf(tab);
        }
        Clipping = true;
    }

    /// The page as a paste aimed at it knows it: its site, and its document.
    private static ClipGuard.Page MarkOf(Tab tab) => new(ClipGuard.Origin(tab.Address), tab.Documents);

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
    /// would — a place, or a search — instead of pasting it; never a secret,
    /// unless the secret is itself an address.
    public async void PasteClip(ClipEntry entry, bool go = false)
    {
        var box = clipBox;
        var page = clipPage;
        var mark = clipMark;
        if (go && entry.Sensitive && !ClipGuard.MayGo(entry, text => Address.Url(text) != null))
        {
            HideClipboard();
            Announce("A secret isn't searched for — Enter pastes it");
            LastPaste = new("secret", "refused");
            return;
        }
        HideClipboard(handBack: !go);
        if (go)
        {
            GoWith(entry.Image ? null : entry.Text);
            return;
        }
        var kind = entry.Image ? "picture" : entry.Sensitive ? "secret" : "text";
        try
        {
            if (entry.Image)
            {
                // Only a real paste carries a picture: it goes on the
                // clipboard, and the page is handed Ctrl+V — if it's still
                // the page the list was opened over.
                if (!await ClipHistory.CopyBack(entry)) { Announce("Couldn't copy that"); return; }
                if (page != null && page == Active && ClipGuard.IntoPage(mark, MarkOf(page), false, true) == ClipGuard.Paste.Insert)
                {
                    UI.After(0.05, () => Keystroke(page, paste: true));
                    LastPaste = new(kind, "page");
                }
                else
                {
                    Announce(page != null ? "The page changed — the picture is copied, Ctrl+V to paste it" : "Picture copied — Ctrl+V to paste it");
                    LastPaste = new(kind, page != null ? "moved" : AimOf(box, null));
                }
                return;
            }
            if (box != null)
            {
                // A secret isn't put back on the clipboard as a side effect:
                // it goes where it was asked for and nowhere else.
                if (!entry.Sensitive) _ = ClipHistory.CopyBack(entry);
                UI.Soon(() => Insert(box, entry.Text));
                LastPaste = new(kind, "field");
            }
            else if (page != null && page == Active)
            {
                UI.After(0.05, () => IntoPage(page, mark, entry));
            }
            else
            {
                _ = ClipHistory.CopyBack(entry);
                Announce("Copied — Ctrl+V puts it where you want it");
                LastPaste = new(kind, "clipboard");
            }
        }
        catch (Exception error)
        {
            Links.Trouble(error);
        }
    }

    /// Into the page the list was opened over, if it still is that page;
    /// otherwise onto the clipboard, and the line says why.
    private async void IntoPage(Tab page, ClipGuard.Page opened, ClipEntry entry)
    {
        var kind = entry.Sensitive ? "secret" : "text";
        var caret = entry.Sensitive ? await CaretOf(page) : "";
        // Decided after the page was asked: it may have moved on meanwhile.
        var verdict = page == Active
            ? ClipGuard.IntoPage(opened, MarkOf(page), entry.Sensitive, caret != "frame")
            : ClipGuard.Paste.Moved;
        switch (verdict)
        {
            case ClipGuard.Paste.Moved:
                _ = ClipHistory.CopyBack(entry);
                Announce("The page changed — copied instead, Ctrl+V to paste it");
                LastPaste = new(kind, "moved");
                return;
            case ClipGuard.Paste.Framed:
                _ = ClipHistory.CopyBack(entry);
                Announce("Copied, not pasted — a secret goes only into the page itself, not a frame in it");
                LastPaste = new(kind, "framed");
                return;
        }
        // A secret isn't put back on the clipboard as a side effect: with a
        // box to type into it goes there and nowhere else; without one it's
        // copied (quietly).
        var typing = entry.Sensitive ? caret == "field" : page.Typing;
        if (!entry.Sensitive || !typing) _ = ClipHistory.CopyBack(entry);
        if (!entry.Sensitive || typing) Keystroke(page, text: entry.Text, copied: !entry.Sensitive);
        if (!typing) Announce("Copied");
        LastPaste = new(kind, typing ? "page" : "clipboard");
    }

    /// Where the page's caret is, asked of the page itself (its main frame):
    /// "field" (something that takes typing, in the page itself), "frame"
    /// (inside a frame in it — maybe another site's), or "none".
    private static async Task<string> CaretOf(Tab page)
    {
        if (page.Core is not { } core) return "none";
        try
        {
            using var said = JsonDocument.Parse(await core.ExecuteScriptAsync(CaretScript));
            return said.RootElement is { ValueKind: JsonValueKind.String } s ? s.GetString() ?? "none" : "none";
        }
        catch (Exception error)
        {
            Log.Write($"clipboard: caret: {error.Message}");
            return "none";
        }
    }

    private const string CaretScript = """
        (() => {
          let a = document.activeElement;
          while (a && a.shadowRoot && a.shadowRoot.activeElement) a = a.shadowRoot.activeElement;
          if (!a) return 'none';
          const tag = a.tagName;
          if (/^(IFRAME|FRAME|OBJECT|EMBED|FENCEDFRAME|PORTAL)$/.test(tag)) return 'frame';
          if (a.isContentEditable || document.designMode === 'on') return 'field';
          if (tag === 'TEXTAREA') return a.readOnly || a.disabled ? 'none' : 'field';
          if (tag === 'INPUT') {
            if (a.readOnly || a.disabled) return 'none';
            return /^(button|checkbox|color|file|hidden|image|radio|range|reset|submit)$/i.test(a.type) ? 'none' : 'field';
          }
          return 'none';
        })()
        """;

    /// What the last paste was and where it went, for the bench: "field",
    /// "page", "clipboard"; "moved" or "framed" when it was copied instead;
    /// "refused" for a secret's Shift+Enter.
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
    private async void Keystroke(Tab page, string? text = null, bool paste = false, bool copied = true)
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
            Announce(copied ? "Couldn't paste there — it's on the clipboard" : "Couldn't paste there");
        }
    }
}
