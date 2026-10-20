//! The menu bar's menus on macOS: Tauri's default ones (File, Edit, View,
//! Window, Help) under an app menu of WattClean's own. Settings… (Cmd+,)
//! opens the Settings page, and Quit (Cmd+Q) is the menu-bar icon's Quit:
//! it hides the window and waits for a running job on a worker thread.
//! AppKit's own Quit would wait in the Exit event on the main thread
//! instead, with the window frozen until a removal ended.

use tauri::AppHandle;
use tauri::menu::{AboutMetadata, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};

const SETTINGS: &str = "app-settings";
const QUIT: &str = "app-quit";

/// Tauri's default menus, the app menu replaced.
pub fn build(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::default(app)?;
    // The default's first menu is its app menu.
    menu.remove_at(0)?;
    menu.prepend(&app_menu(app)?)?;
    Ok(menu)
}

/// About, Settings…, Services, Hide, Hide Others, Show All and Quit, as
/// macOS apps lay it out.
fn app_menu(app: &AppHandle) -> tauri::Result<Submenu<tauri::Wry>> {
    let info = app.package_info();
    let bundle = &app.config().bundle;
    let about = AboutMetadata {
        name: Some(info.name.clone()),
        version: Some(info.version.to_string()),
        copyright: bundle.copyright.clone(),
        authors: bundle.publisher.clone().map(|publisher| vec![publisher]),
        ..Default::default()
    };
    Submenu::with_items(
        app,
        &info.name,
        true,
        &[
            &PredefinedMenuItem::about(app, None, Some(about))?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, SETTINGS, "Settings…", true, Some("CmdOrCtrl+,"))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::services(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, None)?,
            &PredefinedMenuItem::hide_others(app, None)?,
            &PredefinedMenuItem::show_all(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(
                app,
                QUIT,
                format!("Quit {}", info.name),
                true,
                Some("CmdOrCtrl+Q"),
            )?,
        ],
    )
}

/// Settings… and Quit; the other items are AppKit's own.
pub fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    match event.id().as_ref() {
        SETTINGS => crate::tray::show_page(app, "settings"),
        QUIT => crate::quit::quit_when_idle(app),
        _ => {}
    }
}
