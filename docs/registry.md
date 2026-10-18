# The registry cleaner (`wrc-*`)

The engine that came from WattRegClean (last release v0.5.2, commit
7d0e4af), modelled on Revo Registry Cleaner Pro. Its crates keep their
`wrc-` names; the app layer is `src-tauri/src/registry/` and the page
code `ui/src/registry/` (element ids prefixed `reg-`). Where this text
says WattRegClean or "the app", read WattClean's registry cleaner; its
data lives in `%LOCALAPPDATA%\WattClean\registry`. The old app's own
layer diagram still describes the engine; the suite's shell is in
`ARCHITECTURE.md`.


## Layers

```
ui/ (vanilla TypeScript)       six views; ui/src/api.ts decodes every result
  │  invoke
src-tauri/ (wattregclean)      thin commands, the last results, native menus,
                               tray, `--run-job` headless entry
  │
crates/wrc-core                the engine; no Windows API calls
  │  trait System (registry, drives, restore points, shell, change watch,
  │                event tracing, notifications)
crates/wrc-windows             the real machine (winreg + a few Win32 calls)
wrc-core::fake (feature)       an in-memory registry over a sandbox folder
crates/wrc-cmd                 wattclean-cmd.exe registry, the console program, on
                               wrc-core and wrc-windows (no Tauri)
```

`wattclean-cmd.exe registry` is built by `beforeBundleCommand` (release bundles
only) and installed beside the app by `installer-hooks.nsh`.

Dependencies point inward: `wrc-core` knows nothing of Tauri or Win32. Files
named by registry data are checked in `wrc-core` with `std::fs`, so the fake
machine is a temp folder plus a JSON registry.

## wrc-core modules

| Module | Responsibility |
| --- | --- |
| `model`, `keyname`, `format` | Keys, raw values (byte for byte), parsing typed keys, showing data as Registry Editor does |
| `paths`, `check` | The file a command line names (`"…"`, unquoted with spaces, `rundll32 x.dll`, `@x.dll,-1`); whether it is confidently missing |
| `scan` | Categories and modes; `programs` (Run, App Paths, uninstall entries), `caches` (MuiCache, AppCompat, history lists, sounds, cursors, fonts), `classes` (COM, type libraries, file types, context menus, extensions), `machine` (shared DLLs, installer folders, firewall, services, empty keys, include list); exclusions, de-duplication, numbering |
| `search` | Registry Editor-style find over keys, value names and data |
| `backup` | Retention of old cleaner and full backups (`prune`); capture before removal, removal, restore without overwriting, hand backups, `.reg` export; `force` takes ownership of protected keys with their SDDL saved first; `full` saves whole hives (not restored by the app) |
| `jobs` | Schedules: the list, Task Scheduler XML, running a job headless and the notification after it |
| `launch` | Whether this executable is an installed copy Windows may start later |
| `tuner` | The tweak catalogue and switching with exact undo |
| `watch` | Snapshots, differences, one thread per watched key; `explain` turns a reading's changes and the writes traced meanwhile into log entries; each key's log appended to `watch-logs/` and reloaded at start |
| `trace` | Matching a change found to the traced write behind it (the last write of the same kind to the same key or value) |
| `settings`, `store`, `export` | JSON files written atomically; CSV with formula-safe cells |

## Naming the process behind a change (ETW, 0.4.0)

`wrc-windows/src/trace/` runs one real-time ETW session,
`WattClean-Registry`, while the app is elevated and at least one watch is
running (none otherwise). It enables Windows' Kernel-Process provider
(start/stop, to name short-lived processes) and Kernel-Registry with
create, open, close, delete key, set value and delete value. Set and delete
events carry only a key object, so the name of every key object opened
while tracing is kept until it closes, machine-wide; registry events are
read straight from the event data in manifest order (TDH only for process
events). Events arrive per processor, so each waits 300 ms (`REORDER`) and
is applied in timestamp order; Windows flushes buffers every 100 ms. Only
writes beneath watched keys (HKLM and the signed-in account's hives) are
kept, for a minute. Event times come from the processor's counter, which
drifts from the time-synchronised system clock (up to a few hundred ms an
hour); every 10 s the shortest delivery delay seen is taken as the drift and
added to write times. A session left by a crash keeps tracing with no one
reading until the next elevated start replaces it, or
`logman stop WattClean-Registry -ets`.

SetValue events carry the data written and the data it replaced (only the
start of large data: entries then say "first N of M bytes"). The watcher
logs one entry per traced write beneath the key: a single write gives the
change found its process and time, with the readings' whole data; several
writes to one value each get an entry; writes the readings never saw (a key
made and removed, a value changed and changed back) are logged from their
own data. Readings that match are still checked for such writes.

The watcher asks for the writes between the reading before a change and
the one after it (±250 ms), waiting up to a second for delivery. A write
through a handle opened before tracing started (a long-running program's
cached `HKEY_CURRENT_USER`, the app's own hive roots) names only that old
handle's key object; it waits, and when the watcher asks, `trace/handles.rs`
finds the object in Windows' handle table (`NtQuerySystemInformation`,
extended handles), duplicates the handle and asks its name (`NtQueryKey`),
as Process Explorer does; the name is then kept for later writes. Windows
shows object addresses only to administrators (all zero otherwise, seen on
this PC 2026-10-16), and a protected process's handles cannot be
duplicated, so those writes stay unnamed, as do keys under other accounts.
On SwatPC (`trace_check`): 0.4.0 named reg.exe but not the check's own
write through an old handle; 0.4.1 named all four changes (elevated, all
124 of its handles showed their address) and spent 0.5% of one core on
tracing over 10 quiet seconds (this process only: logging itself costs the
writing processes a little more).
The fake attributes writes made through the port to the app's own process,
as Windows would.

## Scan modes

- **Safe**: Run/RunOnce/Policies Run entries whose program is missing (both
  registry views, every scanned account), "on" Task Manager switches for items
  no longer anywhere; App Paths and `Classes\Applications`; MuiCache and
  AppCompatFlags records; SharedDLLs for missing files; uninstall entries
  whose uninstaller and folder are gone (not Windows Installer's); File
  Explorer context-menu verbs running a missing program.
- **Moderate**: Safe plus COM classes whose servers are all missing and type
  libraries whose files are all missing (both views), Windows Installer
  folders, firewall rules for missing programs, sound events, cursors and
  fonts naming missing files, and extensions holding only an unregistered
  file type (or nothing).
- **Advanced**: Moderate plus unused shared DLLs, Win32 services with a
  missing program (never in the Windows folder), empty vendor/product keys,
  file types whose every command is missing, and history lists (RecentDocs,
  RunMRU, TypedPaths, WordWheelQuery, Open/Save dialogs, Paint, WordPad).
- **Custom**: any of the above, plus the include list.

A read-only Advanced dry run on swatpc (2026-10-16) took 2 s and found 1,175
entries, 771 of them MuiCache names of deleted executables; spot checks of
each category were genuine (e.g. App Paths pointing at an older Xbox app
version than the one installed).

## Parity with Revo Registry Cleaner Pro 2.2.5 (0.5.2)

Each feature Revo Registry Cleaner Pro advertises in its interface, command
line and help, against WattRegClean. **SwatPC**: checked on the real machine
(the example programs in `AGENTS.md`). **Tested**: unit tests and the
WebDriver suite on the fake machine. **Out**: left out, reason in
`CONTEXT.md` ("Not built").

| Revo | WattRegClean | Status |
|---|---|---|
| Safe, Moderate, Advanced, Custom (Managed) scans, 16 categories | Same modes; every category | SwatPC (`dry_run`, read-only) |
| Quick Clean | "Scan and clean" (still confirms) | Tested |
| Forced scan: keys, values, data, match case, whole string, where | Search | Tested |
| Scan all Windows accounts | Same (`accounts.rs`) | Tested |
| Exclude external drives, Windows defaults; exclude/include lists with subkeys | Same | Tested |
| Restore point before clean | Same | Tested; Windows may throttle |
| Full registry backup before clean | Same (option, administrator) | Tested; the backup itself SwatPC |
| Force Delete Registry Keys | Take ownership, permissions kept and put back | SwatPC (`force_check`) |
| Context menu (select/unselect, all/none, expand/collapse, regedit, exclude, Search at Google, export) | Native Windows menu; "Expand to Data" is inline | Tested (suite drives the real menu) |
| Export list: txt, xlsx, csv | Same | Tested |
| Registry Tuner, about 55 tweaks | 51; four Windows 10 only | Tested (each switched on and back) |
| Security-weakening tweaks (UAC, SmartScreen, download warning) | — | Out (decision) |
| Explorer tabs, classic ribbon, taskbar side, battery estimate, Rotate, Recycle Bin on removable, legacy boot menu, Defender tray icon | — | Out (no dependable switch, or not registry) |
| "by Revo Registry Cleaner" label; Restart Explorer, Log out, Restart PC | "Changed by WattRegClean"; same three | Tested |
| Registry Watcher with process name and PID (kernel driver) | ETW, no driver, administrator | SwatPC (`trace_check`) |
| Watch start/pause/resume, export and delete log | Same | Start and export tested; pause and clear only offered in the suite |
| Maximum logs' folder size | Per-key cap (10,000 changes, 4 MB) | Out (different limit) |
| Backup Center: restore whole or chosen entries, delete, export | Same, plus `.reg` export | Tested |
| Partial (key) backup | Hand backup | Tested |
| Full registry backup | Same, without SAM/SECURITY | SwatPC (`full_check`) |
| Restore a full backup; offline restore tool | — | Out (unsafe, unverifiable) |
| Auto delete backups: when older than, when larger than | Same; newest always kept | Tested |
| Custom backup / data storage folder | Backups folder, moved not repointed | Tested |
| Scheduled Auto Clean, Auto Backup (full or partial), daily/weekly/monthly | Task Scheduler tasks | SwatPC (`schedule_check`) |
| Auto Clean / Auto Backup notifications | Windows notification | Tested (fake); SwatPC has notifications off |
| Background service; "do not watch when closed" | Tray plus a sign-in task; no service | SwatPC (`autostart_check`) |
| `RRCcmd.exe` | `wattclean-cmd.exe registry` (+ `-scan`, `-backups`, `-restore`) | Tested; installed copy SwatPC |
| Theme, font size, import/export all settings | Same | Tested |
| Languages, highlight colours, skip warnings | — | Out |
| Check for updates | — | Deferred (open question) |
| Licence activation, trial, ads | — | Not applicable |

# Decisions in force

## Decisions (2026-10-16)

- **Only a confident "missing" is offered.** `wrc-core/src/check.rs` reports
  a file missing only for an absolute local path whose lookup says "not
  found"; bare names, network shares, access-denied folders, unplugged or
  removable drives (setting) and the Windows folder (setting) count as
  present. Services inside the Windows folder are never offered.
- **Questionable categories start unticked**: unused shared DLLs, services,
  empty keys (some programs use an empty key as a marker), and every search
  match. Machine-wide and other accounts' entries start unticked without
  administrator rights and cannot be ticked then.
- **Task Manager startup switches** are offered only when set to "on" and
  their item exists in no Run key: an "off" switch records a choice that
  would be lost if the item came back (seen on swatpc: SecurityHealth).
- **Every removal is backed up first** (`backups/<id>/backup.json`, raw value
  bytes, whole key trees), saved before anything is deleted. Restoring never
  overwrites: an entry that exists again is left and reported. A hand backup
  of a key restores only what is missing. Backups export as UTF-16 `.reg`.
- **The page never names what to delete.** A clean sends finding numbers of
  the last scan or search held in Rust; keys typed by the person go through
  `keyname::parse_key` (HKCR refused: it is a merged view).
- **Right-click menus are native.** `src-tauri/src/registry/menu.rs` builds a Windows
  popup menu from the page's item list and returns the item chosen; the page
  never shows the browser's menu (`ui/src/registry/menu.ts`, one listener). Edit
  items on text fields are Windows' own. The e2e suite reads and drives the
  real menu through Win32 (`e2e/native-menu.ps1`).
- **Protected keys are removed only on request** ("Take ownership" in the
  clean dialog, default from Settings, off; administrator only). Before
  anything changes, the owner and permissions (SDDL) of every key involved
  are saved in the backup; the app then makes itself owner with full
  control and removes the entry. A value's key gets its permissions back at
  once; a restored key gets its original owner (e.g. TrustedInstaller) back.
  Scheduled cleans never take ownership.
- **Schedules are Task Scheduler tasks**, `\WattClean\Job <id>`, starting
  the installed exe with `--run-job <id>` as the signed-in person (highest
  rights only when created as administrator); `main.rs` runs the job headless
  before Tauri starts, then shows a toast (AUMID `co.swatto.wattclean`,
  from the installer's shortcut; none for a dev build). A task whose job is
  gone deletes itself. Only an
  installed copy may be recorded (`wrc-core::launch`: not Downloads, Temp or a
  cargo `target` tree); a dev build keeps the job with "Not scheduled".
- **Background running**: "keep running" hides the window to the
  notification area so the watcher goes on. "Start with Windows"
  (`wrc-core::autostart`, installed copy only) switched on as administrator
  is the Task Scheduler task `\WattClean\Start at sign-in`: a sign-in
  trigger for this account, highest rights, so the app starts elevated
  without a prompt and the watcher names programs in the background (Revo
  does this with a service). Switched on without them it is the HKCU `Run`
  value `WattRegClean`; both run `"<exe>" --background`, and switching on as
  administrator replaces the `Run` value. Only administrators can delete the
  task. Single instance (the plugin's hidden window `<identifier>-siw`); an
  elevated copy lets `WM_COPYDATA` from lower rights reach that window
  (`wrc-windows/src/launch.rs`), else a Start menu launch would vanish, and
  a second launch's arguments only ever decide whether to show the window.
  Quit (tray or Settings) is the one exit path. The NSIS uninstall hook
  removes the `Run` value and the tasks, except when an upgrade runs it with
  `/UPDATE`.
- **Tuner tweaks record what they replace** in `tuner.json` before writing
  (values, and keys they create); switching off puts that back exactly. A
  tweak already on when found is switched off to Windows' default. Tweaks
  that weaken security are deliberately not offered: Revo's UAC and
  SmartScreen switches, its downloaded-file warning removal and its network
  drives for elevated apps (`EnableLinkedConnections`). Revo's Windows 10
  only ones (classic volume and battery flyouts, quick action buttons) and
  those that are not registry settings or no longer work on Windows 11
  (legacy boot menu, Explorer tabs, classic ribbon) are left out too. The
  right-click "Take ownership" and "Command prompt here as administrator"
  use the `runas` verb (on files and folders, and on a folder's background),
  so Windows asks for administrator rights each time.
- **The watcher has no driver.** `RegNotifyChangeKeyValue` signals a change
  beneath a key; the watcher re-reads the key and logs the difference. As
  administrator an ETW session names the process and logs every write with
  its data (`watch/explain.rs`), so several writes between readings, or a
  change undone before the next reading, show as Revo's driver shows them;
  a write leaving the data as it was is not logged. Without tracing an
  undone change is missed. It runs only while the app is open (or in the
  tray), and refuses keys over 20,000 subkeys. Each key's log (last 10,000 changes) is appended to
  `watch-logs/<hash>.jsonl` and reloaded at start; a file over 4 MB is
  rewritten with the newest changes that fit in 2 MB.
- **The command line is its own console program**, `wattclean-cmd.exe registry`
  (`crates/wrc-cmd`), with Revo's `RRCcmd.exe` verbs (`-clean`, `-backup
  -key`, `-watch -start|-stop|-del|-export`) plus `-scan`, which changes
  nothing. A console program, not the GUI exe with an attached console: a
  GUI-subsystem exe returns to the prompt at once and its exit code is
  lost. `-clean` is a scheduled Auto Clean's clean (`jobs::clean_ticked`).
  Watch-list changes refuse while the app runs (it would overwrite
  `watch.json`; its single-instance mutex tells), rather than passing them
  to the running copy, which may be elevated: a lower-rights caller must
  not steer it.
- **Lists export as CSV or an Excel workbook**, by the file type chosen in
  the Save dialog (`export::Table`, `xlsx.rs`): a workbook written by hand
  (one sheet, text cells only, stored zip) rather than a spreadsheet
  dependency.
- **Full registry backup, without restore** (`backup/full.rs`,
  `wrc-windows/src/hives.rs`): SOFTWARE, SYSTEM, DEFAULT and the account's
  NTUSER.DAT and UsrClass.dat, saved with `RegSaveKeyEx` beside the
  manifest (administrator only), never SAM or SECURITY (password hashes and
  secrets, and the backup sits in the person's folder). Hives are opened
  read-only to save: the backup-restore option also asks for write access
  once the restore privilege is on (after taking ownership), and SOFTWARE
  refused it (seen on SwatPC, 2026-10-16). Putting a whole registry back is
  deliberately not offered: a `RegReplaceKey` restore was built, but on
  SwatPC Windows would not load even a copy of a saved hive (`reg load`:
  "The filename or extension is too long"), and a hive Windows refuses at
  start-up would stop it starting. Full backups count towards the keeping
  time; schedules (`Work::FullBackup`, administrator only) and
  `wattclean-cmd registry -backup -full` make them too.
- **Registry rights are asked for narrowly** (`wrc-windows/src/registry.rs`):
  `KEY_ALL_ACCESS` on a parent was refused for `HKLM\SOFTWARE` on Windows 11,
  which broke deleting any key directly beneath such a parent (0.2.1).
- **Backup retention** (`keepBackupsDays`, 0 = off) deletes cleaner,
  search and full backups older than that at start-up and after each
  scheduled job; `keepBackupsMb` (0 = off, Revo's "when all backups are
  larger than") then deletes the oldest of them while all backups exceed
  it (`backup/trim.rs`). The newest backup is always kept, so a clean never
  loses the backup it just made; hand backups are never deleted
  automatically but count towards the size.
- **The backups folder moves, it is never just repointed**
  (`backupsFolder`, `backup/relocate.rs`): changing it copies every backup
  there, then deletes the originals, undoing the copy on any failure or
  clash and refusing a folder inside the current one. Saving or importing
  settings never changes it, so no backup is left behind unseen.
- **Settings import** is checked exactly as a save is (1 MB cap), keeps the
  current backups folder, and applies at once; no restart, unlike Revo.
- **Quick Clean** ("Scan and clean") scans, then opens the same clean
  dialog; nothing is removed without that confirmation.
- **Full backup before a clean** is an option (off by default,
  administrator only), made before the restore point; if it fails the
  clean still runs and says so, as for the restore point. Scheduled cleans
  make one when the setting is on and the job runs elevated.
- **Windows 10-only tweaks** (classic volume and battery flyouts, Action
  Center without quick actions, the unsupported-hardware upgrade) refuse on
  build 22000 and later, where Windows ignores them.
- **Other accounts' hives** are opened as in WattUninstaller
  (`wrc-windows/src/accounts.rs`, copied) and released after each scan,
  clean, restore and hand backup.
- **Test machine is a build feature** (`fake-system`), never in a release
  build (verify.sh checks). Exports save to `<data>/exports` there.
- **Data folder:** `%LOCALAPPDATA%\WattClean\registry` (settings.json, tuner.json,
  watch.json, jobs.json, watch-logs/, backups/); `WATTCLEAN_DATA_DIR`
  overrides it.
- **No network access at all.** "Search Google" hands a URL to the default
  browser; the app itself connects to nothing. No updater yet (see open
  questions).

## Not built (Revo features left out on purpose)

- Restoring a full backup, from Windows or from outside it (Revo's
  restore tool): see "Full registry backup, without restore" above; System
  Restore and Windows' own recovery cover a broken registry.
- A Windows service: schedules use Task Scheduler, watching needs the app
  running (in the tray), and the sign-in task starts it elevated so the
  watcher still names programs. Registry defragmentation (no evidence Revo
  has it either).
- Tweaks that weaken security: turning off UAC, SmartScreen or the
  downloaded-file warning (Swatto decided, 2026-10-16).
- Tweaks with no dependable registry switch on Windows 11: File Explorer
  tabs off, the classic ribbon, moving the taskbar to a side (an
  undocumented binary blob; Windows 11 has no side taskbar), the battery
  time estimate (undocumented, differs by build), Rotate in the picture
  menu (belongs to whichever photo app is installed), the Recycle Bin on
  removable drives (not a registry setting). Not tweaks at all: the legacy
  boot menu (boot configuration, `bcdedit`) and the Defender tray icon (on
  by default; a startup item, managed by the cleaner's startup rules).
- Languages other than English; Revo's highlight colours; "skip warning
  messages" (confirmations stay); a maximum size for the watcher's logs
  folder (each key's log is capped instead); "delete only auto backups"
  (hand backups are never deleted; others follow the keeping rules).
