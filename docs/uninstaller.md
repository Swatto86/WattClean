# The uninstaller (`wu-*`)

The engine that came from WattUninstaller (last release v0.4.3, commit
cdb111c), modelled on Revo Uninstaller Pro. Its crates keep their `wu-`
names; the app layer is `src-tauri/src/uninstall/` and the page code
`ui/src/uninstall/` (element ids prefixed `un-`). Where this text says
WattUninstaller or "the app", read WattClean's uninstaller; its data
lives in `%LOCALAPPDATA%\WattClean\uninstall`, and a scan never offers
anything in WattClean's data folder or its program folder.


## Layers

```
ui/ (vanilla TypeScript)      the page: list, wizard, backups, history, settings
  │  invoke (ui/src/api.ts decodes every result)
src-tauri/ (wattuninstaller)  thin commands, one Session per open uninstall
  │
crates/wu-core                 the engine; no Windows API calls
  │  trait System (registry, processes, privileges)
crates/wu-windows              the real machine (winreg + a few Win32 calls)
wu-core::fake (feature)        an in-memory registry over a sandbox folder
```

Dependencies point inward: `wu-core` knows nothing of Tauri or Win32.
Files are handled in `wu-core` with `std::fs`, on paths the registry names, so
the fake machine is just a temp folder plus a JSON registry.

## wu-core modules

| Module | Responsibility |
| --- | --- |
| `programs` | Read the three Uninstall registries (64-bit, 32-bit, per-user); skip updates and child entries; MSI product codes; forced-target lookup |
| `command_line` | Turn `UninstallString` into a program + raw arguments (quoted, unquoted-with-spaces, `MsiExec /I`→`/X`, silent switches for Inno/NSIS) |
| `folders` | Known folders from `User Shell Folders` (so a OneDrive-moved Desktop is found) |
| `identity` (+ `names`) | What a program is known by: names, inner names (only inside the publisher's folder), publisher key, install folders, executables (read before the uninstaller deletes them) |
| `scan` | The three modes over five scanners (`files`, `registry`, `registry::scan_registrations`, `classes`, `deep`); `guard` vets every candidate; results are de-duplicated (a found folder hides its contents) and sized |
| `backup` | Backed-up removal (`remove`) and restore; manifest saved before and during |
| `trace`, `install_log` (+ `noise`) | A traced install's changes, folded into a log of what it left: created vs changed, noise and undone changes dropped, linked to the uninstall key it wrote |
| `msi` | Windows Installer records: packed product codes, component files, upgrade codes |
| `history`, `settings`, `store` | JSON files written atomically; a damaged file is reported, never overwritten |

## Linux and macOS (`wu-core::packages`, `wu-unix`, 2026-10-19)

There is no uninstall registry outside Windows, so the registry engine above
is Windows-only and Linux and macOS have a package engine instead. The app
layer is `src-tauri/src/packages.rs` (`un_pkg_list`, `un_pkg_uninstall`) and
`packages/leftovers.rs` (`un_pkg_orphans`, `un_pkg_trash_leftovers`,
`un_pkg_reveal`), and the page `ui/src/packages/` (ids `un-pkg-`; the
leftovers dialog in `leftovers.ts`).

| Module | Responsibility |
| --- | --- |
| `packages` | `Package`, `Manager`, the `PackageSystem` port; `list` (applications first; applications only unless every package is asked for) |
| `packages::linux` | dpkg, rpm, pacman (the system's own is the first of them that lists anything: Arch packages dpkg and rpm for converting other systems' packages, with nothing in their databases), Flatpak, Snap, Homebrew, AppImages in `~/Applications` and the like |
| `packages::launchers` | A system package is an application when it installed a visible launcher in `/usr/share/applications`, whose name it takes; owners from `dpkg -S`, `pacman -Qo` (both name the file) and `rpm -qf` (lines taken in order only when there is one per file, else asked file by file) |
| `packages::macos` | App bundles in `/Applications` and `~/Applications` (Apple's own left out), Homebrew formulae and casks (a cask takes its app's place, bundle id, version and size, matched by the app it installs or its uninstall step deletes; fonts and plugins are not applications; a formula's size from the Cellar beside the `brew` found), MacPorts |
| `packages::remove` | The removal for each manager: `apt-get remove`/`purge`, `dnf`/`zypper`/`rpm`, `pacman -R`/`-Rns`, `flatpak uninstall` (`--delete-data`), `snap remove` (`--purge`), `brew uninstall` (`--cask --zap`: Homebrew's whole zap list, which may remove files other apps share, outside the home folder too, as the box says), `port uninstall`; an AppImage or bundle goes to the Trash. On macOS: refused while the app (a bundle, or a cask's app) is open; a formula's service stopped first; Homebrew run with `SUDO_ASKPASS`; the `launchctl bootout` that stops a leftover's job by its label (`stop_job`: `gui/<uid>/<label>`, or `system/<label>` as administrator) |
| `packages::others` | What else a removal would take: APT simulated first (`apt-get -s`), dnf and zypper tested with `rpm -e --test --allmatches` |
| `packages::launchd` | A launchd plist's label, program, `AssociatedBundleIdentifiers` and every text it holds; a job's `Domain` (the account's session or the system); the account's LaunchAgents a removed program leaves, and a formula's root service plist |
| `packages::daemons` | Every plist in the account's LaunchAgents, `/Library/LaunchAgents` and `/Library/LaunchDaemons`; the system-wide ones a removed app leaves, and the helpers in `/Library/PrivilegedHelperTools` its daemons start |
| `packages::orphans` | "Leftovers of removed apps" (macOS): the jobs whose app is gone, judged by the apps they name (the listing, then Spotlight's `mdfind`) or the app bundle their program lay in, and their daemons' helpers |
| `packages::leftovers` | Folders and files named exactly after the program in `~/.config`, `~/.local/share`, `~/.cache`, `~/.local/state`, `~/.var/app`, `~/snap`, home dot-folders, launchers and autostart entries, and the `~/Library` folders on macOS (by bundle id and name, `<TeamID>.<bundle id>` and `group.<bundle id>` by the bundle id only, and `<bundle id>.<suffix>` entries such as updaters unless a longer installed id owns them); Apple's own `com.apple.…` entries never, unless the app is Apple's |
| `packages::extra` | macOS leftovers names cannot reach: a cask's own zap list (only `~/Library/…`, at least two folders down; left out: entries another installed cask shares, entries named after another installed program's name or bundle id, and a vendor's entry straight under a Library folder such as `Application Support/Microsoft` while another app of that vendor stays, the vendor read from the bundle id or, once the app is gone, from the identifiers its zap list names) and the folder a lone app came in (`/Applications/World of Warcraft` once no app is left anywhere in it, four folders down, links not followed) |
| `wu-unix` | The real machine: programs found and run through `wcc-unix::process` (with the system's own environment, not an AppImage's; polkit's `pkexec` or macOS's prompt for administrator rights; a removal run as administrator takes its program only from root's own folders, as the disk cleaner's tasks do), `Info.plist` and launchd plists through `plutil`, running programs from `ps -axo comm=` (full paths on macOS), the askpass script (`askpass.rs`), `open -R` or `xdg-open` to show an item, the freedesktop.org Trash on Linux (folders and records it creates are the account's alone, 0700 and 0600) and Finder's on macOS |

Decisions in force:

- **The package manager removes**, never WattClean by hand: it knows the
  files, runs its scripts and keeps its database right. Names passed to it
  must look like package names (no options, no shell characters).
- **A removal is confirmed by the next listing**: when the program is still
  listed, the removal failed (or the prompt was refused) and nothing is
  offered as left over. A failure reports the package manager's error (its
  last `Error:`/`E:` line on stderr, or the first line of the failed
  step's output that Homebrew's error introduces, else its last line
  there), not the progress it printed on stdout.
- **A removal takes only the program chosen.** APT, dnf and zypper, told
  yes, also remove every package that needs it (the desktop, for a
  library), so they are asked first without administrator rights
  (`apt-get -s`, `rpm -e --test`). When anything else would go, or the
  answer cannot be read, nothing is removed and the message names the
  others (five, then how many more): removing them all is left to the
  package manager itself. dpkg, rpm and pacman refuse such a removal
  themselves.
- **Leftovers: exact names only, never another's.** A name another
  installed program goes by (Firefox from APT next to Firefox from Snap) is
  never offered, nor generic folder names (`fonts`, `autostart`, `cache`…).
  A command-line tool's folders (`~/.name`, `~/.config/name`…) stay while a
  program of that name is still found (Apple's git after Homebrew's goes),
  and an app, cask, Flatpak or Snap never offers a `~/.name` folder (the
  Claude app is not Claude Code's `~/.claude`). While another copy of the
  app (the same bundle identifier) stays installed, nothing is offered: the
  copies share every file. Apple's own entries (`com.apple.news.tag`,
  `group.com.apple.notes`) are never offered for another app: Apple's apps
  are never listed, so no name check could hold them back. A cask's zap
  list is checked against every installed program, not only casks.
  A leftover whose size cannot
  be read says "size unknown". None is pre-ticked, and the chosen go to the Trash: Linux and macOS undo
  through the Trash, so no backup of WattClean's own is kept.
- **An open app is not removed** (macOS): moved while it runs, an app runs
  on from the Trash and writes its folders again after they were offered.
  A bundle (or a cask's app) is refused, "<name> is open: quit it, then
  uninstall.", while `ps` shows a program whose path lies inside it (a
  whole-name prefix ending in `/`, letter case ignored). If `ps` cannot
  run, nothing is refused. On Linux nothing is checked (`ps` prints names).
- **A formula's background service is stopped first** (macOS), since
  `brew uninstall` leaves it running from a deleted folder, started again
  at every login or boot. The account's own (`~/Library/LaunchAgents/
  homebrew.mxcl.<name>.plist`): `brew services stop <name>`, which deletes
  its plist. Root's (`/Library/LaunchDaemons/…`): `launchctl bootout
  system/homebrew.mxcl.<name>` as administrator (Homebrew will not run as
  root from an app), then its plist is offered as a leftover by its exact
  name (Finder asks for a password to move it). Only a job launchd has
  loaded (`launchctl print`) is booted out; when both plists exist,
  Homebrew may refuse to stop the account's own (it does when that job is
  not loaded), so it is left to the leftovers. A step that fails stops the
  removal.
- **Homebrew asks for passwords through WattClean** (macOS). Homebrew runs
  a cask's `uninstall delete`/`pkgutil` steps through sudo, which has no
  terminal in an app (and under `tauri dev` would wait on the terminal), so
  Homebrew's removals run with `SUDO_ASKPASS` naming a fixed script
  written afresh (mode 0700) as `<data>/uninstall/askpass.sh`: macOS's
  dialog with a hidden answer, titled WattClean, generic text (no name
  from outside goes into it), giving up after two minutes; cancelling or
  giving up exits non-zero, so sudo gives up. WattClean never sees the
  password.
- **Background jobs are leftovers** (macOS, the account's
  `~/Library/LaunchAgents` only): a plist whose `AssociatedBundleIdentifiers`
  holds the app's bundle id, whose program lies inside the removed bundle,
  or whose file name or label is one of the program's own names (or
  `homebrew.mxcl.<formula>`), unless the other name is another program's.
  Read with `plutil`, as is a LaunchAgents plist a cask's zap list names,
  so its job is stopped too; WattClean's own (`co.swatto.wattclean…`)
  never.
  Before a ticked one moves, a loaded job is stopped with `launchctl bootout
  gui/<uid>/<label>` (no administrator rights; the label must look like a
  name; checked on this Mac with a throwaway agent, 2026-10-20); one that
  cannot be stopped is not moved and is reported. Putting a plist back from
  the Trash takes effect at the next login.
- **System-wide jobs and helpers are leftovers** (macOS, 2026-10-20), by
  what they say, never by name: a plist in `/Library/LaunchAgents` or
  `/Library/LaunchDaemons` is the app's only when its
  `AssociatedBundleIdentifiers` holds the app's bundle identifier (whole,
  in the same letters) or its program (`Program`, else the first of
  `ProgramArguments`) lies inside the removed bundle; one that also names
  another installed app, a link, and WattClean's own stay. A helper is
  offered only when it is a file directly in `/Library/PrivilegedHelperTools`,
  the program of one of the app's daemons (an agent runs as the account,
  which a helper is not for), and no other plist (the account's own
  included) mentions its path anywhere, in any letters; while any plist
  cannot be read, no helper is offered, since that one might start it. A
  helper goes with its daemon's job. A daemon is stopped with `launchctl
  bootout system/<label>` as administrator (macOS's prompt), an agent in
  `/Library/LaunchAgents` in the account's own session (other accounts'
  copies stop at their next login). They go to the Trash through Finder
  like everything else: its scripted delete asks for an administrator's
  password for an item only root may move, and then moves it (tried on
  2026-10-20 with a throwaway root-owned file in a root-only folder in
  `/Users/Shared`), so no other way of moving them is needed. Each item
  asks once. The real case: OrbStack's `dev.orbstack.OrbStack.privhelper`
  daemon and helper, left after OrbStack.app went, are matched by the
  bundle identifier their plist names (`dev.kdrag0n.MacVirt`).
- **Leftovers of removed apps** (macOS, `un_pkg_orphans`): the background
  jobs of apps already gone, however they went (dragged to the Trash,
  removed by another tool), from the account's LaunchAgents and
  `/Library`'s two folders. A job is listed only when either every app its
  `AssociatedBundleIdentifiers` names is neither listed nor found by
  Spotlight (`mdfind 'kMDItemCFBundleIdentifier == "<id>"c'`, results in a
  Trash ignored; an identifier with other characters is never put in a
  query, and its job is not listed), or its program lay inside an app
  bundle (the outermost `.app`) that no longer exists. A job naming an
  installed app, or whose program's app is still there, is never listed,
  nor one on another disk (`/Volumes/…`, maybe only unplugged). A job that
  merely points at a missing program is not an orphan: a tool's path
  changes when it is upgraded (the agent-standards collector's old
  PowerShell path). Spotlight is trusted only once it finds an app known
  to be there (the first listed app with a bundle identifier, else
  Finder); otherwise only the program rule is used and the page says so.
  The helpers of the daemons found go with them, as above. The list
  replaces a removal's leftovers, so the search is marked as a removal
  while it runs; it is offered in the same dialog, nothing ticked.
- **A leftover can be shown before deciding**: "Show" beside each one
  (`un_pkg_reveal`, by number) runs `open -R` (Finder selects it) or opens
  the folder holding it with `xdg-open` (left running, reaped when it
  ends).
- **Apps are what people start**: packages with a visible launcher, Flatpak
  and Snap applications (not runtimes or base snaps), AppImages, bundles,
  casks, formulae and ports asked for by name. "Show every package" lists
  libraries too.

## Scan modes

Every mode checks the program's install log first (when it was installed
with tracing), then its install folders and uninstall entry.

- **Safe**: folders/keys named exactly after the program in Program Files,
  ProgramData, AppData, LocalLow, Common Files and the Software keys; the
  same one level inside the publisher's folder/key; the publisher's
  folder/key itself when it holds only this program or is empty (an
  uninstaller took the program's key and left the shell) and no other
  installed program shares the publisher; Start Menu folders and shortcuts
  (by target when the `.lnk` names a local path, by site for `.url`, else by
  name), three levels deep, and a publisher's Start Menu folder holding only
  the program's shortcuts; Windows Installer product and feature records;
  other uninstall entries of the same program (same name) and their
  Explorer cache records.
- **Moderate**: Safe plus name prefixes and executable-named data folders,
  and every place Windows records the program by path: App Paths, Run and
  RunOnce with their StartupApproved entries, SharedDLLs, services (by file,
  or by exact name when the file is gone), event-log sources, firewall rules,
  fonts, MuiCache, Installer folders, compatibility records, scheduled tasks
  (task record, Tree entry, trigger list and definition file), default-program
  (`Clients`) registrations and the RegisteredApplications values naming them,
  COM classes and type libraries, document types and the open-with entries
  naming them (a type's default handler is offered unticked), `Applications`
  and `AppID` entries, Explorer shell-extension registrations, and Windows
  Installer component and upgrade-code records of an MSI product.
- **Advanced**: Moderate plus COM interfaces proxied by its classes and the
  AppID its classes share, other user accounts (their data folders, and in
  their own registry: keys named after the program, duplicate uninstall
  entries, Run and App Paths entries, COM classes and document types; a
  signed-out account's hive is loaded for the scan and unloaded after),
  a bounded walk of the software registry for key names containing the
  program's name and values pointing into its folders, and two-level
  data-folder and Temp searches. Low-confidence finds start unticked.

Identity (what a program is known by) comes from its uninstall entry, its
icon and uninstaller paths, Windows Installer's component records and
product name for MSI products, and the targets of Start Menu and desktop
shortcuts named after it when the entry names no folder. A folder inside a
known one is folded into it, and a folder's name counts as the program's only
directly under a root (`Program Files\Notepad++`) or under its publisher's
folder (`Program Files\Acme\Widget`, inside Acme's only); component folders
deeper down (`Go\src\net\http`) name nothing. A component several products
share (or Windows keeps for good) names no folder. Other installed programs
are known the same way (their components and shortcuts read once for all of
them), so an MSI product with no install location still protects its
folder. Other entries of the same program (same id or name) are leftovers,
not neighbours to protect; a differently named program installed in the
same folder keeps that folder (every Windows SDK part shares `Windows
Kits\10`).
## Install tracing (ETW, 2026-10-14)

In the app since 0.2.0: `wu-windows::traced` runs the installer under the
session below, follows Windows Installer processes too (including a service
already running), and converts events into `wu_core::trace::Change`s. A
value set on an existing value (the event's previous type or size is not
zero) is a change, never offered for removal.

### Prototype findings

`wu-windows::trace` runs a private real-time ETW session (administrator
rights, no driver) on Windows' Kernel-Process, Kernel-File and
Kernel-Registry providers, keeps only events from the traced installer's
process tree, and maps kernel paths (`\Device\HarddiskVolume3`,
`\REGISTRY\MACHINE`, `\REGISTRY\USER\<sid>`) to drive letters and HKLM/HKCU.
Set and delete registry events carry only a key object, so key paths are
rebuilt from the open/create events before them, sorted by timestamp.

Live runs on this PC with the WattUninstaller NSIS setup:

- Install (2.6 s, 0 events lost): every installed file and the install folder,
  both shortcuts, the Uninstall key with its 10 values and
  `HKLM\SOFTWARE\Swatto\WattUninstaller`.
- Uninstall (3.6 s, 0 lost): followed the uninstaller's relaunched copy
  (`%TEMP%\~nsu1.tmp\Un.exe`) to completion; every deletion recorded. No
  deletion of `HKLM\SOFTWARE\Swatto\WattUninstaller` was seen, so the
  uninstaller probably leaves it behind (not confirmed by reading the key).
- Noise to exclude before using a log: Restart Manager session keys (created
  and deleted during the run), Internet Settings ZoneMap, TIP
  AggregateResults, Explorer StartPage, PendingFileRenameOperations (the
  uninstaller's own temp copy), temp files. One or two keys per run stay
  unnamed (`?`), opened relative to a handle opened before tracing.
- Untested: MSI installs (`--msi` follows msiexec processes started during the
  run, a heuristic), services that install work through the Service Control
  Manager, long installs under heavy system load.

# Decisions in force


- **Safety over completeness.** `wu-core/src/scan/guard.rs` refuses Windows'
  trees, shared roots, user documents, `.ext` and shell-class keys (except one
  extension's own `shellex` entry), Microsoft's trees (except Uninstall, App
  Paths and Windows Installer product records), and anything another
  installed program uses (its uninstall key, its folder, its exact name, its
  publisher's folder while it is still installed). A missed leftover costs
  kilobytes; a wrong deletion can break Windows. The only files allowed under
  Windows are a scheduled task's definition (never Microsoft's), and under
  other accounts only `AppData\{Roaming,Local,LocalLow}\<folder>` (never
  `Local\Packages` or `Package Cache`; inside `Local\Programs` and
  `Local\Temp` only a folder one level down). `Clients` and its category
  lists stay, in both registry views; one client's key below may go.
- **Guard refinements.** Windows' own file types and URL protocols (txtfile,
  htmlfile, http(s), mailto, batfile…) join the shell classes: never offered
  whole. A deep search offers a value only outside Microsoft's trees (except
  the lists the dedicated scanners clean) and outside another installed
  program's key. A program installed inside the target's folder keeps its
  services, Run entries and the like (`Identity::with_others`). A type
  library shared with another program loses only the target's versions. A
  `.url` shortcut is offered only when its address lies on the program's own
  site, path included (never by a shared host such as github.com). Another
  account's still-installed copy (uninstaller or install folder present) is
  not offered. A forced uninstall refuses a folder that is Windows', a root
  or shared (`Identity::is_own_folder`).
- **A folder names the program only directly under a root or under its
  publisher's folder** (`wu-core/src/identity.rs`, `name_from_dir`). Windows
  Installer registers a component for every file, so Go's records named
  hundreds of folders (`src`, `net`, `http`, `json`…) that 0.4.0 took as
  names of the program and matched all over the machine. Such folders are
  folded into the install folder they sit in and name nothing.
- **Every removal is backed up first.** `backups/<id>/backup.json` is written
  before anything is deleted and after each item. Files are moved, not
  copied, into the backup (instant on one drive); registry keys are exported
  whole with raw value bytes. A file in use is copied, then left to Windows to
  delete at restart. An original is deleted only after its copy is complete;
  a failed copy leaves it in place and the item failed. A restore never
  overwrites something new at the original place (a file, a folder, a
  registry value or a key that exists again with other contents; identical
  data counts as restored). Items that could not be put back stay
  restorable and are retried by the next restore; the backup is closed
  only when none failed. An item still Pending after a crash is restored
  when its original is gone and its data is in the backup.
- **Uninstallers run inside a job object** so self-relaunching uninstallers
  (NSIS, Inno Setup) are waited for; the person can always continue by hand.
- **Not elevated by default.** The app starts with the user's rights (per-user
  programs need nothing more) and offers "Restart as administrator"; Windows
  refuses to start an elevated uninstaller from an unelevated process, so a
  machine-wide uninstall asks for the restart.
- **Install with tracing (0.2.0) uses ETW, not a driver.** A traced install
  saves `logs/<id>.json`: items it created (offered at uninstall in every
  mode, still through the guard) and items it changed that existed before
  (shown, never removed). Noise and anything undone during the install are
  dropped (`wu-core/src/install_log`). Tracing needs administrator rights.
  A log links to a program through the uninstall key the install wrote.
- **Other accounts' registries are opened on demand and released after
  every phase.** `HKU\Account-<name>` (`RegPath::account_root`) reaches an
  account's `NTUSER.DAT`/`UsrClass.dat`. `wu-windows::accounts` maps a
  signed-in account to its SID, opens a signed-out one's hive file directly
  (`RegLoadAppKey`: no mount, no privilege) and, for a hive with unsaved
  changes (`ERROR_BADDB`), falls back to mounting it (`RegLoadKey`,
  SeBackup/SeRestore, administrator only). Every open hive is closed and every
  mount unmounted after each scan, removal and restore and on exit, with
  stale `Account-*` mounts unmounted at start-up: a mount left behind
  breaks that account's next sign-in. On swatpc `reg load` itself fails
  for every hive (2026-10-14), so dirty hives there are reported, not read.
  The guard judges such keys as their HKCU twins.
- **A bundle installer's cache is its own.** `Package Cache\{id}` one level
  down is offered only for a program whose entry had `BundleCachePath` or
  `BundleProviderKey`; its dependency records under
  `Classes\Installer\Dependencies` likewise. The app's data folder and its
  own program folder are never offered (`Context.protected`).
- **Forced uninstall adopts a listed program** found by folder or exact name,
  so its own entry and folder are the target rather than "another program".
- **Test machine is a build feature** (`fake-system`), never in a release
  build (verify.sh checks). The e2e build alone may close its window from the
  page (`e2e/tauri.conf.json`).
- **Data folder:** `%LOCALAPPDATA%\WattClean\uninstall` (settings.json,
  history.json, backups/); `WATTCLEAN_DATA_DIR` overrides it.
- **No network access at all.** No updater yet (see open questions).
- **Several programs are uninstalled one at a time** (2026-10-19,
  `ui/src/uninstall/batch.ts`): one confirmation and one set of options,
  then each program's whole wizard (uninstaller, scan, leftovers) before
  the next starts, so two uninstallers never run together. Every chosen
  program's session (its identity and executables) is opened before the
  first uninstaller runs, because one program's uninstaller may delete
  files another is known by. One restore point covers the batch, named
  for it ("Before uninstalling A and 2 more programs").
- **A removal judges its leftovers again** (`scan::still_allowed`) against
  the machine as it is then: a program installed since the scan keeps what
  it uses, and such an item is reported as left in place.
- **GitHub Actions is off** for this repo (Swatto's request). The
  workflow in `.github/workflows/` is kept but never runs, so the local full
  gate is the only verdict; do not wait for CI or re-enable Actions unasked.

## Not built (Revo features left out on purpose)

- Install tracing through a kernel driver, as Revo does: needs an EV
  certificate and Microsoft signing; ETW does the job instead (see above).
- Windows Store apps, browser extensions, autorun manager, junk/evidence
  cleaners, hunter mode, full-registry backups: candidates for later units.
