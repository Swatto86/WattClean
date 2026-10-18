//! Right-click menus are Windows' own popup menus, never the browser's. The
//! page describes the items; this shows them natively at the pointer and
//! answers with the item chosen. Cut, Copy, Paste and Select all are the
//! native edit items, which act on the focused field by themselves.

use std::sync::Arc;
use std::sync::mpsc::channel;
use std::time::Duration;

use serde::Deserialize;
use tauri::menu::{IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::{AppHandle, LogicalPosition, Manager, State, Window};

use crate::error::{CommandError, CommandResult, blocking};
use crate::registry::state::AppState;

/// Menu item ids are `ctx:<menu>:<item>`, so a late answer from an earlier
/// menu is never taken for this one.
const PREFIX: &str = "ctx:";
const MAX_ENTRIES: usize = 40;
/// After the menu closes, its chosen item is a queued event away.
const ANSWER_WAIT: Duration = Duration::from_millis(400);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", deny_unknown_fields)]
pub enum Entry {
    Item {
        id: String,
        text: String,
        #[serde(default = "enabled")]
        enabled: bool,
    },
    Separator,
    Cut {
        enabled: bool,
    },
    Copy {
        enabled: bool,
    },
    Paste {
        enabled: bool,
    },
    SelectAll {
        enabled: bool,
    },
}

fn enabled() -> bool {
    true
}

impl Entry {
    fn check(&self) -> CommandResult<()> {
        if let Self::Item { id, text, .. } = self {
            let id_ok = !id.is_empty()
                && id.len() <= 64
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
            if !id_ok || text.is_empty() || text.chars().count() > 120 {
                return Err("A menu item is malformed.".into());
            }
        }
        Ok(())
    }
}

/// Every menu event: hand a context-menu choice to the menu waiting for it.
pub fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    let Some(rest) = event.id().0.strip_prefix(PREFIX) else {
        return;
    };
    let Some((menu, item)) = rest.split_once(':') else {
        return;
    };
    if let (Ok(menu), Some(state)) = (menu.parse(), app.try_state::<Arc<AppState>>()) {
        state.menu_chosen(menu, item);
    }
}

/// Show a native menu at `x`, `y` in the page and return the id of the item
/// chosen, or `None` when the menu was dismissed or a native edit item
/// handled the choice itself.
#[tauri::command]
pub async fn reg_context_menu(
    window: Window,
    state: State<'_, Arc<AppState>>,
    entries: Vec<Entry>,
    x: f64,
    y: f64,
) -> CommandResult<Option<String>> {
    if entries.is_empty() || entries.len() > MAX_ENTRIES {
        return Err("A menu needs between 1 and 40 items.".into());
    }
    for entry in &entries {
        entry.check()?;
    }
    let state = Arc::clone(&state);
    blocking(move || {
        let (sender, answers) = channel();
        let menu_id = state.open_menu(sender);
        let shown = show(&window, menu_id, &entries, x, y);
        // The chosen item's event was queued as the menu closed; wait for it.
        // Nothing arriving in time means the menu was dismissed.
        let answer = answers.recv_timeout(ANSWER_WAIT).ok();
        state.close_menu(menu_id);
        shown?;
        Ok(answer)
    })
    .await
}

fn show(window: &Window, menu_id: u64, entries: &[Entry], x: f64, y: f64) -> CommandResult<()> {
    let app = window.app_handle();
    let menu = Menu::new(app)?;
    for entry in entries {
        menu.append(item(app, menu_id, entry)?.as_ref())?;
    }
    // Returns once the menu has closed.
    window.popup_menu_at(&menu, LogicalPosition::new(x.max(0.0), y.max(0.0)))?;
    Ok(())
}

fn item(
    app: &AppHandle,
    menu_id: u64,
    entry: &Entry,
) -> Result<Box<dyn IsMenuItem<tauri::Wry>>, CommandError> {
    let edit = |enabled: bool,
                native: fn(&AppHandle) -> tauri::Result<PredefinedMenuItem<tauri::Wry>>,
                text: &str,
                keys: &str|
     -> tauri::Result<Box<dyn IsMenuItem<tauri::Wry>>> {
        if enabled {
            Ok(Box::new(native(app)?))
        } else {
            // Windows' own label and shortcut, greyed, as it shows them.
            Ok(Box::new(MenuItem::new(app, text, false, Some(keys))?))
        }
    };
    Ok(match entry {
        Entry::Item { id, text, enabled } => Box::new(MenuItem::with_id(
            app,
            format!("{PREFIX}{menu_id}:{id}"),
            text,
            *enabled,
            None::<&str>,
        )?),
        Entry::Separator => Box::new(PredefinedMenuItem::separator(app)?),
        Entry::Cut { enabled } => edit(
            *enabled,
            |app| PredefinedMenuItem::cut(app, None),
            "Cu&t",
            "CmdOrCtrl+X",
        )?,
        Entry::Copy { enabled } => edit(
            *enabled,
            |app| PredefinedMenuItem::copy(app, None),
            "&Copy",
            "CmdOrCtrl+C",
        )?,
        Entry::Paste { enabled } => edit(
            *enabled,
            |app| PredefinedMenuItem::paste(app, None),
            "&Paste",
            "CmdOrCtrl+V",
        )?,
        Entry::SelectAll { enabled } => edit(
            *enabled,
            |app| PredefinedMenuItem::select_all(app, None),
            "Select &All",
            "CmdOrCtrl+A",
        )?,
    })
}
