use super::*;

/// Package managers print progress on stdout and their error on
/// stderr; the error is what a failure reports.
#[test]
fn a_programs_last_words_are_its_error_not_its_progress() {
    let said = |stdout: &str, stderr: &str| Output {
        success: false,
        stdout: stdout.into(),
        stderr: stderr.into(),
    };
    let cases = [
        (
            said(
                "==> Uninstalling Cask battle-net\n==> Removing files:\n/Applications/Battle.net.app\n",
                "Error: Failure while executing; `/usr/bin/sudo -u root -- /bin/rm -f -r -- /Applications/Battle.net.app` exited with 1. Here's the output:\nsudo: a terminal is required to read the password\nsudo: a password is required\n",
            ),
            Some("sudo: a terminal is required to read the password"),
        ),
        (
            said(
                "Reading package lists...\nBuilding dependency tree...\nReading state information...\n",
                "E: Could not get lock /var/lib/dpkg/lock-frontend. It is held by process 4242 (unattended-upgr)\nN: Be aware that removing the lock file is not a solution and may break your system.\nE: Unable to acquire the dpkg frontend lock (/var/lib/dpkg/lock-frontend), is another process using it?\n",
            ),
            Some(
                "E: Unable to acquire the dpkg frontend lock (/var/lib/dpkg/lock-frontend), is another process using it?",
            ),
        ),
        (
            // pacman 7, as it printed on Arch Linux.
            said(
                "checking dependencies...\n:: removing testlib breaks dependency 'testlib' required by needer\n",
                "warning: database file for 'core' does not exist (use '-Sy' to download)\n\
                 error: failed to prepare transaction (could not satisfy dependencies)\n",
            ),
            Some("removing testlib breaks dependency 'testlib' required by needer"),
        ),
        (
            said("", "error: target not found: nosuchthing\n"),
            Some("error: target not found: nosuchthing"),
        ),
        (said("Removing vlc...\n  done\n", "  \n"), Some("done")),
        (said("", ""), None),
    ];
    for (output, expected) in cases {
        assert_eq!(output.last_words(), expected, "{output:?}");
    }
}

#[test]
fn a_path_is_within_a_bundle_only_below_its_whole_name() {
    let tea = Path::new("/Applications/Tea.app");
    assert!(within("/Applications/Tea.app/Contents/MacOS/tea", tea));
    assert!(within("/applications/TEA.APP/Contents/MacOS/tea", tea));
    assert!(within(
        "/Applications/Tea.app/x",
        Path::new("/Applications/Tea.app/")
    ));
    assert!(!within("/Applications/Tea.app2/Contents/MacOS/tea", tea));
    assert!(!within("/Applications/Tea.app", tea));
    assert!(!within("/Applications/Tea.app/", tea));
    assert!(!within("/Applications/Te", tea));
    assert!(!within("/Applications/Tea.app/x", Path::new("/")));
}

#[test]
fn sizes_are_read_in_every_unit_package_managers_print() {
    assert_eq!(parse_size("640 kB"), Some(640_000));
    assert_eq!(parse_size("1.5 GB"), Some(1_500_000_000));
    assert_eq!(parse_size("12,0 MiB"), Some(12_582_912));
    assert_eq!(parse_size("512"), Some(512));
    assert_eq!(parse_size("lots"), None);
    assert_eq!(parse_size("3 parsecs"), None);
}
