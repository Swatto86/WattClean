use super::*;
use crate::packages::Output;
use crate::packages::others::{apt_takes, rpm_takes};
use crate::packages::testing::Machine;

fn args(removal: Result<Removal, String>) -> (Vec<String>, bool) {
    match removal.unwrap() {
        Removal::Command { args, admin, .. } => (args, admin),
        Removal::Trash(path) => panic!("trash {path:?}"),
    }
}

#[test]
fn each_manager_removes_with_its_own_command() {
    let machine = Machine::new(&["apt-get", "flatpak", "brew", "snap"]);
    let apt = Package::new(Manager::Apt, "vlc", "VLC");
    assert_eq!(
        args(removal(&machine, &apt, false)),
        (vec!["-y".into(), "remove".into(), "vlc".into()], true)
    );
    assert_eq!(args(removal(&machine, &apt, true)).0[1], "purge");
    let mut flatpak = Package::new(Manager::Flatpak, "org.gimp.GIMP", "GIMP");
    flatpak.installation = Some("user".into());
    assert_eq!(
        args(removal(&machine, &flatpak, true)).0,
        [
            "uninstall",
            "-y",
            "--noninteractive",
            "--user",
            "--delete-data",
            "org.gimp.GIMP"
        ]
    );
    let cask = Package::new(Manager::HomebrewCask, "firefox", "Firefox");
    assert_eq!(
        args(removal(&machine, &cask, true)),
        (
            vec![
                "uninstall".into(),
                "--cask".into(),
                "--zap".into(),
                "firefox".into()
            ],
            false
        )
    );
    let pacman = Package::new(Manager::Pacman, "vlc", "VLC");
    assert!(
        removal(&machine, &pacman, false).is_err(),
        "pacman is not installed"
    );
}

#[test]
fn names_that_could_be_options_are_refused() {
    let machine = Machine::new(&["apt-get"]);
    for id in ["-y", "a b", "x;reboot", "$(id)", ""] {
        let package = Package::new(Manager::Apt, id, id);
        assert!(removal(&machine, &package, false).is_err(), "{id:?}");
    }
}

/// A removal run as administrator never takes its program from a folder
/// the account (or anything it runs) can write; Homebrew's, run as the
/// account, still can.
#[test]
fn an_administrator_removal_never_runs_a_program_the_account_could_have_planted() {
    let mut machine = Machine::new(&["port", "brew", "apt-get", "dpkg"]);
    machine
        .untrusted
        .extend(["port", "brew", "apt-get"].map(String::from));
    let port = Package::new(Manager::MacPorts, "ffmpeg", "ffmpeg");
    assert!(removal(&machine, &port, false).is_err());
    let brew = Package::new(Manager::Homebrew, "wget", "wget");
    assert!(removal(&machine, &brew, false).is_ok());
    let apt = Package::new(Manager::Apt, "vlc", "VLC");
    match removal(&machine, &apt, false) {
        Ok(Removal::Command { program, admin, .. }) => {
            assert_eq!((program, admin), ("/usr/bin/dpkg".into(), true));
        }
        other => panic!("{other:?}"),
    }
}

// Unix paths: on Windows `/Applications` is not a full path.
#[cfg(unix)]
#[test]
fn bundles_and_appimages_go_to_the_trash_whole() {
    let machine = Machine::new(&[]);
    let app = Package::new(Manager::MacApp, "/Applications/Slack.app", "Slack");
    assert_eq!(
        removal(&machine, &app, false),
        Ok(Removal::Trash("/Applications/Slack.app".into()))
    );
    let odd = Package::new(Manager::MacApp, "/Applications/../etc", "x");
    assert!(removal(&machine, &odd, false).is_err());
    let relative = Package::new(Manager::AppImage, "Krita.AppImage", "Krita");
    assert!(removal(&machine, &relative, false).is_err());
}

/// What `apt-get -s remove nautilus` says on Ubuntu 24.04: the desktop
/// metapackages need it, so APT would take them too.
const APT_NAUTILUS: &str = "\
NOTE: This is only a simulation!
      apt-get needs root privileges for real execution.
      Keep also in mind that locking is deactivated,
      so don't depend on the relevance to the real current situation!
Reading package lists...
Building dependency tree...
Reading state information...
The following packages were automatically installed and are no longer required:
  libnautilus-extension4 nautilus-data
Use 'apt autoremove' to remove them.
The following packages will be REMOVED:
  nautilus nautilus-sendto ubuntu-desktop ubuntu-desktop-minimal
0 upgraded, 0 newly installed, 4 to remove and 0 not upgraded.
Remv ubuntu-desktop [1.539.2]
Remv ubuntu-desktop-minimal [1.539.2]
Remv nautilus-sendto [3.8.6-7build2]
Remv nautilus [1:46.2-0ubuntu0.2]
";

/// What `rpm -e --test gtk3` says on Fedora 40 (on stderr, failing).
const RPM_GTK3: &str = "\
error: Failed dependencies:
\tlibgtk-3.so.0()(64bit) is needed by (installed) gnome-shell-46.0-1.fc40.x86_64
\tgtk3(x86-64) = 3.24.41-1.fc40 is needed by (installed) gtk3-devel-3.24.41-1.fc40.x86_64
\tlibgtk-3.so.0()(64bit) is needed by (installed) gtk3-devel-3.24.41-1.fc40.x86_64
\tperl(Gtk3) is needed by (installed) perl-Gtk3-1:0.038-12.fc40.noarch
";

/// APT, dnf and zypper, told yes, also remove every package that needs the
/// one chosen; the removal is refused, naming them, before anything runs
/// as administrator.
#[test]
fn a_removal_that_would_take_other_packages_with_it_is_refused() {
    let apt = Machine::new(&["apt-get"]);
    apt.replies("apt-get -s remove nautilus", true, APT_NAUTILUS, "");
    let nautilus = Package::new(Manager::Apt, "nautilus", "Files");
    let refused = removal(&apt, &nautilus, false).unwrap_err();
    for name in [
        "ubuntu-desktop",
        "ubuntu-desktop-minimal",
        "nautilus-sendto",
    ] {
        assert!(refused.contains(name), "{refused}");
    }
    assert!(
        refused.contains("Files") && refused.contains("apt-get"),
        "{refused}"
    );
    assert_eq!(apt.ran(), ["apt-get -s remove nautilus"]);

    for manager in ["dnf", "zypper"] {
        let machine = Machine::new(&[manager, "rpm"]);
        machine.replies("rpm -e --test --allmatches gtk3", false, "", RPM_GTK3);
        let gtk = Package::new(Manager::Rpm, "gtk3", "gtk3");
        let refused = removal(&machine, &gtk, false).unwrap_err();
        for name in ["gnome-shell", "gtk3-devel", "perl-Gtk3"] {
            assert!(refused.contains(name), "{manager}: {refused}");
        }
        assert!(refused.contains(manager), "{refused}");
        assert_eq!(
            machine.ran(),
            ["rpm -e --test --allmatches gtk3"],
            "{manager}"
        );
    }
}

/// A long list names a handful and counts the rest; purging asks the same.
#[test]
fn a_long_list_of_other_packages_is_named_in_part() {
    let machine = Machine::new(&["apt-get"]);
    let mut said = String::from("Reading package lists...\n");
    for name in ["python3", "a", "b", "c", "d", "e", "f", "g", "h:i386"] {
        said.push_str(&format!("Purg {name} [1.0]\n"));
    }
    machine.replies("apt-get -s purge python3", true, &said, "");
    let python = Package::new(Manager::Apt, "python3", "python3");
    let refused = removal(&machine, &python, true).unwrap_err();
    assert!(refused.contains("8 other packages"), "{refused}");
    assert!(refused.contains("a, b, c, d, e and 3 more"), "{refused}");
    assert!(!refused.contains("h:i386"), "{refused}");
}

/// When the chosen package goes alone, the removal is the usual one.
#[test]
fn a_removal_that_takes_nothing_else_goes_ahead() {
    let apt = Machine::new(&["apt-get"]);
    apt.replies(
        "apt-get -s remove vlc",
        true,
        "Reading package lists...\nRemv vlc [3.0.20-3]\nRemv vlc:i386 [3.0.20-3]\n",
        "",
    );
    let vlc = Package::new(Manager::Apt, "vlc", "VLC");
    assert_eq!(
        args(removal(&apt, &vlc, false)),
        (vec!["-y".into(), "remove".into(), "vlc".into()], true)
    );
    let dnf = Machine::new(&["dnf", "rpm"]);
    dnf.replies("rpm -e --test --allmatches vlc", true, "", "");
    let vlc = Package::new(Manager::Rpm, "vlc", "VLC");
    assert_eq!(
        args(removal(&dnf, &vlc, false)),
        (vec!["-y".into(), "remove".into(), "vlc".into()], true)
    );
}

/// When the package manager's answer cannot be read, nothing is removed:
/// what else would go is unknown.
#[test]
fn a_removal_whose_check_fails_is_refused() {
    let apt = Machine::new(&["apt-get"]);
    apt.replies(
        "apt-get -s remove vlc",
        false,
        "Reading package lists...\n",
        "E: Unable to locate package vlc\n",
    );
    let vlc = Package::new(Manager::Apt, "vlc", "VLC");
    let refused = removal(&apt, &vlc, false).unwrap_err();
    assert!(
        refused.contains("E: Unable to locate package vlc"),
        "{refused}"
    );
    let dnf = Machine::new(&["dnf", "rpm"]);
    dnf.replies(
        "rpm -e --test --allmatches vlc",
        false,
        "",
        "error: can't create transaction lock on /var/lib/rpm/.rpm.lock (Permission denied)\n",
    );
    let vlc = Package::new(Manager::Rpm, "vlc", "VLC");
    assert!(removal(&dnf, &vlc, false).is_err());
    let alone = Machine::new(&["dnf"]);
    assert!(removal(&alone, &vlc, false).is_err(), "rpm is missing");
}

fn said(success: bool, stdout: &str, stderr: &str) -> Output {
    Output {
        success,
        stdout: stdout.into(),
        stderr: stderr.into(),
    }
}

#[test]
fn apt_simulations_are_read_by_their_remv_and_purg_lines() {
    let cases: [(Output, Option<Vec<&str>>); 4] = [
        (
            said(true, APT_NAUTILUS, ""),
            Some(vec![
                "ubuntu-desktop",
                "ubuntu-desktop-minimal",
                "nautilus-sendto",
                "nautilus",
            ]),
        ),
        (
            said(
                true,
                "Reading package lists...\nPurg libgtk-3-0t64:i386 [3.24.41-4ubuntu1.1]\n",
                "",
            ),
            Some(vec!["libgtk-3-0t64:i386"]),
        ),
        (
            said(
                true,
                "Reading package lists...\nPackage 'vlc' is not installed, so not removed\n",
                "",
            ),
            Some(vec![]),
        ),
        (said(false, "", "E: Unable to locate package vlc\n"), None),
    ];
    for (output, expected) in cases {
        let expected = expected.map(|names| names.into_iter().map(String::from).collect());
        assert_eq!(apt_takes(&output), expected, "{output:?}");
    }
}

#[test]
fn rpm_tests_are_read_by_the_packages_that_need_it() {
    let cases: [(Output, Option<Vec<&str>>); 4] = [
        (
            said(false, "", RPM_GTK3),
            Some(vec!["gnome-shell", "gtk3-devel", "gtk3-devel", "perl-Gtk3"]),
        ),
        // openSUSE's release numbers, and a name with a version-like part.
        (
            said(
                false,
                "",
                "error: Failed dependencies:\n\tlibpython3.11.so.1.0()(64bit) is needed by (installed) python311-gobject-3.46.0-1.1.x86_64\n",
            ),
            Some(vec!["python311-gobject"]),
        ),
        (said(true, "", ""), Some(vec![])),
        (
            said(
                false,
                "",
                "error: can't create transaction lock on /var/lib/rpm/.rpm.lock (Permission denied)\n",
            ),
            None,
        ),
    ];
    for (output, expected) in cases {
        let expected = expected.map(|names| names.into_iter().map(String::from).collect());
        assert_eq!(rpm_takes(&output), expected, "{output:?}");
    }
}
