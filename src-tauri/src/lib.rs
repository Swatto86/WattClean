//! Composition root: find the data folder (taking over the old apps' data
//! at first start on Windows), choose the machines, start the Registry
//! Watcher, put the icon in the notification area, register every module's
//! commands, and decide what closing the window does. Behaviour lives in
//! the engines (`wcc-core` disk, `wrc-core` registry, `wu-core` uninstaller);
//! this file only wires them.
//!
//! The registry cleaner and the registry-based uninstaller exist on Windows
//! only; on Linux and macOS the uninstaller is `packages`, over the package
//! managers.

mod commands;
mod disk;
mod error;
#[cfg(windows)]
mod migrate;
#[cfg(not(windows))]
mod packages;
#[cfg(windows)]
mod registry;
mod settings;
mod shell;
mod tray;
#[cfg(windows)]
mod uninstall;

use std::sync::Arc;

use shell::Shell;
use tauri::{Manager, WindowEvent};
use wrc_core::autostart::BACKGROUND_FLAG;

/// Run a scheduled registry job headless (`--run-job <id>`), with a
/// notification saying how it went, and return the exit code: 0 done, 1
/// the job reported a problem, 2 it could not run.
#[cfg(windows)]
pub fn run_job(id: &str) -> i32 {
    let outcome = wrc_core::store::data_dir().and_then(|data_dir| {
        std::fs::create_dir_all(&data_dir).map_err(|error| error.to_string())?;
        let (system, _) =
            registry::platform::machine(&data_dir).map_err(|error| error.to_string())?;
        let (outcome, shown) = wrc_core::jobs::run_scheduled(system.as_ref(), &data_dir, id);
        if let Err(error) = shown {
            eprintln!("WattClean could not show its notification: {error}");
        }
        outcome.map_err(|error| error.to_string())
    });
    match outcome {
        Ok(last) if last.ok => 0,
        Ok(_) => 1,
        Err(error) => {
            eprintln!("WattClean could not run schedule {id}: {error}");
            2
        }
    }
}

/// Schedules are registry jobs, which exist on Windows only.
#[cfg(not(windows))]
pub fn run_job(id: &str) -> i32 {
    eprintln!("WattClean could not run schedule {id}: schedules are Windows registry jobs.");
    2
}

pub fn run() {
    let args: Vec<String> = std::env::args().collect();
    #[cfg(windows)]
    if let Some(pid) = shell::restarted_from(&args) {
        registry::platform::wait_for_exit(pid);
    }
    let started_hidden = args.iter().any(|arg| arg == BACKGROUND_FLAG);
    let builder = tauri::Builder::default()
        // First, so a second launch shows this window instead of a copy. A
        // second launch may have fewer rights than this copy (see
        // `registry::platform::accept_second_launch`): its arguments only
        // ever decide whether to show the window.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if !args.iter().any(|arg| arg == BACKGROUND_FLAG) {
                tray::show_main(app);
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| start(app, started_hidden));
    #[cfg(windows)]
    let builder = builder.on_menu_event(registry::menu::on_menu_event);
    let app = builder
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                // Hide first: nothing below may stop the window going.
                api.prevent_close();
                if app
                    .try_state::<Arc<Shell>>()
                    .is_some_and(|shell| shell.keeps_running())
                {
                    let _ = window.hide();
                } else {
                    shell::quit_when_idle(app);
                }
            }
        })
        .invoke_handler(commands::handler())
        .build(tauri::generate_context!());
    let app = match app {
        Ok(app) => app,
        Err(error) => {
            // Nothing is on screen yet to show it in; stderr is all there is.
            eprintln!("WattClean could not start: {error}");
            std::process::exit(1);
        }
    };
    app.run(on_event);
}

/// The Exit event stops the Registry Watcher.
#[cfg(windows)]
fn on_event(handle: &tauri::AppHandle, event: tauri::RunEvent) {
    if let tauri::RunEvent::Exit = event
        && let Some(state) = handle.try_state::<Arc<registry::state::AppState>>()
    {
        state.watcher.stop_all();
    }
}

/// Clicking the Dock icon (or opening the app again) shows the window that
/// closing hid. macOS's own Quit (Cmd+Q, the Dock's menu, logging out)
/// skips the close and tray paths, so Exit waits for a running job.
#[cfg(target_os = "macos")]
fn on_event(handle: &tauri::AppHandle, event: tauri::RunEvent) {
    match event {
        tauri::RunEvent::Reopen { .. } => tray::show_main(handle),
        tauri::RunEvent::Exit => shell::wait_until_idle(handle),
        _ => {}
    }
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn on_event(_handle: &tauri::AppHandle, _event: tauri::RunEvent) {}

/// Find the data folder, open each module's machine and state, start the
/// watcher and the tray icon.
fn start(app: &mut tauri::App, started_hidden: bool) -> Result<(), Box<dyn std::error::Error>> {
    let root = shell::root()?;
    std::fs::create_dir_all(&root)?;
    // The WebDriver suite sets its own data folder and must never adopt
    // this PC's data. The apps WattClean replaced ran on Windows only.
    #[cfg(windows)]
    let adopt_problems = if std::env::var_os("WATTCLEAN_DATA_DIR").is_none() {
        migrate::adopt(&shell::local_app_data()?, &root).problems
    } else {
        Vec::new()
    };
    #[cfg(not(windows))]
    let adopt_problems = Vec::new();

    let disk_dir = root.join("disk");
    std::fs::create_dir_all(&disk_dir)?;
    let (disk_system, fake) = disk::platform::machine(&disk_dir)?;
    let disk = Arc::new(disk::state::AppState::new(disk_system, disk_dir, fake));
    let tidy = Arc::clone(&disk);
    std::thread::spawn(move || disk::remove_wipe_leftovers(&tidy));

    #[cfg(windows)]
    start_windows_modules(app, &root, fake)?;
    #[cfg(not(windows))]
    app.manage(Arc::new(packages::AppState::new(Arc::new(
        wu_unix::UnixPackages::new().with_data(root.join("uninstall")),
    ))));

    app.manage(Arc::new(Shell::new(
        root,
        started_hidden,
        fake,
        adopt_problems,
    )));
    app.manage(disk);
    tray::build(app.handle())?;
    #[cfg(windows)]
    if let Err(error) = registry::platform::accept_second_launch(&app.config().identifier) {
        eprintln!("A second launch may not reach this window: {error}");
    }
    Ok(())
}

/// The registry cleaner (with its watcher) and the registry-based
/// uninstaller.
#[cfg(windows)]
fn start_windows_modules(
    app: &mut tauri::App,
    root: &std::path::Path,
    fake: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    use wrc_core::watch::Watcher;

    let registry_dir = root.join("registry");
    std::fs::create_dir_all(&registry_dir)?;
    let (registry_system, _) = registry::platform::machine(&registry_dir)?;
    let watcher = Watcher::start(Arc::clone(&registry_system), &registry_dir)?;
    let registry_settings = wrc_core::settings::Settings::load(&registry_dir).unwrap_or_default();
    registry::prune_backups(&registry_dir, &registry_settings);
    let registry = Arc::new(registry::state::AppState::new(
        registry_system,
        registry_dir,
        watcher,
    ));

    let uninstall_dir = root.join("uninstall");
    std::fs::create_dir_all(&uninstall_dir)?;
    let (uninstall_system, _) = uninstall::platform::machine(&uninstall_dir)?;
    let uninstall = Arc::new(uninstall::state::AppState::new(
        uninstall_system,
        uninstall_dir,
        root,
        fake,
    ));
    app.manage(registry);
    app.manage(uninstall);
    Ok(())
}
