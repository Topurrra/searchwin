namespace SearchKit.Field;

/// Tells the field's own text changes from typing.
///
/// The browser puts text into the field (a row walked to, Ctrl+L's address,
/// a command the list has already chosen), and the text box tells of each
/// change a moment later, once per change, reading the text as it is by
/// then. Two puts in one turn (the list redrawn, then its first row
/// picked) come back as two reports of the second text. Forgetting what was
/// put after the first report took the second for typing: `>dark` became a
/// search for "Dark", the command's own title.
///
/// So what was put is remembered until the box reports something else,
/// which can only be a key.
public sealed class FieldEcho
{
    private string? put;

    /// The browser is setting the field's text to `text`.
    public void Put(string text) => put = text;

    /// The box says it now holds `text`: true when that is the browser's
    /// own doing, false when it was typed.
    public bool Ours(string text)
    {
        if (put != null && text == put) return true;
        put = null;
        return false;
    }
}
