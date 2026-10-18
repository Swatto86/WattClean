//! Composition root: find the data folder (taking over the old apps' data
//! at first start), choose the machines, start the Registry Watcher, put the
//! icon in the notification area, register every module's commands, and
//! decide what closing the window does. Behaviour lives in the engines
//! (`wcc-core` disk, `wrc-core` registry, `wu-core` uninstaller); this file
//! only wires them.

mod commands;
mod disk;
mod error;
mod migrate;
mod registry;
mod settings;
mod shell;
mod tray;
mod uninstall;

use std::sync::Arc;

use shell::Shell;
use tauri::{Manager, WindowEvent};
use wrc_core::autostart::BACKGROUND_FLAG;
use wrc_core::watch::Watcher;

/// Run a scheduled registry job headless (`--run-job <id>`), with a
/// notification saying how it went, and return the exit code: 0 done, 1
/// the job reported a problem, 2 it could not run.
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

pub fn run() {
    let started_hidden = std::env::args().any(|arg| arg == BACKGROUND_FLAG);
    let app = tauri::Builder::default()
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
        .setup(move |app| start(app, started_hidden))
        .on_menu_event(registry::menu::on_menu_event)
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
    app.run(|handle, event| {
        if let tauri::RunEvent::Exit = event
            && let Some(state) = handle.try_state::<Arc<registry::state::AppState>>()
        {
            state.watcher.stop_all();
        }
    });
}

/// Find the data folder, open each module's machine and state, start the
/// watcher and the tray icon.
fn start(app: &mut tauri::App, started_hidden: bool) -> Result<(), Box<dyn std::error::Error>> {
    let root = shell::root()?;
    std::fs::create_dir_all(&root)?;
    // The WebDriver suite sets its own data folder and must never adopt
    // this PC's data.
    if std::env::var_os("WATTCLEAN_DATA_DIR").is_none() {
        let adopted = migrate::adopt(&shell::local_app_data()?, &root);
        for problem in &adopted.problems {
            eprintln!("Could not copy from an earlier app: {problem}");
        }
    }

    let disk_dir = root.join("disk");
    std::fs::create_dir_all(&disk_dir)?;
    let (disk_system, fake) = disk::platform::machine(&disk_dir)?;
    let disk = Arc::new(disk::state::AppState::new(disk_system, disk_dir, fake));
    let tidy = Arc::clone(&disk);
    std::thread::spawn(move || disk::remove_wipe_leftovers(&tidy));

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
        &root,
        fake,
    ));

    app.manage(Arc::new(Shell::new(root, started_hidden, fake)));
    app.manage(disk);
    app.manage(registry);
    app.manage(uninstall);
    tray::build(app.handle())?;
    #[cfg(windows)]
    if let Err(error) = registry::platform::accept_second_launch(&app.config().identifier) {
        eprintln!("A second launch may not reach this window: {error}");
    }
    Ok(())
}
