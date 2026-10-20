//! Maintenance commands a cleaner may run: package managers clearing their
//! caches and removing what nothing needs. A definition only names a task
//! by id; the program, its arguments and whether it needs administrator
//! rights come from this table and nowhere else, so no definition file
//! (built-in, custom or imported) can make WattClean run a command of its
//! own. Names a listing step reads from a package manager are checked
//! before they are passed back to it as arguments.

/// What a task's listing step reads from its program's output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parse {
    /// Every non-empty line that does not start with `==>` is one name
    /// (its first word).
    Lines,
    /// `apt-get -s` output: the package of every `Remv` line.
    AptRemoved,
    /// `dpkg-query` lines `<status> <package>`: packages removed with their
    /// settings left behind (`rc`).
    DpkgResidual,
    /// `snap list --all`: revisions marked disabled, kept after an update.
    SnapDisabled,
    /// `xcrun --find simctl`: any path printed means the tool is there.
    Tool,
}

/// How the listed names are used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Use {
    /// Shown in the preview; the command acts on its own.
    Preview,
    /// Appended to the command's arguments, in one run.
    Append,
    /// One run per listed item, its arguments appended.
    Each,
}

/// A read-only command run first, to show (and maybe pass on) what the
/// task acts on. Nothing listed means nothing to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct List {
    pub program: &'static str,
    pub args: &'static [&'static str],
    pub parse: Parse,
    pub then: Use,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Task {
    pub id: &'static str,
    /// Found by the machine in its own program folders, never from `PATH`
    /// alone.
    pub program: &'static str,
    pub args: &'static [&'static str],
    /// Needs administrator (root) rights: the machine asks for them.
    pub admin: bool,
    /// Files or folders (variables and wildcards allowed) whose size is what
    /// the task frees; measured before and after.
    pub measure: &'static [&'static str],
    pub list: Option<List>,
    /// A dry run of the same program whose output says how much the task
    /// would free (`brew cleanup -n`); the preview shows that instead of
    /// the measured size.
    pub estimate: Option<&'static [&'static str]>,
    /// Variables set for the command and its dry run (not its listing),
    /// such as `HOMEBREW_NO_AUTOREMOVE`. Never for a task that needs
    /// administrator rights: the password prompt starts the program with an
    /// environment of its own.
    pub env: &'static [(&'static str, &'static str)],
}

impl Task {
    /// The command as a person would type it, for the preview.
    pub fn command_line(&self) -> String {
        let mut line = self.program.to_string();
        for arg in self.args {
            line.push(' ');
            line.push_str(arg);
        }
        line
    }
}

/// One thing a listing step found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    /// What the preview shows.
    pub label: String,
    /// What is passed to the command for it.
    pub args: Vec<String>,
}

const fn task(id: &'static str, program: &'static str, args: &'static [&'static str]) -> Task {
    Task {
        id,
        program,
        args,
        admin: false,
        measure: &[],
        list: None,
        estimate: None,
        env: &[],
    }
}

const fn admin(mut task: Task) -> Task {
    task.admin = true;
    task
}

const fn measured(mut task: Task, measure: &'static [&'static str]) -> Task {
    task.measure = measure;
    task
}

const fn listed(mut task: Task, list: List) -> Task {
    task.list = Some(list);
    task
}

const fn estimated(mut task: Task, dry_run: &'static [&'static str]) -> Task {
    task.estimate = Some(dry_run);
    task
}

const fn with_env(mut task: Task, env: &'static [(&'static str, &'static str)]) -> Task {
    task.env = env;
    task
}

/// Every task, by id.
pub const TASKS: &[Task] = &[
    // Debian, Ubuntu and their family.
    measured(
        admin(task("apt-clean", "apt-get", &["clean"])),
        &["/var/cache/apt/archives/*.deb", "/var/cache/apt/*.bin"],
    ),
    listed(
        admin(task("apt-autoremove", "apt-get", &["-y", "autoremove"])),
        List {
            program: "apt-get",
            args: &["-s", "autoremove"],
            parse: Parse::AptRemoved,
            then: Use::Preview,
        },
    ),
    listed(
        admin(task("apt-residual", "apt-get", &["-y", "purge"])),
        List {
            program: "dpkg-query",
            args: &[r"--showformat=${db:Status-Abbrev} ${Package}\n", "-W"],
            parse: Parse::DpkgResidual,
            then: Use::Append,
        },
    ),
    // Fedora, RHEL and their family (dnf 4 and 5).
    measured(
        admin(task("dnf-clean", "dnf", &["clean", "all"])),
        &["/var/cache/dnf", "/var/cache/libdnf5"],
    ),
    admin(task("dnf-autoremove", "dnf", &["-y", "autoremove"])),
    // openSUSE.
    measured(
        admin(task(
            "zypper-clean",
            "zypper",
            &["--non-interactive", "clean", "--all"],
        )),
        &["/var/cache/zypp/packages"],
    ),
    // Arch and its family.
    admin(task(
        "pacman-uninstalled",
        "pacman",
        &["-Sc", "--noconfirm"],
    )),
    admin(task("paccache-old", "paccache", &["-rk1"])),
    listed(
        admin(task("pacman-orphans", "pacman", &["-Rns", "--noconfirm"])),
        List {
            program: "pacman",
            args: &["-Qtdq"],
            parse: Parse::Lines,
            then: Use::Append,
        },
    ),
    // Universal formats.
    task(
        "flatpak-unused",
        "flatpak",
        &["uninstall", "--unused", "-y", "--noninteractive"],
    ),
    listed(
        admin(task("snap-disabled", "snap", &["remove"])),
        List {
            program: "snap",
            args: &["list", "--all"],
            parse: Parse::SnapDisabled,
            then: Use::Each,
        },
    ),
    task("nix-gc", "nix-collect-garbage", &[]),
    // The system journal.
    admin(task(
        "journal-vacuum",
        "journalctl",
        &["--vacuum-time=2weeks"],
    )),
    // Homebrew (macOS and Linux). Left to itself, `brew cleanup` also
    // uninstalls the formulae nothing needs: that is brew-autoremove, an
    // option of its own with a warning and the list in its preview.
    with_env(
        estimated(
            measured(
                task("brew-cleanup", "brew", &["cleanup", "--prune=all"]),
                &["%HOMEBREW_CACHE%"],
            ),
            &["cleanup", "--prune=all", "-n"],
        ),
        &[("HOMEBREW_NO_AUTOREMOVE", "1")],
    ),
    listed(
        task("brew-autoremove", "brew", &["autoremove"]),
        List {
            program: "brew",
            args: &["autoremove", "--dry-run"],
            parse: Parse::Lines,
            then: Use::Preview,
        },
    ),
    // MacPorts.
    admin(task("port-clean", "port", &["clean", "--all", "installed"])),
    listed(
        admin(task(
            "port-inactive",
            "port",
            &["-f", "uninstall", "inactive"],
        )),
        List {
            program: "port",
            args: &["-q", "echo", "inactive"],
            parse: Parse::Lines,
            then: Use::Preview,
        },
    ),
    // Developer tools.
    task("pnpm-prune", "pnpm", &["store", "prune"]),
    measured(
        task("go-modcache", "go", &["clean", "-modcache"]),
        &["%GOPATH%/pkg/mod"],
    ),
    task("conda-clean", "conda", &["clean", "--all", "-y"]),
    task("gem-cleanup", "gem", &["cleanup"]),
    task("docker-builder", "docker", &["builder", "prune", "-f"]),
    task("docker-images", "docker", &["image", "prune", "-f"]),
    // macOS.
    // xcrun is on every Mac; simctl only comes with Xcode.
    listed(
        task(
            "simctl-unavailable",
            "xcrun",
            &["simctl", "delete", "unavailable"],
        ),
        List {
            program: "xcrun",
            args: &["--find", "simctl"],
            parse: Parse::Tool,
            then: Use::Preview,
        },
    ),
    task("quicklook-reset", "qlmanage", &["-r", "cache"]),
];

/// The task with this id.
pub fn find(id: &str) -> Option<&'static Task> {
    TASKS.iter().find(|task| task.id == id)
}

mod listing;

pub use listing::{parse, safe_name};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_programs_are_bare_names() {
        let mut ids = std::collections::HashSet::new();
        for task in TASKS {
            assert!(ids.insert(task.id), "{} twice", task.id);
            assert!(
                !task.program.contains(['/', '\\', ' ']),
                "{} names a path",
                task.id
            );
        }
        assert_eq!(
            find("apt-clean").map(Task::command_line).as_deref(),
            Some("apt-get clean")
        );
        assert!(find("rm-rf").is_none());
    }

    /// Variables cannot pass the password prompt; Homebrew's cleanup must
    /// never autoremove.
    #[test]
    fn no_administrator_task_needs_variables() {
        for task in TASKS {
            assert!(!(task.admin && !task.env.is_empty()), "{}", task.id);
        }
        assert_eq!(
            find("brew-cleanup").map(|task| task.env),
            Some(&[("HOMEBREW_NO_AUTOREMOVE", "1")][..])
        );
    }
}
