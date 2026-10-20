//! Read-only check of the package engine on this Linux or macOS machine:
//! lists what the uninstaller would show and, for one program, the command
//! that would remove it and the leftovers it would offer. Changes nothing.
//!
//! `cargo run -p wu-unix --example packages -- [--all] ["<name>"]`

#[cfg(unix)]
fn main() {
    use wu_core::packages::{find_leftovers, list, removal};

    let args: Vec<String> = std::env::args().skip(1).collect();
    let all = args.iter().any(|arg| arg == "--all");
    let wanted = args.iter().find(|arg| !arg.starts_with("--"));
    let machine = wu_unix::UnixPackages::new();
    let started = std::time::Instant::now();
    let (packages, problems) = list(&machine, all);
    for problem in &problems {
        println!("! {problem}");
    }
    let Some(wanted) = wanted else {
        for package in &packages {
            println!(
                "{:<14} {:<40} {:<20} {}",
                package.manager.label(),
                package.name,
                package.version.as_deref().unwrap_or("-"),
                package
                    .size
                    .map_or_else(|| "-".to_string(), |s| format!("{} kB", s / 1000)),
            );
        }
        println!("{} listed in {:?}", packages.len(), started.elapsed());
        return;
    };
    let Some(package) = packages
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(wanted) || p.id.eq_ignore_ascii_case(wanted))
    else {
        println!("{wanted} is not listed (try --all)");
        return;
    };
    println!(
        "{} ({}, {})",
        package.name,
        package.manager.label(),
        package.id
    );
    match removal(&machine, package, false) {
        Ok(plan) => println!("would remove with: {plan:?}"),
        Err(error) => println!("cannot remove: {error}"),
    }
    let (everything, _) = list(&machine, true);
    for leftover in find_leftovers(&machine, package, &everything) {
        let size = leftover
            .size
            .map_or_else(|| "size unknown".to_string(), |s| format!("{s} bytes"));
        let job = leftover
            .job
            .map_or_else(String::new, |label| format!(", background job {label}"));
        println!("leftover: {} ({size}{job})", leftover.path.display());
    }
}

#[cfg(not(unix))]
fn main() {}
