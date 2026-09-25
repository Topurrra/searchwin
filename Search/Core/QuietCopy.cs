using System.Runtime.InteropServices;

namespace Search;

/// A password onto the clipboard, marked the way password managers mark
/// theirs, so it isn't kept anywhere: clipboard monitors — Search's own
/// history among them — skip it (ExcludeClipboardContentFromMonitorProcessing),
/// and so do Windows' own history (Win+V) and cloud clipboard. Clipboard
/// history being on by default makes this the only right way to copy one.
public static class QuietCopy
{
    private const uint CF_UNICODETEXT = 13, GMEM_MOVEABLE = 0x0002;

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool OpenClipboard(IntPtr owner);
    [DllImport("user32.dll")]
    private static extern bool CloseClipboard();
    [DllImport("user32.dll")]
    private static extern bool EmptyClipboard();
    [DllImport("user32.dll", SetLastError = true)]
    private static extern IntPtr SetClipboardData(uint format, IntPtr data);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern uint RegisterClipboardFormatW(string name);
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr GlobalAlloc(uint flags, nuint bytes);
    [DllImport("kernel32.dll")]
    private static extern IntPtr GlobalLock(IntPtr memory);
    [DllImport("kernel32.dll")]
    private static extern bool GlobalUnlock(IntPtr memory);
    [DllImport("kernel32.dll")]
    private static extern IntPtr GlobalFree(IntPtr memory);

    /// False if the clipboard couldn't be had (another app holding it).
    public static bool Copy(string text)
    {
        var owner = App.Window is { } window ? WinRT.Interop.WindowNative.GetWindowHandle(window) : IntPtr.Zero;
        var open = false;
        // Another app can hold the clipboard for a moment while it copies.
        for (var attempt = 0; attempt < 6 && !(open = OpenClipboard(owner)); attempt++) Thread.Sleep(15 << attempt);
        if (!open) return false;
        try
        {
            EmptyClipboard();
            var chars = (text + "\0").ToCharArray();
            if (!Put(CF_UNICODETEXT, MemoryMarshal.AsBytes(chars.AsSpan()))) return false;
            // The markers: a DWORD 0 for Windows' history and cloud, and
            // presence alone for the monitors.
            var zero = new byte[4];
            Put(RegisterClipboardFormatW("ExcludeClipboardContentFromMonitorProcessing"), zero);
            Put(RegisterClipboardFormatW("CanIncludeInClipboardHistory"), zero);
            Put(RegisterClipboardFormatW("CanUploadToCloudClipboard"), zero);
            return true;
        }
        finally
        {
            CloseClipboard();
        }
    }

    private static bool Put(uint format, ReadOnlySpan<byte> bytes)
    {
        if (format == 0) return false;
        var memory = GlobalAlloc(GMEM_MOVEABLE, (nuint)bytes.Length);
        if (memory == IntPtr.Zero) return false;
        var at = GlobalLock(memory);
        if (at == IntPtr.Zero)
        {
            GlobalFree(memory);
            return false;
        }
        Marshal.Copy(bytes.ToArray(), 0, at, bytes.Length);
        GlobalUnlock(memory);
        // The clipboard owns it once it's taken; only a refusal is ours to free.
        if (SetClipboardData(format, memory) != IntPtr.Zero) return true;
        GlobalFree(memory);
        return false;
    }
}
