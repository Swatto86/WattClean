# WattClean

Clean your disk and your registry, and uninstall programs completely, from
one app with a modern Windows 11 look and an icon by the clock. WattClean
brings together what were three tools (WattCompClean, WattRegClean and
WattUninstaller) and replaces them. It runs on Windows, and on Linux and
macOS without the registry tools (see [Linux and macOS](#linux-and-macos)).

## Disk

- **Disk cleaner**: about 70 BleachBit-style cleaners for Windows,
  Explorer, Chrome, Edge, Brave, Vivaldi, Opera, Firefox and its relatives,
  Thunderbird, Discord, Slack, Teams, Spotify, Office, VLC, 7-Zip, Steam,
  Battle.net, World of Warcraft, developer caches, build output (Rust, .NET,
  Python) and more. Preview first: nothing is deleted until you press Clean.
  Options that lose something you might want ask first; **All** ticks
  everything for a full clean after one look at those warnings.
- **Shred files**: overwrite files and folders, then delete them.
- **Wipe free space**: make files deleted earlier unrecoverable.
- Keep list, custom paths, delete in-use files at restart (administrator),
  community cleaners (winapp2.ini), BleachBit CleanerML files, and a
  history of what each clean freed.

## Registry

- **Registry cleaner**: Safe, Moderate, Advanced or Custom scans for
  entries left by programs that are gone, across every account if you
  like. Everything removed is backed up first; a restore point and a full
  registry backup can be made before each clean.
- **Registry search**, **Registry tuner** (about 50 Windows tweaks, each
  undone exactly), **Registry watcher** (logs every change beneath the keys
  you choose and, as administrator, which program made it), **Registry
  backups** (restore whole backups or chosen entries, export as .reg) and
  **Schedules** (automatic cleans and backups through Task Scheduler).

## Programs

- **Uninstall programs**: runs a program's own uninstaller, waits for it,
  then finds what it left behind (Safe, Moderate or Advanced) and removes
  what you tick, with a backup that restores byte for byte. Select several
  programs (Ctrl+click, Shift+click) to uninstall them one after another.
- **Traced installs**: install a program while WattClean records what it
  creates, so its uninstall later removes exactly that.
- **Forced uninstall** for programs whose uninstaller is broken or gone,
  plus **Uninstall backups** and **Uninstall history**.

## Everywhere

- Light and dark themes, accent colours and text size; the window hides to
  the notification area when closed (so the Registry Watcher keeps
  watching), and the icon's menu opens any tool or quits. It can start
  with Windows.
- **Command line**: `wattclean-cmd disk --preview chrome.cache system.temp`,
  `wattclean-cmd disk --clean --preset`, `wattclean-cmd registry -scan
  -safe`, `wattclean-cmd registry -backup -key "HKEY_CURRENT_USER\Software"`.
  `wattclean-cmd --help` lists both.
- No telemetry and no network use, except downloading winapp2.ini when you
  click it.

## Linux and macOS

The disk tools and the uninstaller run on Linux and macOS too; the registry
tools are Windows-only, as only Windows has a registry.

- **Disk cleaner** with each system's own cleaners: temporary files, the
  Trash, thumbnails, caches, session and crash logs, the system journal,
  shell histories, shader caches, browsers (native, Snap and Flatpak builds
  of Chrome, Chromium, Edge, Brave, Vivaldi, Opera, Firefox, LibreWolf,
  Safari, Arc…), chat, music and office apps, Steam and Lutris, Xcode, and
  developer caches (npm, Yarn, pnpm, Bun, Deno, pip, uv, Conda, Cargo, Go,
  Gradle, NuGet, Composer, ccache, Docker, CocoaPods).
- **Package managers**: APT, DNF, Zypper, pacman (and AUR helpers),
  Flatpak, Snap, Nix, Homebrew and MacPorts clean their caches and remove
  what nothing needs through their own commands; the preview shows each
  command and what it lists. Commands that need administrator rights ask
  for your password each time (the system's own prompt); WattClean itself
  never runs as administrator there.
- **Uninstall programs** lists what every package manager installed (APT,
  DNF/Zypper/RPM, pacman, Flatpak, Snap, Homebrew formulae and casks,
  MacPorts), AppImages and app bundles; applications first, every package
  on request. It removes one with its own package manager, optionally with
  its settings, then offers what it left in your home folder or Library,
  nothing ticked, to move to the Trash.
- BleachBit CleanerML files work with their Linux or macOS parts;
  winapp2.ini, deleting in-use files at restart and starting with the
  system are Windows-only.

Linux packages: `.deb`, `.rpm` and AppImage (WebKitGTK 4.1; the AppImage
is the one for Arch Linux and Arch-based systems such as Omarchy; the icon
by the clock needs an AppIndicator-capable desktop, and without one
closing the window quits). macOS: an app in a
`.dmg` (macOS 11 or later); some folders (the Trash, Safari's) need Full
Disk Access in System Settings › Privacy & Security. Data:
`~/.local/share/WattClean` on Linux, `~/Library/Application
Support/WattClean` on macOS. The command line is `wattclean-cmd disk …`.

## Moving from the separate tools

The first time WattClean starts, it copies the settings, history, backups,
keep list, tweak records, watch list and install logs of WattCompClean,
WattRegClean and WattUninstaller into its own folder; the old apps' data
stays where it was. Schedules are not copied (their tasks start the old
app): set them up again under Registry › Schedules, switch on Settings ›
Start with Windows if you used it, then uninstall the old tools from
Windows' Settings › Apps.

## Install

Each release has three downloads:

- `WattClean_X.Y.Z_x64-setup.exe`: installs for all users (asks for
  administrator rights once), with a Start menu shortcut and the command
  line.
- `WattClean_X.Y.Z_x64-portable.exe`: runs without installing (starting
  with Windows needs the installed app).
- `WattClean_X.Y.Z_x64-cmd.exe`: the command line on its own.

Requirements: Windows 10 or 11 (x64) and the Microsoft Edge WebView2
Runtime, which Windows 11 includes. On Windows 10, if the window does not
open, install the Evergreen runtime from
<https://developer.microsoft.com/microsoft-edge/webview2/>.
`SHA256SUMS.txt` lists each file's checksum, and `THIRD-PARTY-NOTICES.txt`
the licences of the open-source packages WattClean is built with (the
installer puts it beside the program).

Machine-wide items (Windows' own temporary folder and logs, machine-wide
registry entries, programs installed for all users) need administrator
rights: click **Restart as administrator** in the status bar.

Data: `%LOCALAPPDATA%\WattClean`. Licence: MIT (see `LICENSE`).
