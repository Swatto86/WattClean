// No console window behind the app in a release build.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Task Scheduler starts the app with `--run-job <id>`: do the registry
    // job and leave, with no window and no tray.
    let args: Vec<String> = std::env::args().collect();
    if let Some(at) = args.iter().position(|arg| arg == "--run-job") {
        let id = args.get(at + 1).map_or("", String::as_str);
        std::process::exit(wattclean_lib::run_job(id));
    }
    wattclean_lib::run();
}
