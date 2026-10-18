//! `wattclean-cmd`: one console program for the disk cleaner
//! (`wattclean-cmd disk …`, BleachBit's options) and the registry cleaner
//! (`wattclean-cmd registry …`, Revo's `RRCcmd` verbs). A console program,
//! not the app with a console attached, so its exit code reaches the caller.

const USAGE: &str = "\
WattClean command line

  wattclean-cmd disk ...       the disk cleaner (wattclean-cmd disk --help)
  wattclean-cmd registry ...   the registry cleaner (wattclean-cmd registry -help)
  wattclean-cmd --version

Exit codes: 0 done, 1 done with problems, 2 bad arguments or could not run.";

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest = args.get(1..).unwrap_or_default();
    let code = match args.first().map(String::as_str) {
        Some("disk") => wcc_cmd::console(rest),
        Some("registry") => wrc_cmd::console(rest),
        Some("--version" | "-v") => {
            println!("WattClean {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Some("--help" | "-h" | "/?" | "help") => {
            println!("{USAGE}");
            0
        }
        _ => {
            eprintln!("{USAGE}");
            2
        }
    };
    std::process::ExitCode::from(u8::try_from(code).unwrap_or(2))
}
