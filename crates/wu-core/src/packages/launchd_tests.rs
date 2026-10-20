use std::path::PathBuf;

use super::*;
use crate::packages::testing::{Machine, launchd_job, offered};
use crate::packages::{Manager, Package, PackageSystem, find_leftovers};

/// Puts a plist in the account's LaunchAgents folder, saying what is given.
fn agent(machine: &mut Machine, file: &str, job: Option<(&str, &str, &[&str])>) -> PathBuf {
    let folder = machine.home().unwrap().join("Library/LaunchAgents");
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join(file);
    std::fs::write(&path, b"plist").unwrap();
    if let Some((label, program, associated)) = job {
        let job = launchd_job(label, program, associated, &[]);
        machine.jobs.insert(path.clone(), job);
    }
    path
}

/// The leftovers found, relative to the home folder, with their jobs.
fn jobs(
    machine: &Machine,
    removed: &Package,
    installed: &[Package],
) -> Vec<(String, Option<String>)> {
    let home = machine.home().unwrap();
    find_leftovers(machine, removed, installed)
        .into_iter()
        .map(|l| {
            let path = l.path.strip_prefix(&home).unwrap().to_string_lossy();
            (path.replace('\\', "/"), l.job.map(|job| job.label))
        })
        .collect()
}

fn found(path: &str, label: Option<&str>) -> (String, Option<String>) {
    (
        format!("Library/LaunchAgents/{path}"),
        label.map(String::from),
    )
}

#[test]
fn a_plist_says_its_label_program_and_apps() {
    let orbstack = LaunchdJob::from_json(
        r#"{"Label":"dev.orbstack.OrbStack.privhelper","Program":"/Library/PrivilegedHelperTools/dev.orbstack.OrbStack.privhelper","AssociatedBundleIdentifiers":["dev.kdrag0n.MacVirt"],"MachServices":{"x":true}}"#,
    )
    .unwrap();
    assert_eq!(orbstack.label, "dev.orbstack.OrbStack.privhelper");
    assert_eq!(
        orbstack.program.as_deref(),
        Some("/Library/PrivilegedHelperTools/dev.orbstack.OrbStack.privhelper")
    );
    assert_eq!(orbstack.associated, ["dev.kdrag0n.MacVirt"]);
    // Every text it holds, keys too, however deep.
    for text in [
        "dev.orbstack.OrbStack.privhelper",
        "/Library/PrivilegedHelperTools/dev.orbstack.OrbStack.privhelper",
        "MachServices",
        "x",
    ] {
        assert!(
            orbstack.mentions.iter().any(|m| m == text),
            "{text} in {:?}",
            orbstack.mentions
        );
    }
    let tea = LaunchdJob::from_json(
        r#"{"Label":"Tea","ProgramArguments":["/Applications/Tea.app/Contents/MacOS/tea","--hidden"],"AssociatedBundleIdentifiers":"xyz.tea.Tea"}"#,
    )
    .unwrap();
    assert_eq!(
        tea.program.as_deref(),
        Some("/Applications/Tea.app/Contents/MacOS/tea")
    );
    assert_eq!(tea.associated, ["xyz.tea.Tea"]);
    assert_eq!(LaunchdJob::from_json(r#"{"Program":"/bin/x"}"#), None);
    assert_eq!(LaunchdJob::from_json("{}"), None);
    assert_eq!(LaunchdJob::from_json("not json"), None);
}

/// OrbStack's helper names its app only in AssociatedBundleIdentifiers and
/// another agent starts a program inside the app's bundle: neither has the
/// app's name. Google's updater names Google's updater, not this app.
#[test]
fn background_jobs_are_found_by_what_they_start_and_the_app_they_name() {
    let mut machine = Machine::macos();
    let helper = (
        "dev.orbstack.helper",
        "/Users/t/.orbstack/bin/helper",
        &["dev.kdrag0n.MacVirt"][..],
    );
    agent(&mut machine, "dev.orbstack.helper.plist", Some(helper));
    let starter = (
        "com.example.starter",
        "/Applications/OrbStack.app/Contents/MacOS/OrbStack",
        &[][..],
    );
    agent(&mut machine, "starter.plist", Some(starter));
    let updater = (
        "com.google.GoogleUpdater.wake",
        "/Users/t/Library/Application Support/Google/GoogleUpdater/Current/GoogleUpdater.app/Contents/MacOS/GoogleUpdater",
        &["com.google.GoogleUpdater"][..],
    );
    agent(
        &mut machine,
        "com.google.GoogleUpdater.wake.plist",
        Some(updater),
    );
    let next_door = (
        "next.door",
        "/Applications/OrbStack.app2/Contents/MacOS/x",
        &[][..],
    );
    agent(&mut machine, "next.door.plist", Some(next_door));
    // WattClean's own job is never offered, whatever it names.
    let own = (
        "co.swatto.wattclean",
        "/Applications/OrbStack.app/Contents/MacOS/x",
        &["dev.kdrag0n.MacVirt"][..],
    );
    agent(&mut machine, "co.swatto.wattclean.plist", Some(own));
    // An updater only a cask's zap list names.
    let updater = ("com.vendor.updater", "/opt/vendor/updater", &[][..]);
    agent(&mut machine, "com.vendor.updater.plist", Some(updater));

    let mut orbstack = Package::new(Manager::MacApp, "/Applications/OrbStack.app", "OrbStack");
    orbstack.bundle_id = Some("dev.kdrag0n.MacVirt".into());
    assert_eq!(
        jobs(&machine, &orbstack, std::slice::from_ref(&orbstack)),
        [
            found("dev.orbstack.helper.plist", Some("dev.orbstack.helper")),
            found("starter.plist", Some("com.example.starter")),
        ]
    );
    // A cask's app, found once the app went; its zap list names that
    // plist too, and another only the list names: both keep their jobs,
    // to be stopped.
    let mut cask = Package::new(Manager::HomebrewCask, "orbstack", "OrbStack");
    cask.app_path = Some("/Applications/OrbStack.app".into());
    cask.zap = vec![
        "~/Library/LaunchAgents/starter.plist".into(),
        "~/Library/LaunchAgents/com.vendor.*.plist".into(),
    ];
    assert_eq!(
        jobs(&machine, &cask, std::slice::from_ref(&cask)),
        [
            found("com.vendor.updater.plist", Some("com.vendor.updater")),
            found("starter.plist", Some("com.example.starter")),
        ]
    );
}

/// Tea's agents are named after the app, by file name or label, or labelled
/// with its bundle identifier plus a suffix. One named after another
/// installed program stays, even labelled after this one; a plist whose
/// contents cannot be read is still offered by its name; names are whole.
#[test]
fn background_jobs_named_after_the_app_are_its() {
    let mut machine = Machine::macos();
    agent(
        &mut machine,
        "Tea.plist",
        Some(("Tea", "/opt/tea/tea", &[][..])),
    );
    agent(
        &mut machine,
        "agent.plist",
        Some(("xyz.tea.tea.helper", "/opt/x", &[][..])),
    );
    agent(&mut machine, "tea.unreadable.plist", None);
    agent(&mut machine, "xyz.tea.tea.plist", None);
    agent(
        &mut machine,
        "coffee.plist",
        Some(("coffee", "/opt/coffee", &[][..])),
    );
    agent(
        &mut machine,
        "milk.plist",
        Some(("tea", "/opt/milk", &[][..])),
    );
    agent(
        &mut machine,
        "helper.plist",
        Some(("tea", "/opt/helper", &[][..])),
    );
    let mut tea = Package::new(Manager::MacApp, "/Applications/Tea.app", "Tea");
    tea.bundle_id = Some("xyz.tea.Tea".into());
    let coffee = Package::new(Manager::MacApp, "/Applications/Coffee.app", "Coffee");
    let mut milk = Package::new(Manager::MacApp, "/Applications/Milk.app", "Milk");
    milk.known_as = vec!["Tea Milk".into()];
    assert_eq!(
        jobs(&machine, &tea, &[tea.clone(), coffee, milk]),
        [
            found("Tea.plist", Some("Tea")),
            found("agent.plist", Some("xyz.tea.tea.helper")),
            found("helper.plist", Some("tea")),
            found("xyz.tea.tea.plist", None),
        ]
    );
    assert_eq!(
        offered(&Machine::new(&[]), &tea, std::slice::from_ref(&tea)),
        Vec::<String>::new(),
        "no LaunchAgents on Linux"
    );
}

/// A formula's service is `homebrew.mxcl.<name>`: the account's own when
/// it is left behind, and root's, stopped before the removal, by its exact
/// name.
#[test]
fn a_formulas_service_plists_are_offered_by_their_exact_name() {
    let mut machine = Machine::macos();
    agent(
        &mut machine,
        "homebrew.mxcl.tailscale.plist",
        Some((
            "homebrew.mxcl.tailscale",
            "/opt/homebrew/opt/tailscale/bin/tailscaled",
            &[][..],
        )),
    );
    agent(&mut machine, "homebrew.mxcl.redis.plist", None);
    let daemons = machine.at("/Library/LaunchDaemons");
    std::fs::create_dir_all(&daemons).unwrap();
    std::fs::write(daemons.join("homebrew.mxcl.tailscale.plist"), b"").unwrap();
    std::fs::write(daemons.join("homebrew.mxcl.redis.plist"), b"").unwrap();
    let tailscale = Package::new(Manager::Homebrew, "tailscale", "tailscale");
    let found: Vec<(PathBuf, Option<String>)> =
        find_leftovers(&machine, &tailscale, std::slice::from_ref(&tailscale))
            .into_iter()
            .map(|l| (l.path, l.job.map(|job| job.label)))
            .collect();
    let home = machine.home().unwrap();
    assert_eq!(
        found,
        [
            (
                home.join("Library/LaunchAgents/homebrew.mxcl.tailscale.plist"),
                Some("homebrew.mxcl.tailscale".to_string())
            ),
            (daemons.join("homebrew.mxcl.tailscale.plist"), None),
        ]
    );
    let cask = Package::new(Manager::HomebrewCask, "tailscale", "tailscale");
    assert!(
        find_leftovers(&machine, &cask, std::slice::from_ref(&cask))
            .iter()
            .all(|l| !l.path.starts_with(&daemons)),
        "a cask has no formula's service"
    );
}
