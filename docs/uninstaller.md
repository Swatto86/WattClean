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
deeper down (`Go\src\net\http`) name nothing. Other entries of the same
program (same name or install folder) are leftovers, not neighbours to
protect.
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
  other accounts only `AppData\{Roaming,Local,LocalLow}\<folder>`.
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
  files another is known by. One restore point covers the batch.
- **GitHub Actions is off** for this repo (Swatto's request). The
  workflow in `.github/workflows/` is kept but never runs, so the local full
  gate is the only verdict; do not wait for CI or re-enable Actions unasked.

## Not built (Revo features left out on purpose)

- Install tracing through a kernel driver, as Revo does: needs an EV
  certificate and Microsoft signing; ETW does the job instead (see above).
- Windows Store apps, browser extensions, autorun manager, junk/evidence
  cleaners, hunter mode, full-registry backups: candidates for later units.
