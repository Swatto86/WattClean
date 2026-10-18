//! Read-only check of the leftover scan on this machine: lists what a scan
//! would offer for one installed program, deleting nothing.
//!
//! `cargo run -p wu-windows --example dry_run -- "<program name>" [safe|moderate|advanced]`

use std::time::Instant;

use wu_core::folders::Folders;
use wu_core::identity::Identity;
use wu_core::programs::list_programs;
use wu_core::scan::{Context, Mode, OtherProgram, Progress, scan};

struct Silent;

impl Progress for Silent {
    fn area(&self, _area: &str) {}
    fn cancelled(&self) -> bool {
        false
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let wanted = args.next().ok_or("name a program")?;
    let mode = match args.next().as_deref() {
        Some("safe") => Mode::Safe,
        Some("advanced") => Mode::Advanced,
        _ => Mode::Moderate,
    };
    let system = wu_windows::WindowsSystem::new();
    let folders = Folders::load(&system)?;
    let programs = list_programs(&system, true);
    println!("{} programs listed", programs.len());
    let program = programs
        .iter()
        .find(|p| p.name.to_lowercase().contains(&wanted.to_lowercase()))
        .ok_or("no such program")?;
    let identity = Identity::of_program(&system, &folders, program);
    println!(
        "{}: names {:?} inner {:?} dirs {:?}",
        program.name, identity.names, identity.inner_names, identity.install_dirs
    );
    let others = OtherProgram::all(&system, &folders, &programs, Some(program));
    let context = Context {
        system: &system,
        folders: &folders,
        others: &others,
        progress: &Silent,
        log: None,
        protected: &[],
    };
    let started = Instant::now();
    let report = scan(&identity, mode, &context);
    println!("{:?} scan took {:?}", mode, started.elapsed());
    for item in &report.items {
        println!(
            "{:?}\t{}\t{}",
            item.confidence,
            item.target.display(),
            item.reason
        );
    }
    for warning in &report.warnings {
        println!("warning: {warning}");
    }
    Ok(())
}
