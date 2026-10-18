# WattClean — architecture

One Tauri 2 app over three engines that were separate apps until
2026-10-18: the disk cleaner (WattCompClean), the registry cleaner
(WattRegClean) and the uninstaller (WattUninstaller). Each engine kept its
crates, tests and test machine; the app gives them one window, one tray icon,
one data folder and one command line. Each engine's own structure and the
decisions behind it are in `docs/disk-cleaner.md`, `docs/registry.md` and
`docs/uninstaller.md`.

## Layers

```
ui/ (vanilla TypeScript)       the shell (navigation, header, status bar,
                               theme, Settings) and three modules' pages
  │  invoke (+ Channel for progress), events from the tray
src-tauri/ (wattclean)         shell commands, the tray, first-start
                               migration, and one folder of thin commands
                               per module: disk/, registry/, uninstall/
  │
crates/wcc-core  wrc-core  wu-core        the engines; no Tauri, no Win32
  │  each has its own `System` trait and a `fake` feature (test machine)
crates/wcc-windows wrc-windows wu-windows the real PC (all `unsafe` code)
crates/wcc-cmd  wrc-cmd                   console commands, as libraries
crates/wattclean-cmd                      wattclean-cmd.exe: `disk …` and
                                          `registry …`
```

Dependencies point inward: no engine knows Tauri, Win32 or another engine.
`wattclean-cmd.exe` is built by `beforeBundleCommand` and installed beside
the app by `src-tauri/installer-hooks.nsh`.

## The app (`src-tauri/src`)

| File | Responsibility |
| --- | --- |
| `lib.rs` | Composition root: data folder, migration, each module's machine and state, the watcher, the tray, close behaviour, `--run-job` |
| `commands.rs` | The one list of every command (shell, then `disk_*`, `reg_*`, `un_*`) |
| `shell.rs` | `Shell` state (data folder, shell settings, started hidden); about, shell settings, data folder, restart as administrator, quit, `quit_when_idle` |
| `settings.rs` | The shell's settings: theme, accent, text size, navigation pane, keep running in the tray |
| `migrate.rs` | Copying the old apps' data into the module folders at first start |
| `tray.rs` | The notification-area icon: open, a page per module, quit |
| `disk/` | WattCompClean's commands: catalog, preview/clean, shred, wipe, community cleaners |
| `registry/` | WattRegClean's commands: scan/search/clean, backups, tuner, watcher, schedules, start with Windows, native right-click menus (`menu.rs`) |
| `uninstall/` | WattUninstaller's commands: programs, uninstall sessions, leftovers, backups, history, traced installs |
| `error.rs` | The one error shape the page receives |

Every command name carries its module's prefix, so the three command sets
never collide. Each module manages its own `AppState` type; the shell's
commands take the states they need. Rights are reported once, from the
registry module's machine (all three agree on a real PC).

Closing the window hides it when "keep running in the notification area" is
on (the default) or the app started at sign-in; otherwise, and from the
tray's Quit, `quit_when_idle` stops a running disk job (a wipe gives its
space back) and exits; the Exit event stops the Registry Watcher.

## The window (`ui/`)

- `ui/src/main.ts`: the shell. Pages are grouped Disk, Registry and Programs
  in the navigation (Up and Down move between them); each page is made once
  and stays in the document, only the current one shown. The tray's
  `open-page` event opens a page.
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

`%LOCALAPPDATA%\WattClean` (`WATTCLEAN_DATA_DIR` overrides it):
`settings.json` (the shell's), and one folder per module, `disk\`,
`registry\` and `uninstall\`, each holding exactly what the old app kept in
its own folder. The engines find their folder the same way
(`wcc_core::store::data_dir`, `wrc_core::store::data_dir`), so the command
line and scheduled jobs agree with the app.

## Tests

- Rust unit tests in every crate; the engines' tests run as they did.
- `npm test`: pure page logic (`ui/src/**/*.test.ts`).
- `e2e/`: one WebdriverIO session over the fake-machine debug build. The
  shell's specs and the disk cleaner's are in `e2e/specs/`; the registry
  cleaner's and the uninstaller's in `e2e/specs/registry/` and
  `e2e/specs/uninstall/`, each with a `workspace.ts` pointing at its module
  folder and a `support.ts` that maps its old tabs onto the suite's pages.
  `native-menu.ps1` reads and drives the real Windows menus;
  `window-visible.ps1` asks Windows whether the window shows.
