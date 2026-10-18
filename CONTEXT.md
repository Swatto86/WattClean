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
  build (its own data folder) never migrates.
- **One right-click policy**: the registry cleaner's native menus (Windows'
  edit menu on text, a list's own menu on a list) serve the whole window;
  the uninstaller's separate edit menu was dropped as a duplicate. The
  modules' text stays selectable for copying paths, the shell's chrome does
  not.
- **One command line**, `wattclean-cmd disk …` (BleachBit's options) and
  `wattclean-cmd registry …` (Revo's `RRCcmd` verbs), so scripts keep their
  exit codes; the old console programs became libraries.
- **Rights are reported once**, from the registry module's machine; on a
  real PC all three agree. On the test build that means "Administrator",
  which the registry and uninstaller suites expect; the disk engine still
  asks its own machine for its own administrator-only behaviour.

## Repository

- Public `Swatto86/WattClean` since 2026-10-18, starting from one commit
  (the earlier private history was not published), so installers can be
  downloaded by anyone; listed on swatto.co.uk. Actions off (as for every
  Watt* repo): the local full gate is the verdict; releases are built and
  published by hand, each with `THIRD-PARTY-NOTICES.txt` as the open-source
  packages' licences ask.
- No network access except the winapp2.ini download the person clicks;
  no automatic updates yet (open question in `AGENTS.md`).
