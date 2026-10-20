use crate::packages::testing::{Machine, launchd_job, place, put_file, put_plist, shown};
use crate::packages::{Domain, Job, Manager, Package, orphans};

const DAEMON: &str = "/Library/LaunchDaemons/dev.orbstack.OrbStack.privhelper.plist";
const HELPER: &str = "/Library/PrivilegedHelperTools/dev.orbstack.OrbStack.privhelper";

/// What Spotlight is asked for an app with this bundle identifier.
fn query(id: &str) -> String {
    format!("mdfind kMDItemCFBundleIdentifier == \"{id}\"c")
}

/// A Mac with Tea installed, which Spotlight finds.
fn mac() -> (Machine, Vec<Package>) {
    let machine = Machine::macos();
    machine.prints("mdfind", "");
    machine.replies(&query("xyz.tea.Tea"), true, "/Applications/Tea.app\n", "");
    std::fs::create_dir_all(place(&machine, "/Applications/Tea.app/Contents/MacOS")).unwrap();
    let mut tea = Package::new(Manager::MacApp, "/Applications/Tea.app", "Tea");
    tea.bundle_id = Some("xyz.tea.Tea".into());
    (machine, vec![tea])
}

fn agent(machine: &mut Machine, label: &str, program: &str, associated: &[&str]) {
    let job = launchd_job(label, program, associated, &[]);
    let path = format!("~/Library/LaunchAgents/{label}.plist");
    put_plist(machine, &path, Some(job));
}

fn listed(machine: &Machine, installed: &[Package]) -> Vec<(String, Option<Job>)> {
    shown(machine, &orphans(machine, installed).leftovers)
}

fn in_session(label: &str) -> (String, Option<Job>) {
    (
        format!("~/Library/LaunchAgents/{label}.plist"),
        Some(Job::new(label, Domain::Session)),
    )
}

/// OrbStack's helper stays after OrbStack.app went: its plist names the
/// app, which is neither listed nor found by Spotlight, and the helper
/// its daemon starts goes with it. A job naming an app that is still
/// there (listed, or found by Spotlight anywhere but in a Trash) is not
/// an orphan, nor is one naming an identifier Spotlight cannot be asked
/// about.
#[test]
fn a_job_is_an_orphan_when_the_apps_it_names_cannot_be_found() {
    let (mut machine, installed) = mac();
    let job = launchd_job(
        "dev.orbstack.OrbStack.privhelper",
        HELPER,
        &["dev.kdrag0n.MacVirt"],
        &[],
    );
    put_plist(&mut machine, DAEMON, Some(job));
    put_file(&machine, HELPER);
    agent(
        &mut machine,
        "both",
        "/opt/both",
        &["dev.kdrag0n.MacVirt", "XYZ.tea.tea"],
    );
    machine.replies(
        &query("com.google.GoogleUpdater"),
        true,
        "/Users/t/Library/Application Support/Google/GoogleUpdater/157.0/GoogleUpdater.app\n",
        "",
    );
    agent(
        &mut machine,
        "com.google.GoogleUpdater.wake",
        "/Users/t/Library/Application Support/Google/GoogleUpdater/Current/GoogleUpdater.app/Contents/MacOS/GoogleUpdater",
        &["com.google.GoogleUpdater"],
    );
    machine.replies(
        &query("com.gone.Gone"),
        true,
        "/Users/t/.Trash/Gone.app\n/Volumes/Data/.Trashes/501/Gone.app\n",
        "",
    );
    agent(
        &mut machine,
        "com.gone.agent",
        "/opt/gone",
        &["com.gone.Gone"],
    );
    agent(
        &mut machine,
        "odd",
        "/opt/odd",
        &["bad\" || kMDItemFSName == \"*"],
    );

    let found = orphans(&machine, &installed);
    assert!(found.spotlight);
    assert_eq!(
        shown(&machine, &found.leftovers),
        [
            in_session("com.gone.agent"),
            (
                DAEMON.into(),
                Some(Job::new("dev.orbstack.OrbStack.privhelper", Domain::System))
            ),
            (
                HELPER.into(),
                Some(Job::new("dev.orbstack.OrbStack.privhelper", Domain::System))
            ),
        ]
    );
    assert!(
        found
            .leftovers
            .iter()
            .all(|l| l.system_wide == !l.path.starts_with(machine_home(&machine))),
        "{:?}",
        found.leftovers
    );
    assert!(
        machine.ran().iter().all(|run| !run.contains("bad")),
        "{:?}",
        machine.ran()
    );
}

fn machine_home(machine: &Machine) -> std::path::PathBuf {
    place(machine, "~/")
}

/// A job whose program lay inside an app bundle that is gone is an orphan,
/// whatever helper app inside it the program was. A job that merely points
/// at a missing program is not (a tool's path changes when it is upgraded),
/// nor is one inside an app that is still there, one on another disk (it
/// may only be unplugged), or one naming an app that is installed.
#[test]
fn a_job_is_an_orphan_when_the_app_its_program_lay_in_is_gone() {
    let (mut machine, installed) = mac();
    agent(
        &mut machine,
        "com.gone.helper",
        "/Applications/Gone.app/Contents/Library/LoginItems/Helper.app/Contents/MacOS/Helper",
        &[],
    );
    agent(
        &mut machine,
        "uk.co.swatto.agent-standards-collector",
        "/opt/homebrew/Cellar/powershell/7.6.5/libexec/pwsh",
        &[],
    );
    agent(
        &mut machine,
        "tea.old",
        "/Applications/Tea.app/Contents/MacOS/old",
        &[],
    );
    agent(
        &mut machine,
        "unplugged",
        "/Volumes/Backup/Old.app/Contents/MacOS/old",
        &[],
    );
    agent(
        &mut machine,
        "moved",
        "/Applications/Gone.app/Contents/MacOS/x",
        &["xyz.tea.Tea"],
    );
    agent(&mut machine, "relative", "Gone.app/Contents/MacOS/x", &[]);
    agent(
        &mut machine,
        "climbs",
        "/Applications/Tea.app/../Gone.app/Contents/MacOS/x",
        &[],
    );
    assert_eq!(
        listed(&machine, &installed),
        [in_session("com.gone.helper")]
    );
}

/// Without Spotlight, a job is judged by its program alone. Spotlight is
/// trusted only once it finds an app known to be there: the first listed
/// app with a bundle identifier, else Finder.
#[test]
fn without_spotlight_only_a_gone_app_folder_makes_an_orphan() {
    let setup = |machine: &mut Machine| {
        let job = launchd_job(
            "dev.orbstack.OrbStack.privhelper",
            HELPER,
            &["dev.kdrag0n.MacVirt"],
            &[],
        );
        put_plist(machine, DAEMON, Some(job));
        put_file(machine, HELPER);
        agent(
            machine,
            "com.gone.helper",
            "/Applications/Gone.app/Contents/MacOS/Gone",
            &["com.gone.Gone"],
        );
    };
    let (mut blind, installed) = mac();
    blind.replies(&query("xyz.tea.Tea"), true, "", "");
    setup(&mut blind);
    let found = orphans(&blind, &installed);
    assert!(!found.spotlight);
    assert_eq!(
        shown(&blind, &found.leftovers),
        [in_session("com.gone.helper")]
    );

    let mut missing = Machine::macos();
    setup(&mut missing);
    assert_eq!(
        listed(&missing, &[]),
        [in_session("com.gone.helper")],
        "no mdfind"
    );

    let mut finder = Machine::macos();
    finder.prints("mdfind", "");
    finder.replies(
        &query("com.apple.finder"),
        true,
        "/System/Library/CoreServices/Finder.app\n",
        "",
    );
    setup(&mut finder);
    assert!(orphans(&finder, &[]).spotlight, "Finder is always there");
    assert_eq!(listed(&finder, &[]).len(), 3);
}

/// Apple's and WattClean's own jobs are never listed, and Linux has no
/// launchd.
#[test]
fn apples_and_wattcleans_own_jobs_are_never_orphans() {
    let (mut machine, installed) = mac();
    let gone = "/Applications/Gone.app/Contents/MacOS/Gone";
    let apples = launchd_job("com.apple.gone", gone, &[], &[]);
    put_plist(
        &mut machine,
        "~/Library/LaunchAgents/gone.plist",
        Some(apples),
    );
    let own = launchd_job("co.swatto.wattclean.agent", gone, &[], &[]);
    put_plist(&mut machine, "~/Library/LaunchAgents/own.plist", Some(own));
    let job = launchd_job("com.vendor.gone", gone, &[], &[]);
    put_plist(
        &mut machine,
        "~/Library/LaunchAgents/com.apple.vendor.plist",
        Some(job),
    );
    assert_eq!(listed(&machine, &installed), []);

    let mut linux = Machine::new(&["mdfind"]);
    agent(&mut linux, "com.gone.helper", gone, &[]);
    assert_eq!(listed(&linux, &[]), []);
}
