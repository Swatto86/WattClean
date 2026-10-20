//! The notification-area icon, present while the app runs. A left click
//! opens the window (on macOS, where a menu-bar icon's click shows its
//! menu, the menu's first item does); its menu opens a module's page or
//! quits. Closing the window hides it here when "keep running" is on, so
//! the Registry Watcher goes on watching.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
#[cfg(not(target_os = "macos"))]
use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

const TRAY: &str = "main";

/// The page each menu item opens, as the window's navigation names it.
#[cfg(windows)]
const PAGES: &[(&str, &str, &str)] = &[
    ("tray-cleaner", "&Disk cleaner", "cleaner"),
    ("tray-registry", "&Registry cleaner", "registry"),
    ("tray-programs", "&Uninstall programs", "programs"),
];

/// Linux and macOS have no registry.
#[cfg(not(windows))]
const PAGES: &[(&str, &str, &str)] = &[
    ("tray-cleaner", "&Disk cleaner", "cleaner"),
    ("tray-programs", "&Uninstall programs", "programs"),
];

/// Show and focus the main window.
pub fn show_main(app: &AppHandle) {
    // A window left in full screen was hidden with the whole app.
    #[cfg(target_os = "macos")]
    let _ = app.show();
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Hide the main window. On macOS a window in full screen goes by hiding
/// the whole app, as Cmd+H does, which leaves its full-screen Space:
/// hiding the window alone would leave the user on an empty black Space.
pub fn hide_main(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    #[cfg(target_os = "macos")]
    if window.is_fullscreen().unwrap_or(false) {
        let _ = app.hide();
        return;
    }
    let _ = window.hide();
}

/// Show the window on one page.
pub fn show_page(app: &AppHandle, page: &str) {
    show_main(app);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.emit("open-page", page);
    }
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "tray-open", "&Open WattClean", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "tray-quit", "&Quit WattClean", true, None::<&str>)?;
    let pages = PAGES
        .iter()
        .map(|(id, label, _)| MenuItem::with_id(app, *id, *label, true, None::<&str>))
        .collect::<tauri::Result<Vec<_>>>()?;
    let lower = PredefinedMenuItem::separator(app)?;
    let mut items: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = vec![&open, &separator];
    items.extend(
        pages
            .iter()
            .map(|item| item as &dyn tauri::menu::IsMenuItem<tauri::Wry>),
    );
    items.push(&lower);
    items.push(&quit);
    let menu = Menu::with_items(app, &items)?;
    let mut tray = TrayIconBuilder::with_id(TRAY)
        .tooltip("WattClean")
        .menu(&menu)
        // A click on a menu-bar icon shows its menu on macOS; elsewhere a
        // left click opens the window and the menu is a right-click away.
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(|app, event| {
            let id = event.id().as_ref();
            match id {
                "tray-open" => show_main(app),
                // Waits for a running job off the main thread (as macOS's
                // Quit menu item does); the Exit event then stops the watcher.
                "tray-quit" => crate::quit::quit_when_idle(app),
                _ => {
                    if let Some((_, _, page)) = PAGES.iter().find(|(item, _, _)| *item == id) {
                        show_page(app, page);
                    }
                }
            }
        });
    #[cfg(not(target_os = "macos"))]
    {
        tray = tray.on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    }
    // The menu bar shows a monochrome glyph it tints for light and dark
    // appearance; elsewhere the app's own icon.
    #[cfg(target_os = "macos")]
    {
        tray = tray
            .icon(tauri::include_image!("icons/tray-template.png"))
            .icon_as_template(true);
    }
    #[cfg(not(target_os = "macos"))]
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}
