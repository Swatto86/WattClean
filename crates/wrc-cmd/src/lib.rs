//! `wattclean-cmd registry`: the registry cleaner from a console or a script, as Revo
//! Registry Cleaner's `RRCcmd.exe` is, with the rights of the console it
//! runs in. It cleans as a scheduled Auto Clean does (only what a scan
//! ticks, with a backup), backs keys up, and manages the Registry Watcher's
//! list and logs.

use std::io::Write;
use std::path::{Path, PathBuf};

use wrc_core::backup::back_up_key;
use wrc_core::jobs::clean_ticked;
use wrc_core::keyname::parse_key;
use wrc_core::model::RegPath;
use wrc_core::scan::{Mode, Progress, Request, scan};
use wrc_core::settings::Settings;
use wrc_core::system::System;
use wrc_core::watch::{offline, table, to_csv, to_text};

pub const USAGE: &str = r#"WattClean registry cleaner (wattclean-cmd registry)

  wattclean-cmd registry -scan -safe|-moderate|-advanced [-all]
      List what a clean would remove; nothing changes.
  wattclean-cmd registry -clean -safe|-moderate|-advanced [-all]
      Remove what the scan ticks, backed up first. -all also cleans the
      other accounts on this PC (administrator rights needed).
  wattclean-cmd registry -backup -key <registry key>
  wattclean-cmd registry -backup -full
      Save the whole registry's hives as files (administrator rights).
      WattClean does not put a whole registry back.
  wattclean-cmd registry -backups
      List the backups, newest first, with the number each is restored by.
  wattclean-cmd registry -restore <backup number>
      Put back what a backup holds, as the Backup Center does: anything that
      exists again is left as it is.
  wattclean-cmd registry -watch -start|-stop|-del <registry key>
      Watch a key (or resume it), pause it, or stop watching and delete its
      log. WattClean must be closed: it picks the list up when it starts.
  wattclean-cmd registry -watch -export -csv|-xlsx|-txt <registry key> <folder>
  wattclean-cmd registry -watch -export -all -csv|-xlsx|-txt <folder>

Examples:
  wattclean-cmd registry -clean -moderate
  wattclean-cmd registry -backup -key "HKEY_CURRENT_USER\Software"
  wattclean-cmd registry -watch -export -csv "HKEY_CURRENT_USER\Software" "C:\Temp\logs"

Exit codes: 0 done, 1 the work reported a problem, 2 it could not run.
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Csv,
    Text,
    Workbook,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Help,
    Scan {
        mode: Mode,
        all: bool,
    },
    Clean {
        mode: Mode,
        all: bool,
    },
    BackupKey(RegPath),
    BackupFull,
    Backups,
    Restore(String),
    WatchStart(RegPath),
    WatchStop(RegPath),
    WatchDelete(RegPath),
    /// `key` is `None` for every watched key.
    WatchExport {
        key: Option<RegPath>,
        format: Format,
        folder: PathBuf,
    },
}

/// A flag without its `-` or `/`, in lower case; `None` for a plain word.
fn flag(arg: &str) -> Option<String> {
    arg.strip_prefix('-')
        .or_else(|| arg.strip_prefix('/'))
        .map(str::to_ascii_lowercase)
}

fn key(text: Option<&String>) -> Result<RegPath, String> {
    let text = text.ok_or("A registry key is missing.")?;
    let key = parse_key(text)?;
    if key.path.is_empty() {
        return Err("Name a key inside the hive, not the whole hive.".into());
    }
    Ok(key)
}

fn mode(flags: &[String]) -> Result<Mode, String> {
    match flags.first().map(String::as_str) {
        Some("safe") => Ok(Mode::Safe),
        Some("moderate") => Ok(Mode::Moderate),
        Some("advanced") => Ok(Mode::Advanced),
        _ => Err("Choose -safe, -moderate or -advanced.".into()),
    }
}

/// The command these arguments (without the program's name) ask for.
pub fn parse(args: &[String]) -> Result<Command, String> {
    let flags: Vec<String> = args.iter().filter_map(|arg| flag(arg)).collect();
    let words: Vec<&String> = args.iter().filter(|arg| flag(arg).is_none()).collect();
    let rest = flags.get(1..).unwrap_or_default();
    let all = rest.iter().any(|flag| flag == "all");
    match flags.first().map(String::as_str) {
        None | Some("help" | "?" | "h") => Ok(Command::Help),
        Some("scan") => Ok(Command::Scan {
            mode: mode(rest)?,
            all,
        }),
        Some("clean") => Ok(Command::Clean {
            mode: mode(rest)?,
            all,
        }),
        Some("backup") if rest.first().map(String::as_str) == Some("key") => {
            Ok(Command::BackupKey(key(words.first().copied())?))
        }
        Some("backup") if rest.first().map(String::as_str) == Some("full") => {
            Ok(Command::BackupFull)
        }
        Some("backups") => Ok(Command::Backups),
        Some("restore") => words
            .first()
            .map(|id| Command::Restore((*id).clone()))
            .ok_or_else(|| "Name the backup to restore (its number from -backups).".into()),
        Some("watch") => match rest.first().map(String::as_str) {
            Some("start") => Ok(Command::WatchStart(key(words.first().copied())?)),
            Some("stop") => Ok(Command::WatchStop(key(words.first().copied())?)),
            Some("del") => Ok(Command::WatchDelete(key(words.first().copied())?)),
            Some("export") => {
                let format = if rest.iter().any(|flag| flag == "txt") {
                    Format::Text
                } else if rest.iter().any(|flag| flag == "csv") {
                    Format::Csv
                } else if rest.iter().any(|flag| flag == "xlsx") {
                    Format::Workbook
                } else {
                    return Err("Choose -csv, -xlsx or -txt.".into());
                };
                let (key, folder) = if all {
                    (None, words.first())
                } else {
                    (Some(key(words.first().copied())?), words.get(1))
                };
                let folder = folder.ok_or("The folder to export to is missing.")?;
                Ok(Command::WatchExport {
                    key,
                    format,
                    folder: PathBuf::from(folder.as_str()),
                })
            }
            _ => Err("Choose -start, -stop, -del or -export.".into()),
        },
        Some(other) => Err(format!("-{other} is not a command.")),
    }
}

/// What a command acts on.
pub struct Machine<'a> {
    pub system: &'a dyn System,
    /// The app's data folder.
    pub dir: &'a Path,
    /// WattClean is open, and would overwrite the watch list.
    pub app_running: bool,
}

/// Carry out `command`, writing what happened to `out`; the exit code.
pub fn run(command: &Command, machine: &Machine<'_>, out: &mut dyn Write) -> i32 {
    match execute(command, machine, out) {
        Ok(code) => code,
        Err(error) => {
            let _ = writeln!(out, "{error}");
            2
        }
    }
}

fn execute(command: &Command, machine: &Machine<'_>, out: &mut dyn Write) -> Result<i32, String> {
    let fail = |error: wrc_core::Error| error.to_string();
    let say = |out: &mut dyn Write, text: &str| writeln!(out, "{text}").map_err(|e| e.to_string());
    let system = machine.system;
    let dir = machine.dir;
    match command {
        Command::Help => say(out, USAGE)?,
        Command::Scan { mode, all } => {
            let settings = Settings::load(dir).map_err(fail)?;
            let request = request(*mode, *all);
            let report = scan(system, &request, &settings.options(), &Progress::default());
            let _ = system.release_account_hives();
            let ticked: Vec<_> = report.findings.iter().filter(|f| f.ticked).collect();
            for finding in &ticked {
                let entry = finding.value.as_ref().map_or_else(
                    || finding.key.display(),
                    |value| format!("{} | {value}", finding.key.display()),
                );
                say(
                    out,
                    &format!("{}: {entry} ({})", finding.category.title(), finding.detail),
                )?;
            }
            for note in &report.notes {
                say(out, &format!("Note: {note}"))?;
            }
            say(
                out,
                &format!(
                    "{} of {} entries found would be removed by -clean.",
                    ticked.len(),
                    report.findings.len()
                ),
            )?;
        }
        Command::Clean { mode, all } => {
            let request = request(*mode, *all);
            let (summary, backup) = clean_ticked(
                system,
                dir,
                &request,
                "Command line clean",
                "WattClean: command line registry clean",
            )
            .map_err(fail)?;
            let _ = system.release_account_hives();
            say(out, &summary)?;
            if let Some(backup) = backup {
                say(
                    out,
                    &format!("Backup: {backup} (restore it in the Backup Center)."),
                )?;
            }
            if summary.contains("could not") {
                return Ok(1);
            }
        }
        Command::BackupKey(key) => {
            let backup = back_up_key(
                system,
                &wrc_core::settings::backups_dir(dir),
                key,
                "Command line backup",
            );
            let _ = system.release_account_hives();
            let backup = backup.map_err(fail)?;
            say(
                out,
                &format!("Backed up {} as backup {}.", key.display(), backup.id),
            )?;
        }
        Command::BackupFull => {
            let backup =
                wrc_core::backup::back_up_full(system, &wrc_core::settings::backups_dir(dir), "")
                    .map_err(fail)?;
            say(
                out,
                &format!(
                    "Saved the whole registry ({} hives) as backup {}.",
                    backup.items, backup.id
                ),
            )?;
        }
        Command::Backups => {
            let backups =
                wrc_core::backup::list(&wrc_core::settings::backups_dir(dir)).map_err(fail)?;
            if backups.is_empty() {
                say(out, "No backups yet.")?;
            }
            for backup in backups {
                let state = backup.damaged.map_or_else(
                    || format!("{} entries", backup.items),
                    |problem| format!("damaged: {problem}"),
                );
                say(
                    out,
                    &format!(
                        "{}  {:?}  {}  ({state})",
                        backup.id, backup.kind, backup.label
                    ),
                )?;
            }
        }
        Command::Restore(id) => {
            let outcome =
                wrc_core::backup::restore(system, &wrc_core::settings::backups_dir(dir), id, None)
                    .map_err(fail)?;
            say(out, &format!("{} entries restored.", outcome.restored))?;
            for (what, why) in &outcome.skipped {
                say(out, &format!("Left as it is: {what} ({why})"))?;
            }
        }
        Command::WatchStart(key) | Command::WatchStop(key) | Command::WatchDelete(key) => {
            if machine.app_running {
                return Err("WattClean is open: change the watch list in its Registry Watcher, or quit it first.".into());
            }
            let done = match command {
                Command::WatchStart(_) => {
                    offline::start(system, dir, key).map_err(fail)?;
                    "Watching"
                }
                Command::WatchStop(_) => {
                    offline::stop(dir, key).map_err(fail)?;
                    "Paused watching"
                }
                _ => {
                    offline::delete(dir, key).map_err(fail)?;
                    "Stopped watching, and deleted the log of,"
                }
            };
            say(out, &format!("{done} {}.", key.display()))?;
        }
        Command::WatchExport {
            key,
            format,
            folder,
        } => {
            let keys = match key {
                Some(key) => vec![key.clone()],
                None => offline::saved(dir)
                    .map_err(fail)?
                    .into_iter()
                    .map(|saved| saved.key)
                    .collect(),
            };
            if keys.is_empty() {
                say(out, "No key is watched.")?;
            }
            std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
            for key in keys {
                let events = offline::events(dir, &key).map_err(fail)?;
                let (bytes, extension) = match format {
                    Format::Csv => (to_csv(&events).into_bytes(), "csv"),
                    Format::Text => (to_text(&events).into_bytes(), "txt"),
                    Format::Workbook => (table(&events).xlsx("Watch log"), "xlsx"),
                };
                let file = folder.join(format!(
                    "WattClean watch log {}.{extension}",
                    file_name(&key)
                ));
                std::fs::write(&file, bytes).map_err(|e| format!("{}: {e}", file.display()))?;
                say(
                    out,
                    &format!("Saved {} changes to {}", events.len(), file.display()),
                )?;
            }
        }
    }
    Ok(0)
}

fn request(mode: Mode, all_accounts: bool) -> Request {
    Request {
        mode,
        categories: Vec::new(),
        all_accounts,
    }
}

/// A key as part of a file name: `HKCU Software Acme`.
fn file_name(key: &RegPath) -> String {
    key.display()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " .-_".contains(c) {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

mod console;

pub use console::console;

// The registry exists only on Windows.
#[cfg(test)]
#[cfg(windows)]
mod tests;
