use crate::packages::testing::{Machine, launchd_job, put_file, put_plist, shown};
use crate::packages::{Domain, Job, Manager, Package, PackageSystem, find_leftovers};

const DAEMON: &str = "/Library/LaunchDaemons/dev.orbstack.OrbStack.privhelper.plist";
const HELPER: &str = "/Library/PrivilegedHelperTools/dev.orbstack.OrbStack.privhelper";
const ORBSTACK: &str = "dev.kdrag0n.MacVirt";

fn orbstack() -> Package {
    let mut app = Package::new(Manager::MacApp, "/Applications/OrbStack.app", "OrbStack");
    app.bundle_id = Some(ORBSTACK.into());
    app
}

/// OrbStack's privileged helper, as it stays on a Mac once the app went.
fn orbstack_daemon(machine: &mut Machine) {
    let job = launchd_job("dev.orbstack.OrbStack.privhelper", HELPER, &[ORBSTACK], &[]);
    put_plist(machine, DAEMON, Some(job));
}

/// The leftovers found outside the home folder, with their jobs.
fn system_wide(
    machine: &Machine,
    removed: &Package,
    installed: &[Package],
) -> Vec<(String, Option<Job>)> {
    let found = find_leftovers(machine, removed, installed);
    let wide: Vec<_> = found.into_iter().filter(|l| l.system_wide).collect();
    shown(machine, &wide)
}

fn daemon(path: &str, label: &str) -> (String, Option<Job>) {
    (path.into(), Some(Job::new(label, Domain::System)))
}

/// Outside the home folder a name is never enough: a plist is the app's
/// only when it names the app's bundle identifier, whole and in the same
/// letters, or starts a program inside the removed bundle; one that also
/// names another installed app, and WattClean's own, stay.
#[test]
fn system_wide_jobs_are_the_apps_only_by_the_id_they_name_or_the_program_they_start() {
    let mut machine = Machine::macos();
    orbstack_daemon(&mut machine);
    let starter = launchd_job(
        "com.example.starter",
        "/Applications/OrbStack.app/Contents/MacOS/OrbStack",
        &[],
        &[],
    );
    put_plist(
        &mut machine,
        "/Library/LaunchAgents/com.example.starter.plist",
        Some(starter),
    );
    let others = [
        ("orbstack", "/opt/orbstack/daemon", &[][..]),
        ("upper", "/opt/upper", &["DEV.KDRAG0N.MACVIRT"][..]),
        ("longer", "/opt/longer", &["dev.kdrag0n.MacVirt.helper"][..]),
        (
            "next.door",
            "/Applications/OrbStack.app2/Contents/MacOS/x",
            &[][..],
        ),
        (
            "shared",
            "/opt/shared",
            &[ORBSTACK, "com.example.Other"][..],
        ),
        (
            "co.swatto.wattclean.helper",
            "/Applications/OrbStack.app/Contents/MacOS/x",
            &[ORBSTACK][..],
        ),
    ];
    for (label, program, associated) in others {
        let job = launchd_job(label, program, associated, &[]);
        put_plist(
            &mut machine,
            &format!("/Library/LaunchDaemons/{label}.plist"),
            Some(job),
        );
    }
    // A link to a plist elsewhere is not followed, whatever it says.
    #[cfg(unix)]
    {
        let elsewhere = put_plist(&mut machine, "/opt/elsewhere/link.plist", None);
        let link = machine.at("/Library/LaunchDaemons/link.plist");
        std::os::unix::fs::symlink(&elsewhere, &link).unwrap();
        let job = launchd_job("link", "/opt/link", &[ORBSTACK], &[]);
        machine.jobs.insert(link, job);
    }
    // Named after the bundle identifier, but its contents say nothing.
    put_plist(
        &mut machine,
        "/Library/LaunchDaemons/dev.kdrag0n.MacVirt.plist",
        None,
    );
    let mut other = Package::new(Manager::MacApp, "/Applications/Other.app", "Other");
    other.bundle_id = Some("com.example.Other".into());
    let app = orbstack();
    assert_eq!(
        system_wide(&machine, &app, &[app.clone(), other]),
        [
            (
                "/Library/LaunchAgents/com.example.starter.plist".into(),
                Some(Job::new("com.example.starter", Domain::Session)),
            ),
            daemon(DAEMON, "dev.orbstack.OrbStack.privhelper"),
        ]
    );

    // A cask's app is found by its bundle too.
    let mut cask = Package::new(Manager::HomebrewCask, "orbstack", "OrbStack");
    cask.app_path = Some("/Applications/OrbStack.app".into());
    assert_eq!(
        system_wide(&machine, &cask, std::slice::from_ref(&cask)),
        [(
            "/Library/LaunchAgents/com.example.starter.plist".into(),
            Some(Job::new("com.example.starter", Domain::Session)),
        )]
    );

    let mut linux = Machine::new(&[]);
    orbstack_daemon(&mut linux);
    assert_eq!(system_wide(&linux, &app, std::slice::from_ref(&app)), []);
}

/// A helper goes only when it sits directly in the helpers' folder, a
/// daemon of the app starts it, and no other plist refers to it (in any
/// letters, anywhere in it). An agent's program is not a helper; a link is
/// never followed.
#[test]
fn a_helper_goes_with_the_daemon_that_starts_it_when_nothing_else_refers_to_it() {
    let mut machine = Machine::macos();
    orbstack_daemon(&mut machine);
    put_file(&machine, HELPER);
    let helpers = [
        ("dev.orbstack.deep", "OrbStack/deep"),
        ("dev.orbstack.shared", "shared-helper"),
        ("dev.orbstack.link", "linked"),
    ];
    for (label, helper) in helpers {
        let program = format!("/Library/PrivilegedHelperTools/{helper}");
        put_file(&machine, &program);
        let job = launchd_job(label, &program, &[ORBSTACK], &[]);
        put_plist(
            &mut machine,
            &format!("/Library/LaunchDaemons/{label}.plist"),
            Some(job),
        );
    }
    std::fs::remove_file(machine.at("/Library/PrivilegedHelperTools/linked")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        machine.at(HELPER),
        machine.at("/Library/PrivilegedHelperTools/linked"),
    )
    .unwrap();
    let tool = launchd_job(
        "com.vendor.tool",
        "/usr/local/bin/tool",
        &[],
        &["--helper=/Library/PrivilegedHelperTools/Shared-Helper"],
    );
    put_plist(
        &mut machine,
        "/Library/LaunchDaemons/com.vendor.tool.plist",
        Some(tool),
    );
    let agent_helper = "/Library/PrivilegedHelperTools/dev.orbstack.agent";
    put_file(&machine, agent_helper);
    let agent = launchd_job("dev.orbstack.agent", agent_helper, &[ORBSTACK], &[]);
    put_plist(
        &mut machine,
        "/Library/LaunchAgents/dev.orbstack.agent.plist",
        Some(agent),
    );
    let app = orbstack();
    assert_eq!(
        system_wide(&machine, &app, std::slice::from_ref(&app)),
        [
            (
                "/Library/LaunchAgents/dev.orbstack.agent.plist".into(),
                Some(Job::new("dev.orbstack.agent", Domain::Session)),
            ),
            daemon(DAEMON, "dev.orbstack.OrbStack.privhelper"),
            daemon(
                "/Library/LaunchDaemons/dev.orbstack.deep.plist",
                "dev.orbstack.deep"
            ),
            daemon(
                "/Library/LaunchDaemons/dev.orbstack.link.plist",
                "dev.orbstack.link"
            ),
            daemon(
                "/Library/LaunchDaemons/dev.orbstack.shared.plist",
                "dev.orbstack.shared"
            ),
            daemon(HELPER, "dev.orbstack.OrbStack.privhelper"),
        ]
    );

    // Without the daemon that starts it, the helper is not the app's.
    std::fs::remove_file(machine.at(DAEMON)).unwrap();
    let still = system_wide(&machine, &app, std::slice::from_ref(&app));
    assert!(
        still.iter().all(|(path, _)| path != HELPER),
        "no daemon starts it: {still:?}"
    );
}

/// A plist this account cannot read might start any helper.
#[cfg(unix)]
#[test]
fn no_helper_goes_while_a_plist_cannot_be_read() {
    use std::os::unix::fs::PermissionsExt;

    let mut machine = Machine::macos();
    orbstack_daemon(&mut machine);
    put_file(&machine, HELPER);
    let locked = put_plist(&mut machine, "/Library/LaunchDaemons/locked.plist", None);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::File::open(&locked).is_ok() {
        // Run as root, which reads anything: nothing here is unreadable.
        return;
    }
    let app = orbstack();
    assert_eq!(
        system_wide(&machine, &app, std::slice::from_ref(&app)),
        [daemon(DAEMON, "dev.orbstack.OrbStack.privhelper")]
    );
}
