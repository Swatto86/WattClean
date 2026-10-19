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
crates/wcc-core             the engine; no Tauri, no Win32
  │  trait System (variables, registry, Recycle Bin, clipboard, running
  │                programs, drives, rights, delete at restart)
crates/wcc-windows          the real PC (the only crate with `unsafe`)
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
| `defs` + `defs/*.toml` | Built-in and custom TOML definitions, templates |
| `cleanerml` | BleachBit CleanerML import (Windows parts) |
| `winapp2` | winapp2.ini import: one cleaner per entry |
| `catalog` | All cleaners together; detection (installed?) and running state |
| `vars` | `%VARIABLE%` expansion (a variable may have several values) |
| `wildcard`, `search` | Globs and walks with filters; links never followed |
| `paths` | Windows path comparison and the `Guard` |
| `ops` | Planning an option's actions into operations |
| `exec` | Carrying out operations (shred, delete at restart) |
| `run` | Preview or clean of a selection, with progress and stop |
| `report` | What a run found or did; progress; the `Sink` trait |
| `sqlite` | History, downloads, form data, site search engines, vacuum |
| `edit` | Removing one key from JSON or one section/parameter from INI |
| `shred` | Overwrite-then-delete; wiping free space |
| `settings`, `store`, `history` | JSON in the data folder |
| `system` | The machine trait; protected folders |
| `fake`, `fake_seed` | The test machine (feature `fake`) |

## Cleaner definitions (`crates/wcc-core/src/defs/*.toml`)

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
category = "browsers"            # system browsers applications developer games deep-scan
template = "chromium"
vars = { root = '%LOCALAPPDATA%\Google\Chrome\User Data' }
detect = ['{root}']              # shown only when one exists (or detect_registry)
running = ["chrome.exe"]
skip = []                        # template options to leave out
```

Actions (`op`): `delete`, `truncate`, `sqlite` (`sql` = vacuum,
chrome-history, chrome-downloads, chrome-autofill, chrome-keywords,
mozilla-history, mozilla-downloads), `json` (`address` a/b/c), `ini`
(`section`, optional `parameter`), `registry` (`key`, optional `value`),
`recycle-bin`, `clipboard`, `custom-paths`. File actions take `path`,
`search` (file, glob, walk.files, walk.all, walk.top, find.dirs), and filters
`regex`, `nregex`, `wholeregex`, `nwholeregex`, `names` (wildcards),
`kind` (file/dir), `older_than_days`, `exclude` (paths; a `*` in an
exclusion spans folders). An exclusion that uses a variable the PC does not
have (other than `%ONEDRIVE%`) makes the action find nothing and report why,
rather than run without it. `walk.all` with a name filter removes matching
files only, never folders. A link to a folder is removed as a folder (the
link only, never its target). Keep-list entries are exact paths: wildcards
are refused. A winapp2 `REG` exclusion inside a `RegKey` drops that
registry action. `find.dirs` looks through every folder below
`path` for folders named by `names`, beside a file matching `beside`
(wildcards, `;` between) when given, and takes each found folder whole
(as walk.top) without looking further inside it. Variables: see `KNOWN_VARIABLES` in
`defs_tests.rs`; `%PROGRAMFILES%` stands for both Program Files folders.

Custom `.toml` and CleanerML `.xml` files go in the data folder's
`cleaners` folder; winapp2.ini is kept there as `winapp2.ini`.

## A run

1. The page sends the ticked keys; the command checks them against the
   catalog and starts the one job (`AppState::start_job`).
2. `run::run` takes the options in catalog order. For each action the
   `Planner` expands variables, finds files (`search`), and asks the
   `Guard`; files already planned by an earlier option are left out.
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

`%LOCALAPPDATA%\WattClean\disk`: `settings.json`, `history.json`, `cleaners\`.

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
build output of Rust, .NET and Python projects, a test machine for
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
- Temporary files are cleaned only when unchanged for a day, so installers
  and programs running now keep theirs.
- Folders are removed only when everything in them went; whatever was
  kept, in use or new keeps its folder.
- A clean always plans again; it never trusts an earlier preview.
- The only network use is the winapp2.ini download the person clicks.
- Built-in definitions are TOML (readable without Rust); CleanerML and
  winapp2.ini are import formats only.
- Build output (Developer tools › Build output) is found by a folder's
  name together with the project file beside it (`target` beside
  `Cargo.toml`, `bin`/`obj` beside a `.csproj`), never by the name alone,
  which would also match a person's own folder called `build` or `target`.
  Python's `__pycache__` needs no partner. It searches the profile only,
  with deep scan's exclusions.
- Select All ticks every option of the cleaners shown, warnings included,
  after one dialog listing the warnings not yet accepted; a full clean
  should not mean a dozen separate questions, nor skip them silently.
- Deep scan skips AppData and developer folders (node_modules, .git,
  target, package caches): they made a scan of a developer's profile take
  over a minute per option and never hold a person's leftovers.

