//! Maintenance commands in a run (`tasks::TASKS`): planning one (is its
//! program installed, what does its listing name, how much would it free),
//! the line a preview shows, and running it when the run cleans.

use std::io;
use std::path::{Path, PathBuf};

use crate::exec::Executor;
use crate::ops::{Op, Planner};
use crate::search::Problem;
use crate::system::System;
use crate::tasks::{self, Listed, Task, Use};
use crate::vars;

/// What a command's preview line adds: whether it asks for administrator
/// rights, and the first few things it acts on.
pub(crate) fn note(task: &Task, listed: &[Listed]) -> Option<String> {
    const SHOWN: usize = 8;
    let mut parts = Vec::new();
    if task.admin {
        parts.push("as administrator".to_string());
    }
    if !listed.is_empty() {
        let mut names: Vec<&str> = listed
            .iter()
            .take(SHOWN)
            .map(|l| l.label.as_str())
            .collect();
        let more = listed.len().saturating_sub(SHOWN);
        let more = format!("and {more} more");
        if listed.len() > SHOWN {
            names.push(&more);
        }
        parts.push(names.join(", "));
    }
    (!parts.is_empty()).then(|| parts.join(": "))
}

/// Bytes in the files at or below these patterns (variables and
/// wildcards allowed); links are counted, never followed.
pub fn measure(system: &dyn System, patterns: &[&str]) -> u64 {
    fn tree(path: &Path) -> u64 {
        let Ok(meta) = std::fs::symlink_metadata(path) else {
            return 0;
        };
        if !meta.is_dir() {
            return meta.len();
        }
        std::fs::read_dir(path)
            .map(|entries| entries.flatten().map(|e| tree(&e.path())).sum())
            .unwrap_or(0)
    }
    let lookup = |name: &str| system.var(name);
    patterns
        .iter()
        .flat_map(|pattern| vars::expand(pattern, &lookup))
        .flat_map(|pattern| crate::wildcard::glob(&pattern))
        .map(|path| tree(&path))
        .sum()
}

impl Planner<'_> {
    /// A task, when its program is installed and (for a task with a
    /// listing step) the listing names something to act on.
    pub(crate) fn command(&self, task: &'static Task) -> Result<Option<Op>, Problem> {
        let Some(program) = self.program(task, task.program) else {
            return Ok(None);
        };
        let mut listed = Vec::new();
        if let Some(list) = task.list {
            let Some(lister) = self.program(task, list.program) else {
                return Ok(None);
            };
            let args: Vec<String> = list.args.iter().map(ToString::to_string).collect();
            // A listing that finds nothing may exit with an error (pacman
            // -Qtdq does): only its output counts.
            let output = self
                .system
                .run_program(&lister, &args, false, &[])
                .map_err(|error| Problem {
                    path: PathBuf::from(task.command_line()),
                    message: crate::search::describe(&error),
                })?;
            listed = tasks::parse(list.parse, list.then, &output.stdout);
            if listed.is_empty() {
                return Ok(None);
            }
        }
        // The dry run only refines the preview: when it cannot run or
        // fails, the measured size stands.
        let estimate = task.estimate.and_then(|dry_run| {
            let args: Vec<String> = dry_run.iter().map(ToString::to_string).collect();
            self.system
                .run_program(&program, &args, false, task.env)
                .ok()
                .filter(|output| output.success)
                .and_then(|output| freed_estimate(&output.stdout))
        });
        Ok(Some(Op::Command {
            task,
            program,
            listed,
            size: measure(self.system, task.measure),
            estimate,
        }))
    }

    /// Where a task's program (or its listing's) is. For a task run as
    /// administrator, only where only an administrator can change it: its
    /// listing too, since what that lists is what the command acts on.
    fn program(&self, task: &Task, name: &str) -> Option<PathBuf> {
        if task.admin {
            self.system.find_admin_program(name)
        } else {
            self.system.find_program(name)
        }
    }
}

/// Bytes a dry run says it would free, from Homebrew's closing line
/// `==> This operation would free approximately 390.1MB of disk space.`
/// (decimal units, as Homebrew prints them). A dry run without that line
/// has nothing to free: 0, not the size of the whole cache, which brew
/// never removes whole. `None` when the figure cannot be read.
pub(crate) fn freed_estimate(stdout: &str) -> Option<u64> {
    let Some(line) = stdout
        .lines()
        .rev()
        .find(|line| line.contains("would free approximately "))
    else {
        return Some(0);
    };
    let amount = line
        .split("would free approximately ")
        .nth(1)?
        .split_whitespace()
        .next()?;
    let split = amount
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(amount.len());
    let (number, unit) = amount.split_at(split);
    let number: f64 = number.parse().ok()?;
    let factor = match unit {
        "B" => 1.0,
        "KB" => 1e3,
        "MB" => 1e6,
        "GB" => 1e9,
        "TB" => 1e12,
        _ => return None,
    };
    format!("{:.0}", number * factor).parse().ok()
}

impl Executor<'_> {
    /// Runs a task's command: once, once with every listed name, or once
    /// per listed item. A run that fails stops the task with its message.
    pub(crate) fn command(
        &self,
        task: &Task,
        program: &Path,
        listed: &[Listed],
    ) -> io::Result<bool> {
        let base: Vec<String> = task.args.iter().map(ToString::to_string).collect();
        let runs: Vec<Vec<String>> = match task.list.map(|list| list.then) {
            Some(Use::Append) => vec![
                base.iter()
                    .cloned()
                    .chain(listed.iter().flat_map(|l| l.args.iter().cloned()))
                    .collect(),
            ],
            Some(Use::Each) => listed
                .iter()
                .map(|l| base.iter().cloned().chain(l.args.iter().cloned()).collect())
                .collect(),
            Some(Use::Preview) | None => vec![base],
        };
        for args in runs {
            if (self.cancelled)() {
                return Ok(false);
            }
            let output = self
                .system
                .run_program(program, &args, task.admin, task.env)?;
            if !output.success {
                let said = output
                    .stderr
                    .lines()
                    .chain(output.stdout.lines())
                    .map(str::trim)
                    .rfind(|line| !line.is_empty())
                    .unwrap_or("it reported a failure");
                return Err(io::Error::other(format!("{}: {said}", task.program)));
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::freed_estimate;
    use crate::fake::FakeSystem;
    use crate::ops::{Op, Planner};
    use crate::paths::Guard;

    /// What brew's dry run says it frees, planned on the test machine.
    fn brew_estimate(dry_run_prints: &str) -> Option<u64> {
        let dir = tempfile::tempdir().unwrap();
        let system = FakeSystem::open(dir.path()).unwrap();
        system.add_program("brew", dry_run_prints).unwrap();
        let guard = Guard::new(&[], &[]);
        let planner = Planner {
            system: &system,
            guard: &guard,
            custom_paths: &[],
            now: std::time::SystemTime::now(),
            cancelled: &|| false,
        };
        let task = crate::tasks::find("brew-cleanup").unwrap();
        match planner.command(task) {
            Ok(Some(Op::Command { estimate, .. })) => estimate,
            other => panic!("{other:?}"),
        }
    }

    /// A dry run that succeeds and names nothing to free means nothing is
    /// freed, not "everything in the cache" (which brew never removes whole).
    #[test]
    fn a_dry_run_that_frees_nothing_previews_nothing() {
        assert_eq!(brew_estimate(""), Some(0));
        assert_eq!(
            brew_estimate("==> This operation would free approximately 2.5MB of disk space.\n"),
            Some(2_500_000)
        );
    }

    #[test]
    fn homebrews_own_estimate_is_read_in_decimal_units() {
        let out = "Would remove: /Users/t/Library/Caches/Homebrew/downloads/x--claude.zip (390.1MB)\n\
                   ==> This operation would free approximately 390.1MB of disk space.\n";
        assert_eq!(freed_estimate(out), Some(390_100_000));
        assert_eq!(
            freed_estimate("==> This operation would free approximately 14.2KB of disk space."),
            Some(14_200)
        );
        assert_eq!(
            freed_estimate("==> This operation would free approximately 55B of disk space."),
            Some(55)
        );
        assert_eq!(freed_estimate(""), Some(0));
        assert_eq!(
            freed_estimate("==> This operation would free approximately 3PB of disk space."),
            None
        );
        assert_eq!(
            freed_estimate("would free approximately lots of disk space"),
            None
        );
    }
}
