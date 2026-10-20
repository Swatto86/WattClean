//! Removals on macOS: open apps, Homebrew's services and password box,
//! and background jobs stopped before their plists go.

use super::*;
use crate::packages::testing::Machine;

/// An app moved to the Trash while it is open keeps running from there and
/// writes its folders again after they were offered as leftovers.
#[test]
fn an_open_app_is_not_removed() {
    let mut machine = Machine::macos();
    machine.prints("brew", "");
    machine.running = vec![
        "/Applications/Tea.app/Contents/MacOS/tea".into(),
        "/Applications/Foo.app2/Contents/MacOS/foo".into(),
    ];
    let tea = Package::new(Manager::MacApp, "/Applications/Tea.app", "Tea");
    assert_eq!(
        removal(&machine, &tea, false),
        Err("Tea is open: quit it, then uninstall.".into())
    );
    let foo = Package::new(Manager::MacApp, "/Applications/Foo.app", "Foo");
    assert_eq!(
        removal(&machine, &foo, false),
        Ok(Removal::Trash("/Applications/Foo.app".into())),
        "Foo.app2 is another app"
    );
    // A cask's app, its letters in another case as macOS's disks allow.
    let mut cask = Package::new(Manager::HomebrewCask, "tea", "Tea");
    cask.app_path = Some("/applications/tea.app".into());
    assert_eq!(
        removal(&machine, &cask, false),
        Err("Tea is open: quit it, then uninstall.".into())
    );
}

fn before(machine: &Machine, id: &str) -> Vec<Step> {
    match removal(machine, &Package::new(Manager::Homebrew, id, id), false) {
        Ok(Removal::Command { before, .. }) => before,
        other => panic!("{other:?}"),
    }
}

/// `brew uninstall` leaves a formula's service running from a deleted
/// folder, started again at every login or boot.
#[test]
fn a_formulas_background_service_is_stopped_before_it_goes() {
    let machine = Machine::macos();
    machine.prints("brew", "");
    machine.prints("launchctl", "");
    let agents = machine.home().unwrap().join("Library/LaunchAgents");
    let daemons = machine.at("/Library/LaunchDaemons");
    std::fs::create_dir_all(&agents).unwrap();
    std::fs::create_dir_all(&daemons).unwrap();
    std::fs::write(agents.join("homebrew.mxcl.redis.plist"), b"").unwrap();
    for id in ["tailscale", "stopped", "both"] {
        std::fs::write(daemons.join(format!("homebrew.mxcl.{id}.plist")), b"").unwrap();
    }
    std::fs::write(agents.join("homebrew.mxcl.both.plist"), b"").unwrap();
    machine.replies(
        "launchctl print system/homebrew.mxcl.stopped",
        false,
        "",
        "Could not find service \"homebrew.mxcl.stopped\" in domain for system",
    );

    let step = |program: &str, args: &[&str], admin| Step {
        program: program.into(),
        args: args.iter().map(ToString::to_string).collect(),
        admin,
    };
    assert_eq!(
        before(&machine, "redis"),
        [step("/usr/bin/brew", &["services", "stop", "redis"], false)]
    );
    let root = step(
        "/usr/bin/launchctl",
        &["bootout", "system/homebrew.mxcl.tailscale"],
        true,
    );
    assert_eq!(before(&machine, "tailscale"), [root]);
    assert_eq!(before(&machine, "stopped"), [], "launchd has not loaded it");
    // Homebrew will not stop the account's own while root's plist is
    // there: that one is left to the leftovers.
    assert_eq!(
        before(&machine, "both"),
        [step(
            "/usr/bin/launchctl",
            &["bootout", "system/homebrew.mxcl.both"],
            true
        )]
    );
    assert_eq!(before(&machine, "wget"), []);
    let ran = machine.ran();
    assert!(
        ran.iter()
            .all(|command| command.starts_with("launchctl print ")),
        "only questions are asked while planning: {ran:?}"
    );

    let linux = Machine::new(&["brew", "launchctl"]);
    assert_eq!(before(&linux, "tailscale"), [], "no launchd on Linux");
}

/// Homebrew runs some casks' steps through sudo, which from an app has no
/// terminal to ask for the password in.
#[test]
fn homebrew_asks_for_a_password_through_wattclean_on_macos() {
    let mac = Machine::macos();
    mac.prints("brew", "");
    let linux = Machine::new(&["brew"]);
    for (machine, expected) in [(&mac, true), (&linux, false)] {
        for manager in [Manager::Homebrew, Manager::HomebrewCask] {
            let package = Package::new(manager, "battle-net", "Blizzard Battle.net");
            match removal(machine, &package, false) {
                Ok(Removal::Command { askpass, admin, .. }) => {
                    assert_eq!((askpass, admin), (expected, false), "{manager:?}");
                }
                other => panic!("{other:?}"),
            }
        }
    }
    let ports = Machine::macos();
    ports.prints("port", "");
    let port = Package::new(Manager::MacPorts, "ffmpeg", "ffmpeg");
    assert!(matches!(
        removal(&ports, &port, false),
        Ok(Removal::Command { askpass: false, .. })
    ));
}

fn plist(label: Option<&str>) -> Leftover {
    Leftover {
        path: "/Users/t/Library/LaunchAgents/Tea.plist".into(),
        size: Some(1),
        folder: false,
        job: label.map(String::from),
    }
}

/// A plist moved to the Trash leaves its job running until logout.
#[test]
fn a_loaded_background_job_is_stopped_before_its_plist_goes() {
    let machine = Machine::macos();
    machine.prints("launchctl", "");
    machine.replies(
        "launchctl print gui/501/Idle",
        false,
        "",
        "Could not find service \"Idle\" in domain for user gui: 501",
    );
    assert_eq!(
        stop_job(&machine, &plist(Some("Tea"))),
        Ok(Some(Step {
            program: "/usr/bin/launchctl".into(),
            args: vec![
                "bootout".into(),
                "gui/501".into(),
                "/Users/t/Library/LaunchAgents/Tea.plist".into()
            ],
            admin: false,
        }))
    );
    assert_eq!(
        stop_job(&machine, &plist(Some("Idle"))),
        Ok(None),
        "not loaded"
    );
    assert_eq!(stop_job(&machine, &plist(None)), Ok(None), "not a job");
    for label in [
        "co.swatto.wattclean",
        "Co.Swatto.WattClean.agent",
        "-x",
        "a b",
    ] {
        assert!(stop_job(&machine, &plist(Some(label))).is_err(), "{label}");
    }
    assert!(
        machine
            .ran()
            .iter()
            .all(|c| c.starts_with("launchctl print ")),
        "{:?}",
        machine.ran()
    );
}
