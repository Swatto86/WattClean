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
        let Some(program) = self.system.find_program(task.program) else {
            return Ok(None);
        };
        let mut listed = Vec::new();
        if let Some(list) = task.list {
            let Some(lister) = self.system.find_program(list.program) else {
                return Ok(None);
            };
            let args: Vec<String> = list.args.iter().map(ToString::to_string).collect();
            // A listing that finds nothing may exit with an error (pacman
            // -Qtdq does): only its output counts.
            let output = self
                .system
                .run_program(&lister, &args, false)
                .map_err(|error| Problem {
                    path: PathBuf::from(task.command_line()),
                    message: crate::search::describe(&error),
                })?;
            listed = tasks::parse(list.parse, &output.stdout);
            if listed.is_empty() {
                return Ok(None);
            }
        }
        // The dry run only refines the preview: when it fails, the measured
        // size stands.
        let estimate = task.estimate.and_then(|dry_run| {
            let args: Vec<String> = dry_run.iter().map(ToString::to_string).collect();
            self.system
                .run_program(&program, &args, false)
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
}

/// Bytes a dry run says it would free, from Homebrew's closing line
/// `==> This operation would free approximately 390.1MB of disk space.`
/// (decimal units, as Homebrew prints them).
pub(crate) fn freed_estimate(stdout: &str) -> Option<u64> {
    let line = stdout
        .lines()
        .rev()
        .find(|line| line.contains("would free approximately "))?;
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
            let output = self.system.run_program(program, &args, task.admin)?;
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
        assert_eq!(freed_estimate("Nothing to clean"), None);
        assert_eq!(
            freed_estimate("would free approximately lots of disk space"),
            None
        );
    }
}
