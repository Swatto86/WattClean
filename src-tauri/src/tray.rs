//! The notification-area icon, present while the app runs. A left click
//! opens the window; its menu opens a module's page or quits. Closing the
//! window hides it here when "keep running" is on, so the Registry Watcher
//! goes on watching.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

const TRAY: &str = "main";

/// The page each menu item opens, as the window's navigation names it.
const PAGES: &[(&str, &str, &str)] = &[
    ("tray-cleaner", "&Disk cleaner", "cleaner"),
    ("tray-registry", "&Registry cleaner", "registry"),
    ("tray-programs", "&Uninstall programs", "programs"),
];

/// Show and focus the main window.
pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Show the window on one page.
fn show_page(app: &AppHandle, page: &str) {
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
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let id = event.id().as_ref();
            match id {
                "tray-open" => show_main(app),
                // The one way out that waits for a running wipe to stop;
                // the Exit event then stops the watcher.
                "tray-quit" => crate::shell::quit_when_idle(app),
                _ => {
                    if let Some((_, _, page)) = PAGES.iter().find(|(item, _, _)| *item == id) {
                        show_page(app, page);
                    }
                }
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}
