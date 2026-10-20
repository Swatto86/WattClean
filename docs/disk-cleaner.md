# The disk cleaner (`wcc-*`)

The engine that came from WattCompClean (last release v0.1.1, commit
e2d441e), a BleachBit-style disk-space and privacy cleaner. Its crates
keep their `wcc-` names; the app layer is `src-tauri/src/disk/` and its
pages are the shell's own (`ui/src/pages/`). Where this text says
WattCompClean or "the app", read WattClean's disk cleaner; its data
lives in `%LOCALAPPDATA%\WattClean\disk`.


## Layers

```
ui/ (vanilla TypeScript)    pages: Cleaner, Shred files, Wipe free space,
                             History, Settings. ui/src/api.ts is the only
                             caller of invoke; ui/src/wire.ts decodes results
  │  invoke (+ Channel for progress)
src-tauri/ (wattcompclean)  thin commands, app state (settings, catalog, the
                             one running job), Open dialogs, single instance
  │
crates/wcc-core             the engine; no Tauri, no OS API
  │  trait System (platform, variables, registry, Recycle Bin or Trash,
  │                clipboard, running programs, drives, rights, delete at
  │                restart, finding and running maintenance programs)
crates/wcc-windows          the real Windows PC (`unsafe`: Win32)
crates/wcc-unix             the real Linux or macOS machine (`unsafe`
                             only in sys.rs: geteuid, getuid, access,
                             signal, _exit; statvfs on Linux; statfs,
                             whose sizes are 64-bit, and confstr on macOS)
wcc-core::fake (feature)    a test machine over a sandbox folder
crates/wcc-cmd              wattclean-cmd.exe disk, BleachBit-style console
                             options, on wcc-core and wcc-windows (no Tauri)
```

`wattclean-cmd.exe disk` is built by `beforeBundleCommand` and installed
beside the app by `src-tauri/installer-hooks.nsh`.

## wcc-core modules

| Module | Responsibility |
| --- | --- |
| `model` | Cleaner, option, action, file spec; `cleaner.option` keys |
| `defs` + `defs/` | Built-in (per platform, `defs/built_in.rs`) and custom TOML definitions, templates |
| `cleanerml` | BleachBit CleanerML import (this platform's parts) |
| `tasks` | The one table of maintenance commands (package managers) and their listing parsers |
| `command` | A task in a run: planning, measuring, the preview line, running it |
| `filter` | A file spec's filters, compiled once per run |
| `winapp2` | winapp2.ini import: one cleaner per entry |
| `catalog` | All cleaners together; detection (installed?) and running state |
| `vars` | `%VARIABLE%` expansion (a variable may have several values) |
| `wildcard`, `search` | Globs and walks with filters; links never followed |
| `paths` | Path comparison (one rule on every platform) and the `Guard` |
| `ops` | Planning an option's actions into operations |
| `exec` | Carrying out operations (shred, delete at restart) |
| `run` | Preview or clean of a selection, with progress and stop |
| `report` | What a run found or did; progress; the `Sink` trait |
| `sqlite` | History, downloads, form data, site search engines, macOS's download records, vacuum |
| `edit` | Removing one key from JSON or one section/parameter from INI |
| `shred` | Overwrite-then-delete; wiping free space |
| `settings`, `store`, `history` | JSON in the data folder |
| `system` | The machine trait; protected folders |
| `fake`, `fake_seed` | The test machine (feature `fake`) |

## Cleaner definitions (`crates/wcc-core/src/defs/`)

`templates.toml` (Chromium, Firefox, Electron) serves every platform;
`windows/`, `linux/` and `macos/` hold each platform's cleaners and `unix/`
the ones Linux and macOS share. Paths may use either separator (on Linux
and macOS a backslash in a definition is a separator).

```toml
[[template]]                     # options shared by several cleaners
id = "chromium"
vars = { profile = '{root}\*', cache = '{profile}' }   # defaults
[[template.option]]
id = "cache"
name = "Cache"
description = "…"
default = true                   # ticked in a fresh installation
warning = "…"                    # optional; asked before ticking
actions = [{ op = "delete", search = "walk.all", path = '{cache}\Cache' }]

[[cleaner]]
id = "chrome"                    # letters, digits, _ or -
name = "Google Chrome"
description = "Web browser"
category = "browsers"            # system packages browsers applications developer games deep-scan
template = "chromium"
vars = { root = '%LOCALAPPDATA%\Google\Chrome\User Data' }
detect = ['{root}']              # shown only when one exists (or detect_registry,
                                 # or detect_program = ["apt-get"])
running = ["chrome.exe"]
skip = []                        # template options to leave out
```

Actions (`op`): `delete`, `truncate`, `sqlite` (`sql` = vacuum,
chrome-history, chrome-downloads, chrome-autofill, chrome-keywords,
mozilla-history, mozilla-downloads, quarantine-events), `json` (`address` a/b/c), `ini`
(`section`, optional `parameter`), `registry` (`key`, optional `value`),
`recycle-bin` (the Trash on Linux and macOS), `clipboard`, `custom-paths`,
`command` (`task` = an id from `tasks::TASKS`: a package manager's clean,
run with administrator rights when it needs them, its listing shown in the
preview; a definition can name a task, never a command; the preview's size
comes from the tool's own dry run when it has one, such as `brew cleanup -n`
(one that runs and names nothing to free means 0), and a task whose tool is
missing, such as Xcode's `simctl`, is not offered. A listing only shown
keeps a tap's `user/repo/name` and counts any name it cannot show; names
passed back to the tool must be plain package names). File actions take `path`,
`search` (file, glob, walk.files, walk.all, walk.top, find.dirs), and filters
`regex`, `nregex`, `wholeregex`, `nwholeregex`, `names` (wildcards),
`kind` (file/dir), `older_than_days`, `exclude` (paths; a `*` in an
exclusion spans folders), `keep_link` (a link relative to each found
item's folder, such as `../current`: the item it names, what is in it, the
link and any folder holding either stay; read when planned, so a clean
reads it again; missing, not a link or naming nothing in that folder, and
nothing there goes, with the reason in the report). An exclusion that uses a variable the PC does not
have (other than `%ONEDRIVE%`) makes the action find nothing and report why,
rather than run without it. `walk.all` with a name filter removes matching
files only, never folders. A link to a folder is removed as a folder (the
link only, never its target). Keep-list entries are exact paths: wildcards
are refused. Custom paths and what is shredded (the tool and `--shred`) are
the chosen file or folder exactly as written: no variables, wildcards or
separator changes, since names on Linux and macOS may hold `%`, `*`, `?`
and `\` (only a trailing separator or `.` is dropped: it would make the
system follow a chosen link instead of removing it). The keep list and Custom paths refuse a `%NAME%` (nothing expands
them) but take a lone `%`; shredding takes any. Emptying, SQLite, JSON and
INI edits are never planned on a link (its target was never checked), and
emptying opens with `O_NOFOLLOW` on Linux and macOS, so a link put in the
file's place afterwards is refused. A winapp2 `REG` exclusion inside a
`RegKey` drops that registry action. `find.dirs` looks through every folder
below `path` for folders named by `names`, beside a file matching `beside`
(wildcards, `;` between) when given, and takes each found folder whole
(as walk.top) without looking further inside it. Variables: see `KNOWN_VARIABLES` in
`defs_tests.rs`; `%PROGRAMFILES%` stands for both Program Files folders.
On Linux and macOS: `vars::UNIX_VARIABLES` (`%HOME%`, the XDG folders,
`%CONFIG%` and `%CACHE%` — XDG on Linux, `~/Library/…` on macOS — the user
folders, `%TRASH%`, `%HOMEBREW_CACHE%`, `%GOPATH%`, `%CARGO_HOME%`), and on
macOS only `vars::MACOS_VARIABLES` (`%DARWIN_USER_CACHE%`, the account's
`/var/folders/…/C`, itself protected).

Custom `.toml` and CleanerML `.xml` files go in the data folder's
`cleaners` folder; winapp2.ini is kept there as `winapp2.ini`.

## A run

1. The page sends the ticked keys; the command checks them against the
   catalog and starts the one job (`AppState::start_job`).
2. `run::run` takes the options in catalog order. For each action the
   `Planner` expands variables, finds files (`search`), and asks the
   `Guard`; files already planned by an earlier option are left out. A
   folder the system will not let it list is a problem in the report (once
   per option, with a Full Disk Access hint on macOS), never "nothing found".
3. Preview reports the plan; clean hands each operation to the `Executor`,
   which checks the guard again, deletes (or shreds), and reports what was
   done, what was in use, and what failed.
4. Progress goes to the page through a Tauri `Channel`, at most every 80 ms;
   Stop sets a flag every loop checks.
5. A clean is added to `history.json`.

## Test machine

`fake-system` build feature (never in a release build; `verify.sh` checks
it). The data folder holds `machine\C\…` (real files), `machine\state.json`
(registry, clipboard, running programs, restart deletions) and a custom
cleaner `cleaners\northwind.toml`. Open dialogs answer from `picks.json`.

## Data folder

`disk` in WattClean's data folder (`%LOCALAPPDATA%\WattClean`,
`~/.local/share/WattClean`, `~/Library/Application Support/WattClean`):
`settings.json`, `history.json`, `cleaners`.

# Decisions in force

## Parity with BleachBit

Covered (Windows): preview and clean with per-option warnings; ~70 built-in
cleaners (Windows, Explorer, browsers of the Chromium and Firefox families,
Internet Explorer's cache, chat and media apps, Office, archivers, game
launchers, developer caches, deep scan); hide cleaners for programs not
installed; running-program detection; SQLite history clean-up and vacuum;
JSON and INI edits; registry keys and values; Recycle Bin and clipboard;
custom paths; keep list (whitelist); overwrite before delete; shred files
and folders; wipe free space (as a tool and as System › Free disk space,
which runs after everything else); delete files in use at restart (admin);
exit after cleaning;
CleanerML import; winapp2.ini import and on-request download; command line
with `--list-cleaners`, `--preview`, `--clean`, `--shred`,
`--wipe-free-space`, `--preset`, `--all-but-warning`, `--overwrite`.

Added beyond BleachBit: History page, per-cleaner tick counts, Select All,
build output of Rust, .NET, Swift and Python projects, a test machine for
automated end-to-end tests.

Left out on purpose, with reasons:

- Linux-only cleaners (APT, YUM, localizations, swap and memory wiping):
  the app is Windows-only.
- Make chaff (fake documents for plausible deniability): needs bundled
  corpora and has nothing to do with freeing space or privacy on Windows.
- Wiping the MFT's free records during free-space wipes: slow, and
  NTFS reuses those records quickly anyway; the file-filling pass stays.
- Several languages: English only for now.
- Automatic update checks: see AGENTS.md (open question).

## Behaviour

- Deleting is permanent (not the Recycle Bin), as in BleachBit; the
  confirmation says so, with the warnings of the ticked options and the
  running programs. "Don't ask again" turns it off (Settings).
- A selected program that is running (and installed) is named in the
  confirmation and the report. Windows refuses to delete a file in use, so
  only those are skipped. macOS and Linux delete it from under the program,
  which goes on using it (a database's journal among them), so there the
  program's options are left out of preview and clean alike, with one
  problem per program saying to quit (close) it first.
- On macOS, a free-space wipe's warnings say that while the disk is full
  macOS removes Time Machine's local snapshots and downloaded iCloud
  copies. No lower limit is set: macOS purges at its own low-space
  thresholds, before the disk is full, so stopping short would not help.
- Temporary files are cleaned only when unchanged for a day, so installers
  and programs running now keep theirs.
- Folders are removed only when everything in them went; whatever was
  kept, in use or new keeps its folder.
- A clean always plans again; it never trusts an earlier preview.
- Codex keeps every release it updated from; its old versions are found
  with `keep_link`, so the one `current` names stays (the option is not
  ticked by default, and Codex running leaves it alone).
- macOS options that ask for an administrator's password (Time Machine
  local snapshots, the system log, the DNS cache) are never ticked by
  default. Each run as administrator asks again, so snapshots go in one
  `tmutil deletelocalsnapshots /` run, offered only when `tmutil` lists
  some; the system log is erased without `--uuidtext`.
- The compiled module cache is cleaned inside the account's Darwin cache
  folder, never the folder itself: it holds live system caches (fonts,
  the Dock's icons) that programs keep open.
- Shredding refuses a file whose data has other names (hard links, as
  Windows' own files have): overwriting it would destroy the data under
  every name. It renames the file to a random name first: a folder that
  refuses that would refuse the deletion too, so the file is left as it
  was rather than emptied in place. A file the account may not write gets
  only the owner's write bit on Linux and macOS (read-only cleared on
  Windows), never opened to other accounts, and on Linux and macOS never
  written through a link put in its place. One
  that then cannot be removed gets its own name and permissions back, so
  nothing is left under a random one. The report names the chosen items
  still there, which the Shred page keeps queued.
- A wipe's filler folder is removed with retries for a few seconds (a
  virus scanner may hold the files just written); one left behind is
  removed when the app starts and before the next console wipe. In the
  console, Ctrl+C (or closing the window) stops a wipe, shred or clean
  between items instead of ending the program; a second Ctrl+C ends it.
- From the first wildcard of a path on, no link is entered, whether the
  wildcard or a plain name after it matched it.
- winapp2.ini: an entry whose `DetectOS` range leaves out Windows 10/11
  (both 10.0), or cannot be read, is left out; `Detect=` keys may hold
  wildcards.
- The only network use is the winapp2.ini download the person clicks.
- Built-in definitions are TOML (readable without Rust); CleanerML and
  winapp2.ini are import formats only.
- Build output (Developer tools › Build output) is found by a folder's
  name together with the project file beside it (`target` beside
  `Cargo.toml`, `bin`/`obj` beside a `.csproj`, `.build` beside
  `Package.swift`), never by the name alone,
  which would also match a person's own folder called `build` or `target`.
  Python's `__pycache__` needs no partner. It searches the profile only,
  with deep scan's exclusions.
- Select All ticks every option of the cleaners shown, warnings included,
  after one dialog listing the warnings not yet accepted; a full clean
  should not mean a dozen separate questions, nor skip them silently.
- Deep scan skips AppData and developer folders (node_modules, .git,
  target, package caches): they made a scan of a developer's profile take
  over a minute per option and never hold a person's leftovers.

