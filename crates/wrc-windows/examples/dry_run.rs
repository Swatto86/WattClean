//! Read-only check of the cleaner on this PC: scans and prints what would be
//! offered, deleting nothing.
//!
//! `cargo run -p wrc-windows --example dry_run -- [safe|moderate|advanced] [all]`

#[cfg(windows)]
fn main() {
    use wrc_core::scan::{Mode, Options, Progress, Request, scan};

    let mode = match std::env::args().nth(1).as_deref() {
        Some("moderate") => Mode::Moderate,
        Some("advanced") => Mode::Advanced,
        _ => Mode::Safe,
    };
    let all_accounts = std::env::args().nth(2).as_deref() == Some("all");
    let system = wrc_windows::WindowsSystem::new();
    let options = Options {
        exclude_removable: true,
        exclude_windows: true,
        ..Options::default()
    };
    let started = std::time::Instant::now();
    let report = scan(
        &system,
        &Request {
            mode,
            categories: Vec::new(),
            all_accounts,
        },
        &options,
        &Progress::default(),
    );
    for finding in &report.findings {
        let target = match &finding.value {
            None => finding.key.display(),
            Some(value) => format!("{} | {value}", finding.key.display()),
        };
        let tick = if finding.ticked { "x" } else { " " };
        println!(
            "[{tick}] {:?}: {target}\n      {}",
            finding.category, finding.detail
        );
    }
    for note in &report.notes {
        println!("note: {note}");
    }
    println!(
        "{} findings in {:.1}s",
        report.findings.len(),
        started.elapsed().as_secs_f32()
    );
}

#[cfg(not(windows))]
fn main() {}
