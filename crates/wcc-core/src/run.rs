//! A preview or a clean of the selected options: plan each option, then list
//! (preview) or carry out (clean) its operations, reporting as it goes.
//! A clean plans again from the disk as it is then, as BleachBit does, so
//! nothing is deleted on the strength of an old preview.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Instant, SystemTime};

use crate::catalog::Catalog;
use crate::exec::{Executor, Outcome};
use crate::model::{Action, Cleaner, CleanerOption, OptionKey};
use crate::ops::{CustomPath, Op, Planner};
use crate::paths::Guard;
use crate::report::{Counts, Mode, OptionReport, Progress, Report, Sink};
use crate::system::{Platform, System};

/// Settings that shape a run.
#[derive(Debug, Clone, Default)]
pub struct RunSettings {
    /// The keep list: files and folders never removed.
    pub keep: Vec<PathBuf>,
    pub custom_paths: Vec<CustomPath>,
    /// Overwrite file contents before deleting.
    pub shred: bool,
    /// Files in use are deleted at the next restart (needs administrator).
    pub delete_on_reboot: bool,
}

pub fn run(
    catalog: &Catalog,
    selection: &[OptionKey],
    mode: Mode,
    system: &dyn System,
    settings: &RunSettings,
    sink: &dyn Sink,
) -> Report {
    let started = Instant::now();
    let guard = Guard::new(&system.protected_folders(), &settings.keep);
    let cancelled = || sink.cancelled();
    let planner = Planner {
        system,
        guard: &guard,
        custom_paths: &settings.custom_paths,
        now: SystemTime::now(),
        cancelled: &cancelled,
    };
    let executor = Executor {
        system,
        guard: &guard,
        shred: settings.shred,
        delete_on_reboot: settings.delete_on_reboot && system.is_elevated(),
        cancelled: &cancelled,
    };

    // Options in catalog order, each once, unknown keys ignored.
    let wanted: HashSet<&OptionKey> = selection.iter().collect();
    let mut options: Vec<_> = catalog
        .cleaners
        .iter()
        .flat_map(|c| c.options.iter().map(move |o| (c, o)))
        .filter(|(c, o)| wanted.contains(&OptionKey::new(&c.id, &o.id)))
        .collect();
    // Wiping free space goes last, so it covers what the clean freed.
    options.sort_by_key(|(_, o)| o.actions.contains(&Action::FreeDiskSpace));

    // Programs that are running and installed: Chromium shares chrome.exe
    // with Chrome, so a running name alone says too little.
    let running_now = system.running();
    let mut running_ids: Vec<&str> = Vec::new();
    let mut running: Vec<String> = Vec::new();
    for (cleaner, _) in &options {
        if !running_ids.contains(&cleaner.id.as_str())
            && cleaner.running.iter().any(|exe| running_now.contains(exe))
            && crate::catalog::is_present(cleaner, system)
        {
            running_ids.push(&cleaner.id);
            running.push(cleaner.name.clone());
        }
    }
    // Windows refuses to delete a file a program holds open, so a running
    // program loses only what it does not hold. macOS and Linux delete it
    // from under the program, which goes on using it (a database's journal
    // among them): there its options are left until it is closed.
    let platform = system.platform();
    let leave_running = platform != Platform::Windows;

    let mut step = Step {
        mode,
        planner: &planner,
        executor: &executor,
        sink,
        seen: HashSet::new(),
        changed_shell: false,
        left_alone: HashSet::new(),
    };
    let mut reports = Vec::new();
    let mut totals = Counts::default();
    for (index, (cleaner, option)) in options.iter().enumerate() {
        if sink.cancelled() {
            break;
        }
        let place = (index, options.len());
        let report = if leave_running && running_ids.contains(&cleaner.id.as_str()) {
            step.left_alone(cleaner, option, platform)
        } else {
            step.option(cleaner, option, place, &totals)
        };
        totals.absorb(&report.counts);
        reports.push(report);
    }
    let changed_shell = step.changed_shell;
    if mode == Mode::Clean && (changed_shell || totals.files > 0) {
        system.shell_changed();
    }
    let cancelled = sink.cancelled();
    sink.progress(&Progress {
        mode,
        label: String::new(),
        done: reports.len(),
        total: options.len(),
        items: totals.items(),
        bytes: totals.bytes,
    });
    Report {
        mode,
        options: reports,
        totals,
        cancelled,
        elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        running,
    }
}

/// One option at a time: plan each action, then list or carry out its
/// operations.
struct Step<'a> {
    mode: Mode,
    planner: &'a Planner<'a>,
    executor: &'a Executor<'a>,
    sink: &'a dyn Sink,
    /// Files already planned by an earlier option.
    seen: HashSet<String>,
    /// A registry entry went, so Explorer is told afterwards.
    changed_shell: bool,
    /// Running programs already said to be left alone, by cleaner id.
    left_alone: HashSet<String>,
}

impl Step<'_> {
    /// An option of a program that is running, on a system that would
    /// delete its open files: nothing is planned, and the program's first
    /// option says why, once.
    fn left_alone(
        &mut self,
        cleaner: &Cleaner,
        option: &CleanerOption,
        platform: Platform,
    ) -> OptionReport {
        let key = OptionKey::new(&cleaner.id, &option.id);
        let mut report = OptionReport::new(&key, &cleaner.name, &option.name);
        if self.left_alone.insert(cleaner.id.clone()) {
            let verb = if platform == Platform::Macos {
                "Quit"
            } else {
                "Close"
            };
            report.push_problem(
                cleaner.name.clone(),
                format!("it is running, so it was left alone. {verb} it, then clean again."),
                false,
            );
        }
        report
    }

    fn option(
        &mut self,
        cleaner: &Cleaner,
        option: &CleanerOption,
        (index, total): (usize, usize),
        before: &Counts,
    ) -> OptionReport {
        let key = OptionKey::new(&cleaner.id, &option.id);
        let label = format!("{} › {}", cleaner.name, option.name);
        let mut report = OptionReport::new(&key, &cleaner.name, &option.name);
        let progress = |report: &OptionReport| Progress {
            mode: self.mode,
            label: label.clone(),
            done: index,
            total,
            items: before.items() + report.counts.items(),
            bytes: before.bytes + report.counts.bytes,
        };
        self.sink.progress(&progress(&report));
        for action in &option.actions {
            if self.sink.cancelled() {
                break;
            }
            let (ops, problems) = self.planner.plan(action, &mut self.seen);
            for problem in problems {
                // Every action under a folder macOS refuses meets the same
                // refusal: it is said once per option.
                let path = problem.path.to_string_lossy().into_owned();
                let said = report
                    .problems
                    .iter()
                    .any(|p| p.path == path && p.message == problem.message);
                if !said {
                    report.push_problem(path, problem.message, false);
                }
            }
            for op in ops {
                if self.sink.cancelled() {
                    break;
                }
                let outcome = match self.mode {
                    Mode::Preview => Outcome::Done(op.item()),
                    Mode::Clean => self.executor.execute(&op),
                };
                match outcome {
                    Outcome::Done(item) => {
                        self.changed_shell |=
                            matches!(op, Op::RegistryKey { .. } | Op::RegistryValue { .. });
                        self.sink.item(&key, &item);
                        report.push_item(item);
                        if report.counts.items().is_multiple_of(64) {
                            self.sink.progress(&progress(&report));
                        }
                    }
                    Outcome::Skipped => {}
                    Outcome::Failed { message, in_use } => {
                        report.push_problem(op.item().path, message, in_use);
                    }
                }
            }
        }
        report
    }
}

#[cfg(test)]
#[path = "run_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "run_command_tests.rs"]
mod command_tests;

#[cfg(test)]
#[path = "run_running_tests.rs"]
mod running_tests;
