//! The notification-area icon, present while the app runs. A left click
//! opens the window on Windows; on macOS (a menu-bar icon's click shows
//! its menu) and Linux (where AppIndicator trays report no clicks, only
//! show the menu) the menu's first item does. The menu opens a module's
//! page or quits. Closing the window hides it here when "keep running" is
//! on and the desktop shows the icon, so the Registry Watcher goes on
//! watching.

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

/// The StatusNotifier host's bus name, which Linux's trays own (KDE,
/// Omarchy's bar, GNOME only with the AppIndicator extension).
#[cfg(target_os = "linux")]
const WATCHER: &str = "org.kde.StatusNotifierWatcher";

/// Whether the desktop shows the icon, so a hidden window can be found
/// again. A Wayland session shows it only through a StatusNotifier host;
/// an X11 one may also have an older tray of its own, which cannot be
/// asked. When it cannot be told, the icon is taken to show.
#[cfg(target_os = "linux")]
pub fn shown() -> bool {
    shown_on(std::env::var_os("WAYLAND_DISPLAY").is_some(), || {
        let bus = zbus::blocking::Connection::session().ok()?;
        let name = zbus::names::BusName::try_from(WATCHER).ok()?;
        zbus::blocking::fdo::DBusProxy::new(&bus)
            .ok()?
            .name_has_owner(name)
            .ok()
    })
}

/// Whether the icon shows on a Wayland session or not, given what the
/// session bus says about a StatusNotifier host (`None`: it could not be
/// asked).
#[cfg(any(test, target_os = "linux"))]
fn shown_on(wayland: bool, host: impl FnOnce() -> Option<bool>) -> bool {
    !wayland || host().unwrap_or(true)
}

/// Windows' notification area and macOS's menu bar are always there.
#[cfg(not(target_os = "linux"))]
pub fn shown() -> bool {
    true
}

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
        // A click on a menu-bar icon shows its menu on macOS; on Windows a
        // left click opens the window and the menu is a right-click away.
        // Linux's trays show the menu on any click.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// GNOME on Wayland without the AppIndicator extension shows no tray:
    /// the window must not hide there. X11 may have a tray the bus does
    /// not know of, and a bus that cannot be asked proves nothing.
    #[test]
    fn the_icon_is_taken_to_show_unless_a_wayland_session_has_no_tray() {
        assert!(!shown_on(true, || Some(false)));
        assert!(shown_on(true, || Some(true)));
        assert!(shown_on(true, || None));
        assert!(shown_on(false, || Some(false)));
    }
}
