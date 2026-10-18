<#
Read the native popup menu the app has open, then close it. A Windows menu
is not part of the page, so WebDriver cannot see it; this asks Windows.

Prints the menu as a JSON list, one string per item: "Cut Ctrl+X" for an
enabled item, "(Cut Ctrl+X)" for a greyed one, "-" for a separator, and
"[]" when none of the processes shows a menu within the wait. With -Choose,
the item whose label starts with that text is chosen with the arrow keys and
Enter, as a person would; otherwise the menu is closed with Esc. Exits 1 if a
menu was found but would not close, or the item to choose is not in it.
#>
param(
    [Parameter(Mandatory)][int[]]$ProcessIds,
    [int]$WaitMs = 5000,
    [string]$Choose = ''
)
$ErrorActionPreference = 'Stop'
# Menu labels hold characters such as the ellipsis; the reader expects UTF-8.
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public static class NativeMenu {
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern IntPtr FindWindowEx(IntPtr parent, IntPtr after, string className, string title);
    [DllImport("user32.dll")]
    static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
    [DllImport("user32.dll")]
    static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")]
    static extern bool IsWindow(IntPtr window);
    [DllImport("user32.dll")]
    static extern IntPtr SendMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")]
    static extern bool PostMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")]
    static extern int GetMenuItemCount(IntPtr menu);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern int GetMenuString(IntPtr menu, uint item, StringBuilder text, int max, uint flags);
    [DllImport("user32.dll")]
    static extern uint GetMenuState(IntPtr menu, uint item, uint flags);

    const uint MN_GETHMENU = 0x01E1, WM_KEYDOWN = 0x0100, VK_ESCAPE = 0x1B, VK_DOWN = 0x28, VK_RETURN = 0x0D;
    const uint MF_BYPOSITION = 0x400, MF_GRAYED = 0x1, MF_DISABLED = 0x2, MF_SEPARATOR = 0x800;

    /// A visible popup menu window (class #32768) of one of the processes.
    public static IntPtr Find(uint[] processIds) {
        IntPtr window = IntPtr.Zero;
        while ((window = FindWindowEx(IntPtr.Zero, window, "#32768", null)) != IntPtr.Zero) {
            uint owner;
            GetWindowThreadProcessId(window, out owner);
            if (IsWindowVisible(window) && Array.IndexOf(processIds, owner) >= 0) return window;
        }
        return IntPtr.Zero;
    }

    public static string[] Items(IntPtr window) {
        IntPtr menu = SendMessage(window, MN_GETHMENU, IntPtr.Zero, IntPtr.Zero);
        var items = new List<string>();
        int count = GetMenuItemCount(menu);
        for (uint at = 0; at < count; at++) {
            uint state = GetMenuState(menu, at, MF_BYPOSITION);
            if ((state & MF_SEPARATOR) != 0) { items.Add("-"); continue; }
            var text = new StringBuilder(256);
            GetMenuString(menu, at, text, text.Capacity, MF_BYPOSITION);
            string label = text.ToString().Replace("&", "").Replace('\t', ' ');
            bool greyed = (state & (MF_GRAYED | MF_DISABLED)) != 0;
            items.Add(greyed ? "(" + label + ")" : label);
        }
        return items.ToArray();
    }

    /// Esc, as a person would press it; true once the menu has gone.
    public static bool Close(IntPtr window) {
        PostMessage(window, WM_KEYDOWN, (IntPtr)VK_ESCAPE, IntPtr.Zero);
        return Gone(window);
    }

    /// Down to the item (separators are skipped, as the keyboard skips
    /// them), then Enter; true once the menu has gone.
    public static bool Pick(IntPtr window, int stops) {
        for (int at = 0; at < stops; at++) {
            PostMessage(window, WM_KEYDOWN, (IntPtr)VK_DOWN, IntPtr.Zero);
            System.Threading.Thread.Sleep(30);
        }
        PostMessage(window, WM_KEYDOWN, (IntPtr)VK_RETURN, IntPtr.Zero);
        return Gone(window);
    }

    static bool Gone(IntPtr window) {
        for (int tries = 0; tries < 40; tries++) {
            if (!IsWindow(window) || !IsWindowVisible(window)) return true;
            System.Threading.Thread.Sleep(50);
        }
        return false;
    }
}
'@

$ids = [uint32[]]$ProcessIds
$deadline = [DateTime]::UtcNow.AddMilliseconds($WaitMs)
do {
    $window = [NativeMenu]::Find($ids)
    if ($window -ne [IntPtr]::Zero) { break }
    Start-Sleep -Milliseconds 100
} while ([DateTime]::UtcNow -lt $deadline)

if ($window -eq [IntPtr]::Zero) {
    '[]'
    exit 0
}
$items = [NativeMenu]::Items($window)
ConvertTo-Json -InputObject @($items) -Compress
if ($Choose) {
    $stops = 0
    $found = $false
    foreach ($label in $items) {
        if ($label -eq '-') { continue }
        $stops++
        if ($label.StartsWith($Choose)) { $found = $true; break }
    }
    if (-not $found) {
        [NativeMenu]::Close($window) | Out-Null
        [Console]::Error.WriteLine("no item starting with '$Choose' in the menu")
        exit 1
    }
    if (-not [NativeMenu]::Pick($window, $stops)) {
        [Console]::Error.WriteLine('the menu did not close on Enter')
        exit 1
    }
    exit 0
}
if (-not [NativeMenu]::Close($window)) {
    [Console]::Error.WriteLine('the menu did not close on Esc')
    exit 1
}
