//! `wattclean-cmd disk`: the disk cleaner from a console or a scheduled task, with
//! BleachBit's console options. It uses the app's own settings (keep list,
//! custom paths, community cleaners, and the ticked options for --preset).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use wcc_core::catalog::Catalog;
use wcc_core::model::OptionKey;
use wcc_core::paths::Guard;
use wcc_core::report::{Item, Mode, Report, Sink};
use wcc_core::run::{self, RunSettings};
use wcc_core::settings::{Settings, check_full_path};
use wcc_core::shred;
use wcc_core::store;
use wcc_core::system::System;

pub const USAGE: &str = "\
WattClean disk cleaner (wattclean-cmd disk)

  wattclean-cmd disk --list-cleaners
  wattclean-cmd disk --preview  [--preset] [--all-but-warning] [cleaner.option ...]
  wattclean-cmd disk --clean    [--preset] [--all-but-warning] [--overwrite] [cleaner.option ...]
  wattclean-cmd disk --shred PATH ...
  wattclean-cmd disk --wipe-free-space PATH
  wattclean-cmd disk --version

Options are written cleaner.option (chrome.cache); cleaner.* means every
option of that cleaner. --preset adds the options ticked in the app.
--overwrite shreds files instead of just deleting them.
Exit codes: 0 done, 1 done with problems, 2 bad arguments.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    List,
    Run {
        mode: Mode,
        preset: bool,
        all_but_warning: bool,
        overwrite: bool,
        keys: Vec<String>,
    },
    Shred(Vec<PathBuf>),
    Wipe(PathBuf),
    Version,
    Help,
}

/// Reads the arguments (without the program name).
pub fn parse(args: &[String]) -> Result<Command, String> {
    let mut mode = None;
    let mut action: Option<Command> = None;
    let (mut preset, mut all_but_warning, mut overwrite) = (false, false, false);
    let mut rest = Vec::new();
    for arg in args {
        match arg.as_str() {
            "-l" | "--list-cleaners" | "--list" => action = Some(Command::List),
            "-p" | "--preview" => mode = Some(Mode::Preview),
            "-c" | "--clean" => mode = Some(Mode::Clean),
            "-s" | "--shred" => action = Some(Command::Shred(Vec::new())),
            "--wipe-free-space" => action = Some(Command::Wipe(PathBuf::new())),
            "--preset" => preset = true,
            "--all-but-warning" => all_but_warning = true,
            "-o" | "--overwrite" => overwrite = true,
            "-v" | "--version" => action = Some(Command::Version),
            "-h" | "--help" | "/?" => action = Some(Command::Help),
            other if other.starts_with('-') => return Err(format!("unknown option {other}")),
            other => rest.push(other.to_string()),
        }
    }
    match (action, mode) {
        (Some(Command::Shred(_)), None) if !rest.is_empty() => Ok(Command::Shred(
            rest.into_iter().map(PathBuf::from).collect(),
        )),
        (Some(Command::Shred(_)), None) => Err("--shred needs files or folders".into()),
        (Some(Command::Wipe(_)), None) => match rest.as_slice() {
            [one] => Ok(Command::Wipe(PathBuf::from(one))),
            _ => Err("--wipe-free-space needs one folder or drive, such as C:\\".into()),
        },
        (Some(other), None) if rest.is_empty() => Ok(other),
        (None, Some(mode)) => {
            if rest.is_empty() && !preset && !all_but_warning {
                return Err("name options to run, or use --preset or --all-but-warning".into());
            }
            Ok(Command::Run {
                mode,
                preset,
                all_but_warning,
                overwrite,
                keys: rest,
            })
        }
        (None, None) => Ok(Command::Help),
        _ => Err(
            "use one of --list-cleaners, --preview, --clean, --shred or --wipe-free-space".into(),
        ),
    }
}

/// The option keys a run covers, in catalog order.
pub fn selection(
    catalog: &Catalog,
    settings: &Settings,
    preset: bool,
    all_but_warning: bool,
    keys: &[String],
) -> Result<Vec<OptionKey>, String> {
    let mut chosen: Vec<OptionKey> = Vec::new();
    if preset {
        chosen.extend(settings.selected(&catalog.defaults()));
    }
    if all_but_warning {
        chosen.extend(catalog.all_but_warning());
    }
    for text in keys {
        let key = OptionKey::parse(text).ok_or_else(|| format!("{text} is not cleaner.option"))?;
        if key.option == "*" {
            let cleaner = catalog
                .cleaner(&key.cleaner)
                .ok_or_else(|| format!("no cleaner {}", key.cleaner))?;
            chosen.extend(
                cleaner
                    .options
                    .iter()
                    .map(|o| OptionKey::new(&cleaner.id, &o.id)),
            );
        } else if catalog.contains(&key) {
            chosen.push(key);
        } else {
            return Err(format!("no option {text} (see --list-cleaners)"));
        }
    }
    chosen.retain(|k| catalog.contains(k));
    Ok(chosen)
}

/// Prints each item as it is found or done.
struct Printer<'a> {
    out: std::sync::Mutex<&'a mut (dyn Write + Send)>,
    /// Set by Ctrl+C: the run stops between items.
    stop: &'a AtomicBool,
}

impl Sink for Printer<'_> {
    fn item(&self, _key: &OptionKey, item: &Item) {
        if let Ok(mut out) = self.out.lock() {
            let size = if item.size > 0 {
                format!(" ({})", human(item.size))
            } else {
                String::new()
            };
            let note = item
                .note
                .as_deref()
                .map(|n| format!(" [{n}]"))
                .unwrap_or_default();
            let _ = writeln!(out, "{}{size}{note}", item.path);
        }
    }

    fn cancelled(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }
}

pub fn human(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["bytes", "KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} bytes");
    }
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    format!("{size:.1} {}", UNITS[unit])
}

fn summary(report: &Report, out: &mut dyn Write) -> std::io::Result<()> {
    let t = &report.totals;
    let cleaning = report.mode == Mode::Clean;
    writeln!(out)?;
    writeln!(
        out,
        "{}: {}",
        if cleaning {
            "Disk space recovered"
        } else {
            "Disk space to be recovered"
        },
        human(t.bytes)
    )?;
    writeln!(
        out,
        "Files {}: {}",
        if cleaning { "deleted" } else { "to be deleted" },
        t.files
    )?;
    writeln!(
        out,
        "Folders {}: {}",
        if cleaning { "deleted" } else { "to be deleted" },
        t.folders
    )?;
    if t.registry > 0 {
        writeln!(out, "Registry entries: {}", t.registry)?;
    }
    if t.other > 0 {
        writeln!(out, "Special operations: {}", t.other)?;
    }
    for option in &report.options {
        for problem in &option.problems {
            writeln!(out, "Problem: {}: {}", problem.path, problem.message)?;
        }
    }
    if t.problems > 0 {
        writeln!(out, "Problems: {}", t.problems)?;
    }
    if !report.running.is_empty() {
        writeln!(
            out,
            "Running (files in use were skipped): {}",
            report.running.join(", ")
        )?;
    }
    Ok(())
}

/// Runs a parsed command and returns the exit code.
pub fn execute(
    command: &Command,
    system: &dyn System,
    data_dir: &Path,
    stop: &AtomicBool,
    out: &mut (dyn Write + Send),
) -> std::io::Result<i32> {
    let settings = Settings::load(&data_dir.join("settings.json"));
    let catalog = Catalog::load(
        system.platform(),
        Some(&store::cleaners_dir(data_dir)),
        settings.community,
    );
    let keep: Vec<PathBuf> = settings
        .keep
        .iter()
        .map(|k| PathBuf::from(&k.path))
        .collect();
    match command {
        Command::Help => writeln!(out, "{USAGE}").map(|()| 0),
        Command::Version => writeln!(out, "WattClean {}", env!("CARGO_PKG_VERSION")).map(|()| 0),
        Command::List => {
            for cleaner in &catalog.cleaners {
                for option in &cleaner.options {
                    writeln!(out, "{}.{}", cleaner.id, option.id)?;
                }
            }
            Ok(0)
        }
        Command::Run {
            mode,
            preset,
            all_but_warning,
            overwrite,
            keys,
        } => {
            let chosen = match selection(&catalog, &settings, *preset, *all_but_warning, keys) {
                Ok(chosen) if !chosen.is_empty() => chosen,
                Ok(_) => {
                    writeln!(out, "Nothing to run: no option matched.")?;
                    return Ok(2);
                }
                Err(message) => {
                    writeln!(out, "{message}")?;
                    return Ok(2);
                }
            };
            let run_settings = RunSettings {
                keep,
                custom_paths: settings.custom_paths.clone(),
                shred: *overwrite || settings.shred,
                delete_on_reboot: settings.delete_on_reboot,
            };
            let report = {
                let printer = Printer {
                    out: std::sync::Mutex::new(&mut *out),
                    stop,
                };
                run::run(&catalog, &chosen, *mode, system, &run_settings, &printer)
            };
            summary(&report, out)?;
            Ok(if report.totals.problems > 0 { 1 } else { 0 })
        }
        Command::Shred(paths) => shred(paths, system, &keep, stop, out),
        Command::Wipe(folder) => wipe(folder, system, stop, out),
    }
}

fn shred(
    paths: &[PathBuf],
    system: &dyn System,
    keep: &[PathBuf],
    stop: &AtomicBool,
    out: &mut dyn Write,
) -> std::io::Result<i32> {
    for path in paths {
        if let Err(message) = check_full_path(&path.to_string_lossy()) {
            writeln!(out, "{message}")?;
            return Ok(2);
        }
    }
    let guard = Guard::new(&system.protected_folders(), keep);
    let report = shred::shred_paths(
        paths,
        &guard,
        &|path| system.link_count(path),
        &|| stop.load(Ordering::SeqCst),
        &|_, _| {},
    );
    writeln!(
        out,
        "Shredded {} files and {} folders ({}).",
        report.files,
        report.folders,
        human(report.bytes)
    )?;
    if report.cancelled {
        writeln!(out, "Stopped before the end.")?;
    }
    for problem in &report.problems {
        writeln!(out, "Problem: {}: {}", problem.path, problem.message)?;
    }
    Ok(if report.problems.is_empty() && !report.cancelled {
        0
    } else {
        1
    })
}

/// Fills the drive's free space and gives it back. A wipe this program or
/// the app left behind (a killed process, a power cut) is removed first.
fn wipe(
    folder: &Path,
    system: &dyn System,
    stop: &AtomicBool,
    out: &mut dyn Write,
) -> std::io::Result<i32> {
    if !folder.is_dir() {
        writeln!(out, "{} is not a folder or drive.", folder.display())?;
        return Ok(2);
    }
    for (path, error) in shred::remove_wipe_leftovers(folder) {
        writeln!(
            out,
            "An earlier wipe's folder could not be removed: {}: {error}",
            path.display()
        )?;
    }
    writeln!(
        out,
        "Wiping the free space on the drive holding {} (Ctrl+C stops it) ...",
        folder.display()
    )?;
    let stopped = || stop.load(Ordering::SeqCst);
    match shred::wipe_free_space(folder, system.wipe_limit(), &stopped, &|_| {}) {
        Ok(report) if report.cancelled => {
            writeln!(
                out,
                "Stopped after writing {}; it was removed.",
                human(report.written)
            )?;
            Ok(1)
        }
        Ok(report) => {
            writeln!(out, "Wrote and removed {}.", human(report.written))?;
            Ok(0)
        }
        Err(error) => {
            writeln!(out, "The wipe stopped: {error}")?;
            Ok(1)
        }
    }
}

mod console;

pub use console::console;

#[cfg(test)]
mod tests;
