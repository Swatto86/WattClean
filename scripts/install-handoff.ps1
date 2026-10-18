#Requires -Version 5.1
<#
.SYNOPSIS
  Installs (or upgrades in place) WattClean from the local release build and checks it.

.DESCRIPTION
  Build the installer first (AGENT_RELEASE=1 npx tauri build). WattClean installs for all users
  (Program Files), so the installer asks for administrator rights once (UAC); the app itself is
  then started with ordinary rights.
  1. Stops only the installed WattClean (matched by its exact path) and notes whether it was running.
  2. Records the saved shell settings.
  3. Runs the new setup with /S, elevated, and checks the installed version, the Start menu
     shortcut and the uninstall entry.
  4. Starts WattClean through the Start menu shortcut and checks that its window opens, then starts
     it a second time and checks that the second copy exits (one instance) and the first stays.
  5. Reports which module folders exist (a first start copies the old apps' data into them) and
     checks the shell settings are as before.
  6. Leaves WattClean stopped unless it was running before; -KeepRunning leaves it open.

  A session started from the Claude desktop app may run inside that app's MSIX package, which
  redirects writes under %LOCALAPPDATA%, %APPDATA% and HKCU\Software into a private cache: a
  process started there never reaches the real profile. So WattClean is always started through
  WMI, which makes the new process a child of no package, and the real profile is always read
  through a WMI-started probe (the same approach as WattTask).
#>
[CmdletBinding()]
param(
    # Defaults to the local build of the version in src-tauri/tauri.conf.json.
    [string] $Setup,
    [string] $InstalledExe = (Join-Path $env:ProgramFiles 'WattClean\wattclean.exe'),
    [string] $Log,
    [switch] $KeepRunning,
    # The setup was run by hand (when Windows refuses elevation from this session): only check it.
    [switch] $AlreadyInstalled
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$version = (Get-Content -LiteralPath (Join-Path $root 'src-tauri\tauri.conf.json') -Raw | ConvertFrom-Json).version
if (-not $Setup) { $Setup = Join-Path $root "target\release\bundle\nsis\WattClean_${version}_x64-setup.exe" }
if (-not $Log) { $Log = Join-Path $root 'install-handoff.log' }
$shortcut = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs\WattClean.lnk'

Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public static class WattCleanWindow {
    delegate bool EnumProc(IntPtr hwnd, IntPtr lParam);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc callback, IntPtr lParam);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint processId);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr hwnd, StringBuilder name, int size);

    // 0: no WattClean window yet, 1: hidden, 2: visible.
    public static int State(uint processId) {
        int state = 0;
        EnumWindows((hwnd, lParam) => {
            uint owner;
            GetWindowThreadProcessId(hwnd, out owner);
            if (owner != processId) return true;
            var name = new StringBuilder(64);
            GetClassName(hwnd, name, name.Capacity);
            if (name.ToString() != "Tauri Window") return true;
            state = IsWindowVisible(hwnd) ? 2 : 1;
            return false;
        }, IntPtr.Zero);
        return state;
    }
}
"@

function Write-Log([string] $Message) {
    Add-Content -LiteralPath $Log -Value $Message
    Write-Host $Message
}

# Starts a program through WMI, so the new process is not a child of this session (or of any MSIX
# package around it) and sees the real profile.
function Start-Detached([string] $FilePath, [string] $Arguments = '') {
    $line = ('"{0}" {1}' -f $FilePath, $Arguments).TrimEnd()
    $result = Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{ CommandLine = $line }
    if ($result.ReturnValue -ne 0) { throw "could not start $line (WMI return value $($result.ReturnValue))" }
    return [int] $result.ProcessId
}

function Wait-Exit([int] $ProcessId, [int] $Seconds) {
    $process = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    if ($process) { return $process.WaitForExit($Seconds * 1000) }
    return $true
}

# What the machine and the real profile hold, as JSON, read by a WMI-started process.
$stateScript = @'
$exe = Join-Path $env:ProgramFiles 'WattClean\wattclean.exe'
$data = Join-Path $env:LOCALAPPDATA 'WattClean'
$settings = Join-Path $data 'settings.json'
$link = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs\WattClean.lnk'
$uninstall = Get-ItemProperty 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\WattClean' -ErrorAction SilentlyContinue
[pscustomobject]@{
    exe = Test-Path -LiteralPath $exe
    version = if (Test-Path -LiteralPath $exe) { (Get-Item -LiteralPath $exe).VersionInfo.ProductVersion } else { $null }
    # Plain text: Get-Content attaches file properties that JSON turns into an object.
    settings = if (Test-Path -LiteralPath $settings) { [System.IO.File]::ReadAllText($settings) } else { $null }
    shortcut = if (Test-Path -LiteralPath $link) { (New-Object -ComObject WScript.Shell).CreateShortcut($link).TargetPath } else { $null }
    uninstall = [bool] $uninstall
    modules = (@('disk', 'registry', 'uninstall') | Where-Object { Test-Path -LiteralPath (Join-Path $data $_) }) -join ','
} | ConvertTo-Json -Compress
'@

function Get-State {
    $script = Join-Path $root '.state-probe.ps1'
    $output = Join-Path $root '.state-probe.json'
    Remove-Item -LiteralPath $output -ErrorAction SilentlyContinue
    # The script ends in a pipeline, so the output file is one more stage on its last line.
    ($stateScript.TrimEnd() + " | Set-Content -LiteralPath '$output'") | Set-Content -LiteralPath $script
    try {
        $id = Start-Detached 'powershell.exe' "-NoProfile -ExecutionPolicy Bypass -File `"$script`""
        [void] (Wait-Exit $id 60)
        foreach ($i in 1..20) { if (Test-Path -LiteralPath $output) { break }; Start-Sleep -Milliseconds 250 }
        if (-not (Test-Path -LiteralPath $output)) { throw 'the profile probe produced nothing' }
        return (Get-Content -LiteralPath $output -Raw | ConvertFrom-Json)
    } finally {
        Remove-Item -LiteralPath $script, $output -ErrorAction SilentlyContinue
    }
}

function Get-InstalledProcess {
    Get-Process -Name 'wattclean' -ErrorAction SilentlyContinue | Where-Object { $_.Path -ieq $InstalledExe }
}

function Wait-WindowState([uint32] $ProcessId, [int] $Want, [int] $Seconds) {
    $state = 0
    foreach ($i in 1..($Seconds * 2)) {
        $state = [WattCleanWindow]::State($ProcessId)
        if ($state -eq $Want) { return $state }
        Start-Sleep -Milliseconds 500
    }
    return $state
}

# Program Files and HKLM need administrator rights: Windows asks once (UAC). The request comes from
# a WMI-started PowerShell, outside this session and any sandbox or package around it, which may
# not be allowed to raise a prompt itself. Where Windows still refuses (seen from Claude desktop
# sessions, 2026-10-18), run the setup by hand and pass -AlreadyInstalled.
function Invoke-ElevatedSetup {
    $marker = Join-Path $root '.state-probe.install.txt'
    $elevator = Join-Path $root '.state-probe.install.ps1'
    Remove-Item -LiteralPath $marker -ErrorAction SilentlyContinue
    # A script file rather than -Command, so no quoting passes through WMI's command line.
    @"
try {
    `$p = Start-Process -FilePath '$Setup' -ArgumentList '/S' -Verb RunAs -Wait -PassThru
    [string] `$p.ExitCode | Set-Content -LiteralPath '$marker'
} catch {
    ('cancelled: ' + `$_) | Set-Content -LiteralPath '$marker'
}
"@ | Set-Content -LiteralPath $elevator
    try {
        $id = Start-Detached 'powershell.exe' "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File `"$elevator`""
        if (-not (Wait-Exit $id 300)) { throw 'the installer did not finish within five minutes (is the UAC prompt still open?)' }
        foreach ($i in 1..20) { if (Test-Path -LiteralPath $marker) { break }; Start-Sleep -Milliseconds 250 }
        $outcome = if (Test-Path -LiteralPath $marker) { (Get-Content -LiteralPath $marker -Raw).Trim() } else { 'no result' }
    } finally {
        Remove-Item -LiteralPath $marker, $elevator -ErrorAction SilentlyContinue
    }
    Write-Log "installer result=$outcome"
    if ($outcome -ne '0') {
        throw "the installer did not run to completion: $outcome. Run $Setup by hand, then this script with -AlreadyInstalled."
    }
}

'' | Set-Content -LiteralPath $Log
$wasRunning = $false
try {
    if (-not (Test-Path -LiteralPath $Setup)) { throw "No installer at $Setup. Build it with: AGENT_RELEASE=1 npx tauri build" }
    Write-Log "setup=$Setup"

    $running = @(Get-InstalledProcess)
    $wasRunning = $running.Count -gt 0
    $running | Stop-Process -Force
    $running | ForEach-Object { [void] $_.WaitForExit(10000) }

    $before = Get-State
    Write-Log "before: version=$($before.version) was-running=$wasRunning settings=$([bool] $before.settings) modules=$($before.modules)"

    if ($AlreadyInstalled) {
        Write-Log 'installer: run by hand'
    } else {
        Invoke-ElevatedSetup
    }
    $after = Get-State
    Write-Log "installed version=$($after.version) path=$InstalledExe"
    if (-not $after.exe) { throw "the installer did not put WattClean at $InstalledExe" }
    if ($after.version -ne $version) { throw "expected $version at $InstalledExe, found $($after.version)" }
    if (-not $after.shortcut) { throw "no Start menu shortcut at $shortcut" }
    Write-Log "shortcut target=$($after.shortcut)"
    if ($after.shortcut -ine $InstalledExe) { throw "the Start menu shortcut points at $($after.shortcut), not $InstalledExe" }
    if (-not $after.uninstall) { throw 'no uninstall entry, so it cannot be removed from Settings > Apps' }

    # First start, through the shortcut a user would click: the window opens.
    [void] (Start-Detached 'cmd.exe' "/c start `"`" `"$shortcut`"")
    $started = $null
    foreach ($i in 1..30) {
        Start-Sleep -Milliseconds 500
        $started = Get-InstalledProcess | Select-Object -First 1
        if ($started) { break }
    }
    if (-not $started) { throw 'WattClean did not start from its Start menu shortcut' }
    $state = Wait-WindowState ([uint32] $started.Id) 2 30
    Write-Log "started pid=$($started.Id) window=$(@('none', 'hidden', 'visible')[$state])"
    if ($state -ne 2) { throw 'WattClean started but its window never became visible' }

    # Second start: the running copy is brought forward and the new one exits.
    $second = Start-Detached $InstalledExe
    $exited = Wait-Exit $second 15
    Write-Log "second start: new process exited=$exited"
    if (-not $exited) { throw 'a second WattClean stayed running; there must be one instance' }
    if (@(Get-InstalledProcess).Count -ne 1) { throw 'expected exactly one running WattClean after the second start' }
    if ([WattCleanWindow]::State([uint32] $started.Id) -ne 2) { throw 'the running copy lost its window after the second start' }

    $final = Get-State
    Write-Log "module folders=$($final.modules)"
    if ($final.settings -ne $before.settings -and $before.settings) {
        # Both readings are kept so the difference can be seen.
        $before.settings | Set-Content -LiteralPath (Join-Path $root 'target\handoff-settings-before.json') -NoNewline
        "$($final.settings)" | Set-Content -LiteralPath (Join-Path $root 'target\handoff-settings-after.json') -NoNewline
        throw 'the settings changed during the install check (both readings are in target\handoff-settings-*.json)'
    }

    if (-not ($wasRunning -or $KeepRunning)) {
        Get-InstalledProcess | Stop-Process -Force
    }
    Write-Log 'INSTALL OK'
} catch {
    Write-Log ("ERROR: " + $_)
    Write-Log $_.ScriptStackTrace
    throw
} finally {
    if ($wasRunning -and -not (Get-InstalledProcess) -and (Test-Path -LiteralPath $InstalledExe)) {
        [void] (Start-Detached $InstalledExe)
        Write-Log 'WattClean was running before, so it was started again.'
    }
}
