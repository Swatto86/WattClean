//! What else removing a package would take with it. APT, dnf and zypper,
//! told yes, also remove every package that needs the one chosen, so the
//! package manager is asked first (without administrator rights) and the
//! removal is refused when anything else would go.

use std::path::Path;

use super::{Output, Package, PackageSystem};

/// Refuses a removal by `remover` (found at `path`) that would take other
/// packages with it: APT's is simulated first (`apt-get -s`), dnf's and
/// zypper's tested with rpm (`rpm -e --test --allmatches`).
pub(super) fn takes_nothing_else(
    system: &dyn PackageSystem,
    package: &Package,
    remover: &str,
    path: &Path,
    purge: bool,
) -> Result<(), String> {
    match remover {
        "apt-get" => goes_alone(
            system,
            package,
            remover,
            path,
            &["-s", if purge { "purge" } else { "remove" }],
            apt_takes,
        )?,
        "dnf" | "zypper" => {
            let rpm = system.find_admin_program("rpm").ok_or_else(|| {
                format!(
                    "rpm is not installed, so WattClean cannot check what else removing {} would remove",
                    package.name
                )
            })?;
            // dnf and zypper remove every installed copy of the name (both
            // architectures); rpm tests them all only when told to.
            let test = ["-e", "--test", "--allmatches"];
            goes_alone(system, package, remover, &rpm, &test, rpm_takes)?;
        }
        _ => {}
    }
    Ok(())
}

/// Asks `checker` (with `args` and the package's name, without
/// administrator rights) what removing `package` with `remover` would take;
/// an error naming the others when anything besides the package would go.
fn goes_alone(
    system: &dyn PackageSystem,
    package: &Package,
    remover: &str,
    checker: &Path,
    args: &[&str],
    read: fn(&Output) -> Option<Vec<String>>,
) -> Result<(), String> {
    let args: Vec<String> = args
        .iter()
        .copied()
        .chain([package.id.as_str()])
        .map(ToString::to_string)
        .collect();
    let output = system.run(checker, &args, false, &[]).map_err(|error| {
        format!(
            "WattClean could not check what else removing {} would remove: {error}",
            package.name
        )
    })?;
    let Some(mut others) = read(&output) else {
        let said = output
            .last_words()
            .unwrap_or("its package manager gave no answer");
        return Err(format!(
            "WattClean could not check what else removing {} would remove, so it was left alone: {said}",
            package.name
        ));
    };
    // `name:arch` is the chosen package's own copy for another architecture.
    others.retain(|other| other.split(':').next() != Some(package.id.as_str()));
    others.sort();
    others.dedup();
    if others.is_empty() {
        return Ok(());
    }
    const NAMED: usize = 5;
    let mut list = others
        .iter()
        .take(NAMED)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    if others.len() > NAMED {
        list.push_str(&format!(" and {} more", others.len() - NAMED));
    }
    let (count, verb) = match others.len() {
        1 => ("1 other package".to_string(), "needs"),
        n => (format!("{n} other packages"), "need"),
    };
    Err(format!(
        "Removing {} would also remove {count} that {verb} it: {list}. WattClean removes only the program chosen; if you mean to remove them all, do it with {remover} yourself.",
        package.name
    ))
}

/// The packages `apt-get -s remove|purge` lists (`Remv name [version]`,
/// `Purg …`); `None` when the simulation failed.
pub(super) fn apt_takes(output: &Output) -> Option<Vec<String>> {
    output.success.then(|| {
        output
            .stdout
            .lines()
            .chain(output.stderr.lines())
            .filter_map(|line| {
                line.strip_prefix("Remv ")
                    .or_else(|| line.strip_prefix("Purg "))
            })
            .filter_map(|rest| rest.split_whitespace().next())
            .map(ToString::to_string)
            .collect()
    })
}

/// The packages `rpm -e --test` says need the one asked about (`… is
/// needed by (installed) name-version-release.arch`); none when it would
/// succeed, `None` when it failed for another reason.
pub(super) fn rpm_takes(output: &Output) -> Option<Vec<String>> {
    if output.success {
        return Some(Vec::new());
    }
    let needing: Vec<String> = output
        .stderr
        .lines()
        .chain(output.stdout.lines())
        .filter_map(|line| line.rsplit_once(" is needed by "))
        .map(|(_, by)| by.trim().trim_start_matches("(installed)").trim())
        .filter(|nevra| !nevra.is_empty())
        .map(|nevra| {
            // The version and release hold no `-`; the name may.
            let mut parts = nevra.rsplitn(3, '-');
            match (parts.next(), parts.next(), parts.next()) {
                (Some(_), Some(_), Some(name)) => name.to_string(),
                _ => nevra.to_string(),
            }
        })
        .collect();
    (!needing.is_empty()).then_some(needing)
}
