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
    // Homebrew (macOS and Linux).
    measured(
        task("brew-cleanup", "brew", &["cleanup", "--prune=all"]),
        &["%HOMEBREW_CACHE%"],
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
    task(
        "simctl-unavailable",
        "xcrun",
        &["simctl", "delete", "unavailable"],
    ),
    task("quicklook-reset", "qlmanage", &["-r", "cache"]),
];

/// The task with this id.
pub fn find(id: &str) -> Option<&'static Task> {
    TASKS.iter().find(|task| task.id == id)
}

/// A package name safe to hand back to a package manager: no option
/// (`-x`), no spaces or shell characters.
pub fn safe_name(name: &str) -> bool {
    name.len() <= 200
        && name
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '_' | '-' | ':' | '@'))
}

/// What a listing step's output names, unsafe names left out.
pub fn parse(parse: Parse, output: &str) -> Vec<Listed> {
    let one = |name: &str| Listed {
        label: name.to_string(),
        args: vec![name.to_string()],
    };
    let mut out: Vec<Listed> = match parse {
        Parse::Lines => output
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with("==>"))
            .filter_map(|line| line.split_whitespace().next())
            .map(one)
            .collect(),
        Parse::AptRemoved => output
            .lines()
            .filter_map(|line| line.strip_prefix("Remv "))
            .filter_map(|rest| rest.split_whitespace().next())
            .map(one)
            .collect(),
        Parse::DpkgResidual => output
            .lines()
            .filter_map(|line| {
                let mut words = line.split_whitespace();
                (words.next() == Some("rc")).then(|| words.next()).flatten()
            })
            .map(one)
            .collect(),
        Parse::SnapDisabled => output
            .lines()
            .skip(1)
            .filter_map(|line| {
                let words: Vec<&str> = line.split_whitespace().collect();
                let disabled = words
                    .last()
                    .is_some_and(|notes| notes.split(',').any(|note| note == "disabled"));
                match (disabled, words.first(), words.get(2)) {
                    (true, Some(name), Some(revision))
                        if revision.chars().all(|c| c.is_ascii_digit()) =>
                    {
                        Some(Listed {
                            label: format!("{name} (revision {revision})"),
                            args: vec![(*name).to_string(), format!("--revision={revision}")],
                        })
                    }
                    _ => None,
                }
            })
            .collect(),
    };
    out.retain(|listed| listed.args.first().is_some_and(|name| safe_name(name)));
    out.dedup();
    out
}

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

    #[test]
    fn names_that_could_be_options_or_shell_text_are_refused() {
        assert!(safe_name("libfoo1"));
        assert!(safe_name("g++-13"));
        assert!(safe_name("python3.12:amd64"));
        assert!(!safe_name("-rf"));
        assert!(!safe_name("a b"));
        assert!(!safe_name("a;reboot"));
        assert!(!safe_name("$(id)"));
        assert!(!safe_name(""));
    }

    #[test]
    fn listings_are_read_from_each_package_manager() {
        let apt = "NOTE: This is only a simulation!\nRemv libold1 [1.0-1]\nRemv -bad [1]\nInst x\n";
        assert_eq!(
            parse(Parse::AptRemoved, apt),
            vec![Listed {
                label: "libold1".into(),
                args: vec!["libold1".into()],
            }]
        );
        let dpkg = "ii  bash\nrc  oldapp\nrc  other:amd64\n";
        let names: Vec<_> = parse(Parse::DpkgResidual, dpkg)
            .into_iter()
            .map(|l| l.label)
            .collect();
        assert_eq!(names, ["oldapp", "other:amd64"]);
        let brew = "==> Would autoremove 2 unneeded formulae:\nlibyaml\nopenssl@1.1\n";
        let names: Vec<_> = parse(Parse::Lines, brew)
            .into_iter()
            .map(|l| l.label)
            .collect();
        assert_eq!(names, ["libyaml", "openssl@1.1"]);
        let snap = "Name  Version  Rev  Tracking  Publisher  Notes\n\
                    core  16-2.61  16928  latest/stable  canonical✓  core,disabled\n\
                    core  16-2.62  17200  latest/stable  canonical✓  core\n\
                    firefox  128.0  4650  latest/stable  mozilla✓  disabled\n";
        let listed = parse(Parse::SnapDisabled, snap);
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].label, "core (revision 16928)");
        assert_eq!(listed[1].args, ["firefox", "--revision=4650"]);
    }
}
