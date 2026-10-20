# WattClean — architecture

One Tauri 2 app over three engines that were separate apps until
2026-10-18: the disk cleaner (WattCompClean), the registry cleaner
(WattRegClean) and the uninstaller (WattUninstaller). Each engine kept its
crates, tests and test machine; the app gives them one window, one tray icon,
one data folder and one command line. It runs on Windows, Linux and macOS:
the registry cleaner exists on Windows only, and on Linux and macOS the
uninstaller is a package engine over the package managers. Each engine's
own structure and the decisions behind it are in `docs/disk-cleaner.md`,
`docs/registry.md` and `docs/uninstaller.md`.

## Layers

```
ui/ (vanilla TypeScript)       the shell (navigation, header, status bar,
                               theme, Settings) and each platform's pages
  │  invoke (+ Channel for progress), events from the tray
src-tauri/ (wattclean)         shell commands, the tray, first-start
                               migration, and thin commands per module:
                               disk/ (all), registry/ and uninstall/
                               (Windows), packages.rs (Linux, macOS)
  │
crates/wcc-core  wrc-core  wu-core        the engines; no Tauri, no OS API
  │  each has its own `System` trait and a `fake` feature (test machine);
  │  wu-core::packages has `PackageSystem` (Linux and macOS)
crates/wcc-windows wrc-windows wu-windows the real Windows PC (`unsafe`)
crates/wcc-unix  wu-unix                  the real Linux or macOS machine
crates/wcc-cmd  wrc-cmd                   console commands, as libraries
crates/wattclean-cmd                      wattclean-cmd: `disk …` and
                                          `registry …` (Windows)
```

Dependencies point inward: no engine knows Tauri, an OS API or another
engine. `wu-unix` uses `wcc-unix::process` (finding and running programs,
with administrator rights) so both modules ask for rights the same way.
`wattclean-cmd.exe` is built by `beforeBundleCommand` and installed beside
the app by `src-tauri/installer-hooks.nsh`.

## The app (`src-tauri/src`)

| File | Responsibility |
| --- | --- |
| `lib.rs` | Composition root: data folder, migration (Windows), each module's machine and state, the watcher, the tray, close behaviour, `--run-job` |
| `commands.rs` | The list of every command (shell, then `disk_*`, `reg_*`, `un_*`): one for Windows, one for Linux and macOS |
| `shell.rs` | `Shell` state (data folder, shell settings, started hidden); about, shell settings, data folder, restart as administrator, quit, `quit_when_idle` |
| `settings.rs` | The shell's settings: theme, accent, text size, navigation pane, keep running in the tray |
| `migrate.rs` | Copying the old apps' data into the module folders at first start |
| `tray.rs` | The notification-area icon: open, a page per module, quit |
| `disk/` | WattCompClean's commands: catalog, preview/clean, shred, wipe, community cleaners |
| `registry/` | WattRegClean's commands: scan/search/clean, backups, tuner, watcher, schedules, start with Windows, native right-click menus (`menu.rs`) |
| `uninstall/` | WattUninstaller's commands (Windows): programs, uninstall sessions, leftovers, backups, history, traced installs |
| `packages.rs` | The uninstaller on Linux and macOS (`un_pkg_*`): list, remove with the package manager (after the steps that stop a background service), leftovers to the Trash (a loaded job stopped first), a leftover shown in the file manager |
| `error.rs` | The one error shape the page receives |

Every command name carries its module's prefix, so the three command sets
never collide. Each module manages its own `AppState` type; the shell's
commands take the states they need. Rights are reported once, from the
registry module's machine on Windows and the disk module's elsewhere (all
agree on a real machine). `registry/`, `uninstall/` and `migrate.rs` are
compiled for Windows only, `packages.rs` for Linux and macOS only.

Closing the window hides it when "keep running in the notification area" is
on (the default) or the app started at sign-in; otherwise, and from the
tray's Quit, `quit_when_idle` stops a running disk job (a wipe gives its
space back) and exits; the Exit event stops the Registry Watcher.

## The window (`ui/`)

- `ui/src/main.ts`: the shell. Pages are grouped Disk, Registry and Programs
  in the navigation (Up and Down move between them); each page is made once
  and stays in the document, only the current one shown. The tray's
  `open-page` event opens a page. `pagelist.ts` holds each platform's pages
  (no Registry group on Linux and macOS); `platform.ts` holds the platform
  the app reported and the words that differ (Recycle Bin or Trash).
- `ui/src/packages/`: the Linux and macOS "Uninstall programs" page and its
  commands' decoders; ids carry `un-pkg-`.
- Disk pages are the shell's own (`ui/src/pages/`, `api.ts`, `wire.ts`);
  `pages/settings.ts` and `pages/general.ts` make the Settings page: the
  shell's groups, the disk cleaner's, then the registry cleaner's and the
  uninstaller's own settings views.
- The registry cleaner (`ui/src/registry/`) and the uninstaller
  (`ui/src/uninstall/`) keep their own code. Their views are static markup
  in `ui/index.html` (hidden in `#module-views`), wired at start by
  `registry/start.ts` and `uninstall/start.ts` and shown inside a scrolling
  page frame. Every id they make or find goes through their `dom.ts`, which
  adds the prefix (`reg-`, `un-`); their dialogs open in their own roots.
- Their stylesheets are scoped to `.reg` and `.un` and take WattTask's
  palette (their colour tokens are mapped onto the shell's at the top of
  each file). One right-click handler serves the window: Windows' edit menu
  on text, a list's native menu on a list (`registry/menu.ts`).
- The webview is bundled content only: strict CSP, no fs, shell, dialog or
  menu permission; machine text reaches the DOM only through `textContent`.

## Data folder

`%LOCALAPPDATA%\WattClean` on Windows, `~/.local/share/WattClean` (under
`XDG_DATA_HOME`) on Linux, `~/Library/Application Support/WattClean` on
macOS (`WATTCLEAN_DATA_DIR` overrides it; `wcc_core::store::app_root`):
`settings.json` (the shell's), and one folder per module, `disk\`,
`registry\` and `uninstall\`, each holding exactly what the old app kept in
its own folder. The engines find their folder the same way
(`wcc_core::store::data_dir`, `wrc_core::store::data_dir`), so the command
line and scheduled jobs agree with the app.

## Tests

- Rust unit tests in every crate. The disk engine's run on every platform
  (its test machine is Windows-shaped; paths may use either separator); the
  registry engine's and the Windows uninstaller's run on Windows only
  (`cfg(test)` with `cfg(windows)`); `wcc-unix`, `wu-unix` and
  `wu-core::packages` run on Linux and macOS (the latter on every platform,
  over a test `PackageSystem`).
- `npm test`: pure page logic (`ui/src/**/*.test.ts`).
- `e2e/`: one WebdriverIO session over the fake-machine debug build. The
  shell's specs and the disk cleaner's are in `e2e/specs/`; the registry
  cleaner's and the uninstaller's in `e2e/specs/registry/` and
  `e2e/specs/uninstall/`, each with a `workspace.ts` pointing at its module
  folder and a `support.ts` that maps its old tabs onto the suite's pages.
  `native-menu.ps1` reads and drives the real Windows menus;
  `window-visible.ps1` asks Windows whether the window shows. Windows only.
- `e2e/linux/smoke.py`: the Linux build's acceptance through tauri-driver
  and WebKitWebDriver, in a temporary home: the shell, a preview on the real
  package managers, and a throwaway AppImage uninstalled to the Trash.
