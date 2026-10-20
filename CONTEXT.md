# WattClean — decisions in force

What was decided for the suite and why. Each engine's own decisions (what
is safe to delete, backups, tracing, BleachBit and Revo parity and what was
left out) are in `docs/disk-cleaner.md`, `docs/registry.md` and
`docs/uninstaller.md`. Code is the ground truth.

## One suite instead of three apps (2026-10-18)

- WattClean replaces WattCompClean, WattRegClean and WattUninstaller
  completely: every feature of each is here, and their WebDriver suites run
  here (99 tests). The old apps were uninstalled from SwatPC and their
  repositories archived on 2026-10-18; their data folders were kept.
- **The engines were moved, not rewritten.** Their crates keep their names
  (`wcc-`, `wrc-`, `wu-`), code and tests; only app names, data folders and
  the command line's usage changed. A rewrite would have thrown away tested
  behaviour (other accounts' hives, ETW tracing, backups that restore byte
  for byte) for no gain.
- **The look is WattTask's**, from WattCompClean's shell, which already
  shared WattTask's tokens. The registry cleaner's and uninstaller's views
  kept their markup and code but take the shell's palette through scoped
  stylesheets, rather than being redrawn page by page.
- **Navigation is grouped by tool** (Disk, Registry, Programs) in one
  sidebar, not three tabbed apps inside a window: every page is one click
  away and the tray can open any of them.
- **Command names and element ids carry a module prefix** (`disk_`, `reg_`,
  `un_`; `reg-`, `un-`) so the three apps' names never clash; the modules'
  `dom.ts` add the id prefix, so their page code is unchanged.
- **Shared commands and settings belong to the shell**: about, restart as
  administrator, quit, theme, accent, text size, the navigation pane and
  keeping running in the tray. Each module keeps its own settings file and
  section of Settings.
- **The tray icon is always there**, and closing the window hides to it by
  default, so the Registry Watcher keeps watching (WattRegClean's reason)
  and the suite feels like one background tool; Quit is in the icon's menu
  and in Settings. Switching "keep running" off makes closing quit.
- **Per-machine install** (`Program Files\WattClean`, one UAC prompt), as
  WattRegClean was: starting elevated at sign-in through Task Scheduler must
  run an executable an ordinary process cannot replace. Data stays per user.
- **The old apps' data is copied at first start** (`migrate.rs`), for each
  module whose folder is missing: settings, history, cleaners, backups,
  tweaks, watch list and install logs. Copied, not moved, so an old app
  keeps working until it is uninstalled. Schedules are not copied: their
  tasks start the old app, so they are set up again here. The WebDriver
  build (its own data folder) never migrates. What could not be copied (a
  locked file) stays listed in the module's `adopting.txt`, is tried again
  at each start without overwriting anything already there, and is shown
  in a dialog until it is in.
- **Restart as administrator waits** (`--after <pid>`): the elevated copy
  waits for the copy that asked to end before its single-instance check,
  which would otherwise hand over to the closing copy and quit.
- **One right-click policy**: the registry cleaner's native menus (Windows'
  edit menu on text, a list's own menu on a list) serve the whole window;
  the uninstaller's separate edit menu was dropped as a duplicate. The
  modules' text stays selectable for copying paths, the shell's chrome does
  not.
- **One command line**, `wattclean-cmd disk …` (BleachBit's options) and
  `wattclean-cmd registry …` (Revo's `RRCcmd` verbs), so scripts keep their
  exit codes; the old console programs became libraries.
- **Rights are reported once**, from the registry module's machine (the
  disk module's on Linux and macOS); on a real PC all three agree. On the test build that means "Administrator",
  which the registry and uninstaller suites expect; the disk engine still
  asks its own machine for its own administrator-only behaviour.

## Linux and macOS (2026-10-19)

- **One app on three systems.** The disk cleaner and the uninstaller run on
  Linux and macOS too; the registry cleaner (and with it schedules, the
  watcher, start with Windows and restart as administrator) is compiled
  for Windows only, since only Windows has a registry. The window asks the
  app which platform it is on (`app_info.platform`) and offers only that
  platform's pages.
- **The disk engine is shared; definitions are per platform.**
  `defs/windows`, `defs/linux`, `defs/macos` and `defs/unix` (both), over
  shared Chromium, Firefox and Electron templates (`defs/templates.toml`).
  Linux and macOS paths use `%HOME%`, the XDG folders, and `%CONFIG%` /
  `%CACHE%` (XDG on Linux, `~/Library/…` on macOS) so one definition serves
  both. The tests check each platform's set uses only its own variables
  and kinds of action.
- **Paths compare by one text rule everywhere**: either separator,
  case-insensitive. On Linux that can only make the guard refuse more. The
  disk engine's existing tests run on Linux too (the test machine stays
  Windows-shaped; definitions may use either separator).
- **Package managers clean through their own commands**, picked by id from
  one table in Rust (`wcc-core/src/tasks.rs`): apt, dnf, zypper, pacman,
  Flatpak, Snap, Nix, Homebrew, MacPorts, journald and developer tools. A
  definition file can only name a task, never a command; names a listing
  step reads back are checked before they are passed on. A program run as
  the account is looked for where package managers install (Apple
  Silicon's Homebrew first, as Homebrew's `shellenv` puts it, so an Intel
  one left in `/usr/local` cannot hide it; pnpm's and the Docker tools'
  macOS homes after the system's), then on `PATH`; only `gem` is looked
  for first in the version managers' folders (rbenv, asdf, mise, rvm), as
  in the account's terminal. Those folders, Homebrew's and `PATH` are the
  account's to write, so a program run as administrator (and an
  administrator task's listing, which decides what it acts on) comes only
  from the system's folders (and MacPorts' on macOS), and only when its
  real path, every folder above it and every folder holding a link on the
  way belong to root and nobody else may write them; a copy that fails is
  passed over, and the check is made again just before it runs
  (`wcc-unix/src/process/lookup.rs`). A task may set variables for its own
  runs, never through the password prompt: `brew cleanup` runs with
  `HOMEBREW_NO_AUTOREMOVE=1`, since uninstalling unneeded formulae is the
  separate, warned autoremove option. System caches
  belong to root, so those commands ask for administrator rights each time
  (polkit's `pkexec`, macOS's own prompt): the app itself never runs as
  root, and there is no "restart as administrator" outside Windows.
- **Programs WattClean starts get the system's own environment**
  (2026-10-20, `wcc-unix/src/process/host.rs`). The AppImage's launcher
  puts its own folders first in `LD_LIBRARY_PATH`, `PATH`,
  `XDG_DATA_DIRS`, `PYTHONHOME`, GTK's and GIO's module and schema paths,
  sets `GTK_THEME` and moves into the AppImage; inherited, they made
  Arch's pacman load the AppImage's older OpenSSL and fail, so on Arch
  (Omarchy) Uninstall programs listed nothing and pacman's cleaning tasks
  could not run, while Ubuntu, where the AppImage is built, never showed
  it. So every program WattClean starts (`process::command`: package
  managers, `pkexec`, `xdg-open`, the clipboard tools) has the AppImage's
  entries taken out of each variable (removed when nothing is left), the
  launcher's own variables removed, and starts in the home folder; `find`
  searches the system's `PATH`. Only when WattClean runs from inside
  `APPDIR` (its own executable is there); elsewhere nothing changes.
- **Linux trays name WattClean by its application name**: libayatana's
  item has no title of its own and falls back to GTK's application name,
  which was the program's file name (`wattclean`, shown as the tooltip by
  Omarchy's bar and KDE); `lib.rs` sets it to WattClean at start.
- **Unix safety rules**: sockets, pipes and devices are never listed (the
  temporary folder holds running sessions'); clearing "read-only" stays a
  Windows-only fallback (on Unix it would make a file world-writable;
  shredding adds only the owner's write bit and puts it back on failure);
  a chosen path (Shred, Custom paths) is taken literally, as names may hold
  `%`, `*`, `?` and `\`; nothing is emptied or edited through a link; the
  temporary-folder option keeps hidden and session entries and anything
  touched in the last two days; a running program's options are left
  alone, since these systems delete files in use (`docs/disk-cleaner.md`).
  `TMPDIR` and `XDG_CACHE_HOME`,
  `XDG_DATA_HOME` and `XDG_CONFIG_HOME` are not trusted when one is, or
  holds, `/`, the home folder, a user folder, a protected home folder or a
  system folder other than a temporary one: it gives way to its usual
  folder. A Linux drive's Trash is used only as the freedesktop.org
  specification says (`.Trash-<uid>` or `.Trash/<uid>` in a sticky
  `.Trash`, real folders of this account, real `files` and `info`), so no
  one can point it at someone's files. The clipboard is judged by the
  kinds of data it lists (`clipboard info` on macOS, X11 targets,
  `wl-paste --list-types`), never by reading it; with only xsel, which
  cannot list them, the clear is always offered. Linux free-space wipes
  skip ZFS (a compressed dataset stores the zeros as nothing, so the wipe
  would never fill it) and read-only mounts, and write in a folder on any
  mount of the drive's device (Btrfs subvolumes).
- **The uninstaller on Linux and macOS is a package engine**
  (`wu-core/src/packages`, `wu-unix`), not the registry one: it lists what
  apt/dpkg, dnf/zypper/rpm, pacman, Flatpak, Snap, Homebrew (formulae and
  casks), MacPorts, AppImages and app bundles installed, shows applications
  (those with a launcher, or bundles) unless asked for every package, and
  removes with the package manager's command, refusing a removal that
  would take other packages with it. Leftovers are matched by
  exact name only, never offered when another installed program goes by
  the name, never pre-ticked, and moved to the Trash (freedesktop.org Trash
  on Linux, Finder on macOS) rather than backed up: the Trash is how these
  systems undo. On macOS an app still open is not removed, a formula's
  background service is stopped first, Homebrew's sudo asks for passwords
  through WattClean's own password box (`SUDO_ASKPASS`), and the
  background jobs that name or start the app are offered too (the
  account's LaunchAgents, `/Library`'s agents and daemons, and the helpers
  those daemons start), their jobs stopped before they go. "Leftovers of
  removed apps" offers the jobs of apps already gone, however they went
  (`docs/uninstaller.md`).
- **winapp2.ini is Windows-only** (its entries are Windows paths); BleachBit
  CleanerML files are read for each platform's own parts, `~` being home.
- **Data folders**: `~/.local/share/WattClean` (XDG) on Linux,
  `~/Library/Application Support/WattClean` on macOS.
- **Bundles**: deb, rpm and AppImage on Linux; app and dmg on macOS; NSIS
  on Windows. Each carries `wattclean-cmd` as a `bundle.externalBin`
  (2026-10-20), built for the bundle's own target by `beforeBundleCommand`
  (`scripts/bundle-cmd.mjs`, lipo joining both for a universal Mac) into
  the gitignored `src-tauri/binaries/`, so Tauri installs it beside the app
  (`/usr/bin`, `Contents/MacOS`, the install folder) and would sign it with
  a signed Mac build (unsigned so far). The fixed `target/release` copy it
  replaces was the host's architecture, unsigned, and missed under
  `CARGO_TARGET_DIR`. The licence notices cover every shipped target;
  `deny.toml` allows ISC and Apache-2.0 WITH LLVM-exception (Linux only)
  and records four "unmaintained" advisories in Tauri's Linux stack, to
  review 2027-01-19.
- **Verification**: the full gate runs on Linux with `e2e/linux/smoke.py`
  (WebKitGTK through tauri-driver: real package managers, read only, and a
  throwaway AppImage uninstalled to the Trash, in a temporary home). On a
  Mac the same gate runs natively (first on 2026-10-19); macOS has no
  WebDriver for WKWebView, so the window is checked by hand.
- **macOS specifics** (from the first run on a Mac): one folder has several
  spellings (`/System/Volumes/Data/…`, `/private/var/…`,
  `/Volumes/<startup disk>/…`, the `/.nofollow/` and `/.resolve/<n>/`
  prefixes, and these nested), so the guard folds them, repeatedly, into
  one key; on Linux and macOS a leading `//` is `/` (no shares). What
  macOS will not let WattClean read (Safari, Mail, other apps' containers)
  is reported with a Full Disk Access hint; WattClean never asks macOS for
  that access itself, but the report (one banner with a count) and
  Settings › About (whether it has it, from listing `~/Library/Safari` or
  `/Library/Application Support/com.apple.TCC`, where only EPERM means no)
  offer a button that opens System Settings at Full Disk Access (a fixed
  address through `/usr/bin/open`). Free-space wipes are offered only for
  local, visible, writable volumes. The Trash is counted from every local volume's Trash
  folder (`~/.Trash`, `.Trashes/<uid>`), and from Finder when one cannot
  be read; Finder empties it and moves removed
  apps there, which macOS asks the user to allow once
  (`NSAppleEventsUsageDescription` in `src-tauri/Info.plist`). Signing
  depends on `src-tauri/Entitlements.plist`
  (`com.apple.security.automation.apple-events`): a signed build runs
  hardened, which otherwise refuses those Apple events with no prompt.
  Developer and deep-scan cleaners never look inside
  libraries, bundles or disk images (`*.photoslibrary`, `*.app`,
  `*.sparsebundle`, OrbStack's folder).
- **The window and quitting on macOS** (2026-10-20): the menu-bar icon (a
  template image) shows its menu on a click; the Dock icon brings the
  window back. The app menu is Tauri's default with Settings… (Cmd+,) and
  a Quit (Cmd+Q) of its own that waits for a running task on a worker
  thread, like the icon's Quit; AppKit's own quit (the Dock's menu,
  logging out) can only wait on the main thread, so it hides the window
  and waits at most a minute for a removal. A window closed in full screen
  goes by hiding the app (Cmd+H), which leaves no empty black Space. The
  `fake-system` build fakes only the disk on Linux and macOS (its pill says
  "Test disk"). An older WebKit without `<dialog>` (before macOS 12.3) is
  told to update instead of running pages whose confirmations cannot open.

## Repository

- Public `Swatto86/WattClean` since 2026-10-18, starting from one commit
  (the earlier private history was not published), so installers can be
  downloaded by anyone; listed on swatto.co.uk. Actions off (as for every
  Watt* repo): the local full gate is the verdict; releases are built and
  published by hand, each with `THIRD-PARTY-NOTICES.txt` as the open-source
  packages' licences ask.
- **Releases** (from v0.1.3, 2026-10-20) carry each system's files, built
  on that system from the tagged commit: `WattClean_X.Y.Z_x64-setup.exe`,
  `_x64-portable.exe` (`target/release/wattclean.exe`) and `_x64-cmd.exe`
  (`src-tauri/binaries/wattclean-cmd-x86_64-pc-windows-msvc.exe`) from
  swatpc; `WattClean_X.Y.Z_universal.dmg` and `_macos-universal-cmd`
  (`wattclean-cmd-universal-apple-darwin`) from the Mac; `_amd64.deb`,
  `WattClean-X.Y.Z-1.x86_64.rpm`, `_amd64.AppImage` and `_linux-x64-cmd`
  from WSL Ubuntu on swatpc (a clone on ext4 with Node 22 in
  `~/.local/node`); plus `THIRD-PARTY-NOTICES.txt` and `SHA256SUMS.txt`.
  The Windows gate's WebDriver suite needs the desktop, so over SSH it runs
  as a scheduled task in the signed-in session (`/IT`); the install check
  runs the same way with `/RL HIGHEST`, so the setup needs no UAC click.
  swatto.co.uk names the standalone command lines through `assets` rules.
- No network access except the winapp2.ini download the person clicks;
  no automatic updates yet (open question in `AGENTS.md`).
