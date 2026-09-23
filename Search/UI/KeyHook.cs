using System.Runtime.InteropServices;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Windows.System;

namespace Search;

/// A page takes the keyboard while it has focus — and in WebView2 the page's
/// keys never pass through this process's windows at all: they go straight to
/// the engine's. So the shortcuts are caught one step earlier, from Windows
/// itself, while this window is the one in front. It is what the Mac's local
/// event monitor does, and it is the only way Ctrl+T can mean a new tab
/// whether the caret is in the address field, in a page's text box, or
/// nowhere in particular.
///
/// Anything that isn't a shortcut goes on its way untouched.
public static class KeyHook
{
    private const int WH_KEYBOARD_LL = 13;
    private const int WM_KEYDOWN = 0x0100, WM_KEYUP = 0x0101, WM_SYSKEYDOWN = 0x0104, WM_SYSKEYUP = 0x0105;
    private const int VK_RMENU = 0xA5;

    [StructLayout(LayoutKind.Sequential)]
    private struct KeyInfo
    {
        public uint Vk, Scan, Flags, Time;
        public IntPtr Extra;
    }

    private delegate IntPtr Hook(int code, IntPtr message, IntPtr info);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern IntPtr SetWindowsHookEx(int kind, Hook hook, IntPtr module, uint thread);
    [DllImport("user32.dll")]
    private static extern bool UnhookWindowsHookEx(IntPtr hook);
    [DllImport("user32.dll")]
    private static extern IntPtr CallNextHookEx(IntPtr hook, int code, IntPtr message, IntPtr info);
    [DllImport("user32.dll")]
    private static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    [DllImport("kernel32.dll")]
    private static extern IntPtr GetModuleHandle(string? name);

    private static Hook? callback;
    private static IntPtr handle;
    private static IntPtr window;
    private static readonly HashSet<uint> down = [];

    /// Keys taken on their way down are taken on their way up too, so the page
    /// never sees half a key press.
    private static readonly HashSet<uint> swallowed = [];

    public static void Start(Window owner)
    {
        window = WinRT.Interop.WindowNative.GetWindowHandle(owner);
        callback = OnKey;
        handle = SetWindowsHookEx(WH_KEYBOARD_LL, callback, GetModuleHandle(null), 0);
    }

    public static void Stop()
    {
        if (handle != IntPtr.Zero) UnhookWindowsHookEx(handle);
        handle = IntPtr.Zero;
    }

    /// This app's window, or one of its own (the floating video, a DevTools
    /// window is the engine's and is left alone).
    private static bool Ours()
    {
        var front = GetForegroundWindow();
        return front == window;
    }

    private static IntPtr OnKey(int code, IntPtr message, IntPtr info)
    {
        if (code < 0) return CallNextHookEx(handle, code, message, info);
        var key = Marshal.PtrToStructure<KeyInfo>(info);
        var kind = (int)message;
        var pressed = kind is WM_KEYDOWN or WM_SYSKEYDOWN;
        var released = kind is WM_KEYUP or WM_SYSKEYUP;

        if (released)
        {
            var repeatGone = down.Remove(key.Vk);
            if (Ours()) Shortcuts.Released((VirtualKey)key.Vk);
            if (swallowed.Remove(key.Vk)) return 1;
            return CallNextHookEx(handle, code, message, info);
        }
        if (!pressed || !Ours()) return CallNextHookEx(handle, code, message, info);

        var repeat = !down.Add(key.Vk);
        var vk = (VirtualKey)key.Vk;
        // Modifiers on their own are nobody's shortcut.
        if (vk is VirtualKey.Control or VirtualKey.LeftControl or VirtualKey.RightControl or VirtualKey.Shift
            or VirtualKey.LeftShift or VirtualKey.RightShift or VirtualKey.Menu or VirtualKey.LeftMenu or VirtualKey.RightMenu
            or VirtualKey.LeftWindows or VirtualKey.RightWindows)
            return CallNextHookEx(handle, code, message, info);

        var ctrl = Keys.Down(Keys.Control);
        var alt = Keys.Down(Keys.Menu);
        var shift = Keys.Down(Keys.Shift);
        // AltGr is Ctrl+Alt to Windows, and on half the world's keyboards it
        // types letters — ł, ć, € — that must never be taken for a shortcut.
        if (ctrl && alt && Keys.Down(VK_RMENU)) return CallNextHookEx(handle, code, message, info);
        // With the Windows key held, the key belongs to Windows.
        if (Keys.Down(Keys.LeftWin) || Keys.Down(0x5C)) return CallNextHookEx(handle, code, message, info);

        var inPage = InPage();
        bool taken;
        try { taken = Shortcuts.Take(vk, ctrl, shift, alt, repeat, inPage); }
        catch (Exception e) { Links.Trouble(e); taken = false; }
        if (!taken) return CallNextHookEx(handle, code, message, info);
        swallowed.Add(key.Vk);
        return 1;
    }

    /// A field tagged with this answers Esc itself — a rename in place, where
    /// Esc means "never mind" rather than "put the panel away".
    public const string OwnEscape = "own-escape";

    public static bool FocusOwnsEscape() =>
        App.Root?.XamlRoot is { } root && FocusManager.GetFocusedElement(root) is FrameworkElement { Tag: OwnEscape };

    /// Whether the keyboard is with a page rather than with the browser's own
    /// fields.
    private static bool InPage()
    {
        if (App.Root?.XamlRoot is not { } root) return true;
        return FocusManager.GetFocusedElement(root) is null or WebView2;
    }
}
