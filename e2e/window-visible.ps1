<#
Say whether one of the processes shows a visible top-level window with this
exact title: prints "true" or "false". The title tells the app's own window
from helper windows (the single-instance plugin keeps a hidden one).
#>
param(
    [Parameter(Mandatory)][int[]]$ProcessIds,
    [Parameter(Mandatory)][string]$Title
)
$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class TopWindows {
    delegate bool EnumProc(IntPtr window, IntPtr data);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc proc, IntPtr data);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr window, StringBuilder text, int max);

    public static bool Visible(uint[] processIds, string title) {
        bool found = false;
        EnumWindows((window, data) => {
            uint owner;
            GetWindowThreadProcessId(window, out owner);
            if (Array.IndexOf(processIds, owner) < 0 || !IsWindowVisible(window)) return true;
            var text = new StringBuilder(256);
            GetWindowText(window, text, text.Capacity);
            if (text.ToString() == title) { found = true; return false; }
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
'@

if ([TopWindows]::Visible([uint32[]]$ProcessIds, $Title)) { 'true' } else { 'false' }
