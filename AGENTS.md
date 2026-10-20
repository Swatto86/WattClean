# WattClean — working context

A cleaning suite for Windows, Linux and macOS in Rust + Tauri 2 with
WattTask's look and a notification-area icon. It replaces three apps: a BleachBit-style disk and
privacy cleaner (was WattCompClean), a Revo-style registry cleaner with
tuner, watcher, backups and schedules (was WattRegClean), and a Revo-style
uninstaller with leftover scans, backups and traced installs (was
WattUninstaller). The registry cleaner is Windows-only; on Linux and
macOS the uninstaller asks the package managers. `ARCHITECTURE.md` is the map, `CONTEXT.md` the suite's
decisions; each engine's own structure and decisions are in
`docs/disk-cleaner.md`, `docs/registry.md` and `docs/uninstaller.md`.

## Build, test, verify

- Windows (x64, MSVC), Linux and macOS. Toolchain in `rust-toolchain.toml`.
- Inner loop: `pwsh scripts/fastcheck.ps1` (`-Package <crate>` checks one).
  Run the app on the test machines with
  `npx tauri dev --features fake-system` and `WATTCLEAN_DATA_DIR` set to a
  temp folder. A plain `tauri dev` acts on the real PC (and copies the old
  apps' data on its first start unless `WATTCLEAN_DATA_DIR` is set). On
  Linux and macOS that build fakes only the disk (its pill says "Test
  disk"): Uninstall programs removes real programs. A temp `HOME` (as
  `e2e/linux/smoke.py` sets) keeps per-user apps and leftovers in the
  sandbox, not system packages, `/Applications` or Homebrew.
- Full gate: `node "$HOME/.agents/hooks/gate-runner.mjs" verify` →
  `scripts/verify.ps1` → `scripts/verify.sh`: fmt, clippy, Rust tests,
  release feature guard (all three engines' `fake`), frontend build, types,
  format and tests, version agreement, 400-line cap (a `size-exception:`
  line in the first 30 lines allows more), test-machine debug build, then
  the WebDriver suite (`e2e/`, 102 tests, about 3 minutes). It drives real
  Windows right-click menus on the desktop: leave the mouse and keyboard
  alone while it runs.
- Linux (WSL Ubuntu here: build with `CARGO_TARGET_DIR` on ext4): the same
  `bash scripts/verify.sh`, ending with `e2e/linux/smoke.py` (needs the
  WebKitGTK dev packages, `webkit2gtk-driver`, `xvfb`, Linux node_modules
  and `scripts/setup-tauri-driver.sh`). macOS: the same `bash
  scripts/verify.sh` on the Mac; there is no WebDriver, so open the built
  app (`npx tauri build --debug --bundles app`) and check the window by hand.
- WebDriver setup once per WebView2 update: `pwsh scripts/setup-e2e.ps1`
  (run it from PowerShell: from Git Bash it cannot find Git's bash.exe).
- Read-only checks on this PC (they change nothing):
  `cargo run -p wattclean-cmd -- disk --preview --all-but-warning`;
  `cargo run -p wrc-windows --example dry_run -- [safe|moderate|advanced] [all]`;
  `cargo run -p wu-windows --example dry_run -- "<program>" [mode]`;
  on Linux or macOS `cargo run -p wu-unix --example packages -- [--all]
  [--orphans] [name]`.
- Real-machine checks of the registry engine, as administrator, each
  touching only its own keys or tasks (see `docs/registry.md`):
  `force_check`, `trace_check`, `autostart_check` (WattClean installed and
  closed), `full_check`, and unelevated `schedule_check`
  (`target\debug\examples\*.exe`). Run them after changing that code.
- `cargo test -p wcc-windows -- --ignored` writes and removes a scratch key
  under `HKCU\Software\WattClean-Test`; run it after changing the disk
  engine's `registry.rs`.
- Before a release: `cargo deny --target <triple> check advisories
  licenses` for each shipped target (policy: `deny.toml`) and
  `npm audit --omit=dev`.
- Local install: `AGENT_RELEASE=1 npx tauri build` (NSIS, per machine),
  then `pwsh scripts/install-handoff.ps1`: the installer asks for
  administrator rights once (UAC), then the script starts the app through
  WMI (Claude desktop sessions run inside an MSIX package that redirects
  `%LOCALAPPDATA%`) and checks the version, Start menu shortcut, window,
  single instance and the module folders. Where Windows refuses elevation
  from the session (seen from Claude desktop sessions), open the setup for
  Swatto through `explorer.exe` and run the script with `-AlreadyInstalled`.
  Installed at `C:\Program Files\WattClean`.
- Every bundle carries `wattclean-cmd` as a `bundle.externalBin`:
  `beforeBundleCommand` (`scripts/bundle-cmd.mjs`) builds it for the
  bundle's target (`--target`, lipo for `universal-apple-darwin`) into
  `src-tauri/binaries/wattclean-cmd-<triple>` (gitignored). Until then
  `src-tauri/build.rs` leaves an empty stand-in there, which Tauri also
  copies over `target/<profile>/wattclean-cmd`: use `cargo run -p
  wattclean-cmd`, never that file.
- Releases are published by hand (Actions off) for all three systems:
  bump every manifest, gate here and on swatpc (Windows in the signed-in
  desktop session, Linux in WSL), commit, `AGENT_RELEASE=1 npx tauri build`
  on each (`--target universal-apple-darwin` on the Mac), tag `vX.Y.Z` on
  that commit, `gh release create` with the files in `CONTEXT.md`
  (Releases), download them back and check the sums. First published:
  v0.1.0 (2026-10-18); macOS and Linux from v0.1.3 (2026-10-20).
- `THIRD-PARTY-NOTICES.txt` (the licences of every package built into the
  programs, installed beside them) is generated by
  `node scripts/third-party-notices.mjs`; run it after a dependency change
  (the gate checks it is current).

## Constraints

- `unsafe` lives in the `*-windows` crates, `wcc-unix/src/sys.rs` (and
  `wrc-cmd`'s one mutex check); every other crate forbids it.
- Commands run for cleaning come only from `wcc-core/src/tasks.rs`;
  removals only from `wu-core/src/packages/remove.rs`. Unix leftovers go to
  the Trash, never deleted outright.
- Each engine keeps its safety rules: the disk cleaner's `paths::Guard`
  (checked when planned and again before deleting, keep list honoured,
  links removed as links), the registry cleaner's "only a confident missing
  is offered" and backup-before-removal, the uninstaller's `scan/guard.rs`
  and backup-before-removal. A clean or removal always plans again from the
  machine as it is; the page never names a registry path or file to delete,
  only numbers of findings held in Rust.
- The webview is bundled content only: strict CSP, no remote URLs, no fs,
  shell, dialog or menu permission (dialogs and menus are shown from Rust).
  Text from the machine reaches the DOM only through `textContent`.
  Commands validate their input (`SettingsPatch` and `ShellPatch` refuse
  unknown fields; paths must be full paths; keys go through `parse_key`).
- Command names carry their module's prefix (`disk_`, `reg_`, `un_`) and
  module element ids theirs (`reg-`, `un-`); add new ones the same way.
- The only network access is the user clicking "Download the latest"
  winapp2.ini (Windows' own `curl.exe`, HTTPS only).
- Data: `%LOCALAPPDATA%\WattClean`, `~/.local/share/WattClean`,
  `~/Library/Application Support/WattClean` (`WATTCLEAN_DATA_DIR` overrides), the
  shell's `settings.json` and one folder per module; files are written to a
  temp file then renamed, and loading keeps every field that parses.
- Nothing from BleachBit, Revo or Task Manager TMOG (code, text, icons) is
  copied into this repository.

## Open questions

- Automatic updates are required by the tauri skill; none yet. The
  repository is public, so an updater could read its releases.
- Public repository `Swatto86/WattClean` (since 2026-10-18), Actions off (as
  for every Watt* repo): the local full gate is the only verdict.
- `SRSetRestorePoint` may be throttled by Windows to one point per 24 hours;
  both cleaners report what it returns and carry on.
- macOS builds are not signed by Apple (Gatekeeper asks for Open Anyway
  once); signing and notarization are undecided (`src-tauri/Entitlements.plist` holds the
  Apple-events entitlement a signed build needs).
