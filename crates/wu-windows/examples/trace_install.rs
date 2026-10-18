//! Run an installer under the same install trace the app uses and print the
//! install log it would save. Run it as administrator.
//!
//! `trace_install [--out report.txt] <installer> [arguments...]`

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use wu_core::System;
use wu_core::folders::Folders;
use wu_core::install_log::{Recorded, build};
use wu_core::system::Launch;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut out = None;
    if args.first().is_some_and(|first| first == "--out") && args.len() > 1 {
        out = Some(args[1].clone());
        args.drain(..2);
    }
    let Some((installer, rest)) = args.split_first() else {
        return Err("usage: trace_install [--out report.txt] <installer> [arguments...]".into());
    };
    let system = wu_windows::WindowsSystem::new();
    let folders = Folders::load(&system)?;
    let launch = Launch {
        program: installer.into(),
        args: rest.join(" "),
    };
    let started = Instant::now();
    let mut tracing = system.trace_install(&launch)?;
    while !tracing.finished()? {
        std::thread::sleep(Duration::from_millis(200));
    }
    let took = started.elapsed();
    let trace = tracing.finish()?;
    let log = build(&system, &folders, &trace, installer, 0);
    let mut text = String::new();
    let _ = writeln!(text, "Traced {installer} in {:.1} s", took.as_secs_f64());
    let _ = writeln!(
        text,
        "{} changes; events lost {}; unnamed registry changes {}; program {:?}",
        trace.changes.len(),
        trace.events_lost,
        trace.unnamed,
        log.program
    );
    for process in &trace.processes {
        let _ = writeln!(text, "  process {process}");
    }
    for item in &log.items {
        let recorded = match item.recorded {
            Recorded::Created => "created",
            Recorded::Changed => "changed",
        };
        let _ = writeln!(text, "  {recorded:<8} {}", item.target.display());
    }
    print!("{text}");
    if let Some(out) = out {
        std::fs::write(out, text)?;
    }
    Ok(())
}
