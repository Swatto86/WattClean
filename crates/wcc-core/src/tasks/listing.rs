//! Reading what a task's listing step printed: names checked before they
//! are handed back to a package manager.

use super::{Listed, Parse, Use};

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

/// What a listing step's output names. A name passed back to the package
/// manager must pass [`safe_name`]; others are left out, so the command
/// never acts on them. A name only shown (`Use::Preview`, where the command
/// acts on its own) is kept when each `/`-separated part passes, as in a
/// Homebrew tap's `user/repo/name`; any still refused are counted first,
/// never dropped silently, since the command acts on them all the same.
pub fn parse(parse: Parse, then: Use, output: &str) -> Vec<Listed> {
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
        Parse::Tool => output
            .lines()
            .map(str::trim)
            .find(|line| line.starts_with('/'))
            .map(|_| Listed {
                label: "Xcode's simulators".to_string(),
                args: vec!["tool".to_string()],
            })
            .into_iter()
            .collect(),
        Parse::SnapshotDates => output
            .lines()
            .map(str::trim)
            .filter(|line| snapshot_date(line))
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
    let found = out.len();
    out.retain(|listed| {
        listed.args.first().is_some_and(|name| match then {
            Use::Preview => name.split('/').all(safe_name),
            Use::Append | Use::Each => safe_name(name),
        })
    });
    let refused = found - out.len();
    out.dedup();
    if then == Use::Preview && refused > 0 {
        let label = if refused == 1 {
            "1 name that cannot be shown".to_string()
        } else {
            format!("{refused} names that cannot be shown")
        };
        out.insert(
            0,
            Listed {
                label,
                args: Vec::new(),
            },
        );
    }
    out
}

/// `YYYY-MM-DD-HHMMSS`, as `tmutil` names a snapshot.
fn snapshot_date(line: &str) -> bool {
    let digits =
        |part: &str, len: usize| part.len() == len && part.bytes().all(|b| b.is_ascii_digit());
    let parts: Vec<&str> = line.split('-').collect();
    matches!(parts.as_slice(), [year, month, day, time]
        if digits(year, 4) && digits(month, 2) && digits(day, 2) && digits(time, 6))
}

#[cfg(test)]
mod tests {
    use super::*;

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
            parse(Parse::AptRemoved, Use::Append, apt),
            vec![Listed {
                label: "libold1".into(),
                args: vec!["libold1".into()],
            }]
        );
        let dpkg = "ii  bash\nrc  oldapp\nrc  other:amd64\n";
        let names: Vec<_> = parse(Parse::DpkgResidual, Use::Append, dpkg)
            .into_iter()
            .map(|l| l.label)
            .collect();
        assert_eq!(names, ["oldapp", "other:amd64"]);
        let brew = "==> Would autoremove 2 unneeded formulae:\nlibyaml\nopenssl@1.1\n";
        let names: Vec<_> = parse(Parse::Lines, Use::Preview, brew)
            .into_iter()
            .map(|l| l.label)
            .collect();
        assert_eq!(names, ["libyaml", "openssl@1.1"]);
        let snap = "Name  Version  Rev  Tracking  Publisher  Notes\n\
                    core  16-2.61  16928  latest/stable  canonical✓  core,disabled\n\
                    core  16-2.62  17200  latest/stable  canonical✓  core\n\
                    firefox  128.0  4650  latest/stable  mozilla✓  disabled\n";
        assert!(parse(Parse::Tool, Use::Preview, "").is_empty());
        assert!(
            parse(
                Parse::Tool,
                Use::Preview,
                "xcrun: error: unable to find utility \"simctl\"\n"
            )
            .is_empty()
        );
        let xcode = "/Applications/Xcode.app/Contents/Developer/usr/bin/simctl\n";
        assert_eq!(parse(Parse::Tool, Use::Preview, xcode).len(), 1);
        let listed = parse(Parse::SnapDisabled, Use::Each, snap);
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].label, "core (revision 16928)");
        assert_eq!(listed[1].args, ["firefox", "--revision=4650"]);
    }

    /// `tmutil listlocalsnapshotdates /` starts with a heading, which is no
    /// snapshot; only `YYYY-MM-DD-HHMMSS` lines are.
    #[test]
    fn snapshot_dates_are_read_without_the_heading() {
        let list = crate::tasks::find("tm-snapshots")
            .and_then(|task| task.list)
            .unwrap();
        let out = "Snapshot dates for disk /:\n2026-10-19-101530\n 2026-10-20-091500 \n\
                   not-a-date\n2026-1-19-101530\n2026-10-19-10153x\n";
        let labels: Vec<_> = parse(list.parse, list.then, out)
            .into_iter()
            .map(|l| l.label)
            .collect();
        assert_eq!(labels, ["2026-10-19-101530", "2026-10-20-091500"]);
        assert!(parse(list.parse, list.then, "Snapshot dates for disk /:\n").is_empty());
    }

    /// A name only shown may be a tap's `user/repo/name`; one passed back
    /// may not. A shown listing never drops a name without saying so.
    #[test]
    fn a_preview_keeps_tap_names_and_counts_what_it_cannot_show() {
        let brew = "==> Would autoremove 4 unneeded formulae:\n\
                    hashicorp/tap/foo\nlibyaml\n-rf\na//b\n";
        let labels = |then| -> Vec<String> {
            parse(Parse::Lines, then, brew)
                .into_iter()
                .map(|l| l.label)
                .collect()
        };
        assert_eq!(
            labels(Use::Preview),
            [
                "2 names that cannot be shown",
                "hashicorp/tap/foo",
                "libyaml"
            ]
        );
        assert_eq!(labels(Use::Append), ["libyaml"]);
        assert_eq!(labels(Use::Each), ["libyaml"]);
        let one = parse(Parse::Lines, Use::Preview, "$(id)\n");
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].label, "1 name that cannot be shown");
    }
}
