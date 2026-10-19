//! Windows' own records about programs and files: caches, history lists,
//! sounds, cursors and fonts.

use super::{Category, Context, Found};
use crate::model::{Hive, RegPath};
use crate::paths::command_target;
use crate::system::{subkeys_or_empty, text_value, values_or_empty};

const MUI_CACHE: &str = r"Local Settings\Software\Microsoft\Windows\Shell\MuiCache";
const APP_COMPAT: &str = r"Microsoft\Windows NT\CurrentVersion\AppCompatFlags";

/// Program names (MuiCache) and compatibility records (AppCompatFlags) for
/// programs that no longer exist.
pub fn cache(context: &Context, out: &mut Vec<Found>) {
    for user in &context.users {
        let key = user.classes.child(MUI_CACHE);
        for value in values_or_empty(context.system, &key) {
            let path = mui_path(&value.name);
            if context.check_for(&key).missing(&path, false) {
                out.push(Found::value(
                    Category::Cache,
                    key.clone(),
                    &value.name,
                    format!("Cached name of {path}, which no longer exists."),
                ));
            }
        }
    }
    let mut compat: Vec<RegPath> = vec![RegPath::new(
        Hive::LocalMachine,
        &format!(r"SOFTWARE\{APP_COMPAT}\Layers"),
    )];
    for user in &context.users {
        let flags = user.software.child(APP_COMPAT);
        compat.push(flags.child("Layers"));
        compat.push(flags.child(r"Compatibility Assistant\Store"));
        compat.push(flags.child(r"Compatibility Assistant\Persisted"));
    }
    for key in compat {
        for value in values_or_empty(context.system, &key) {
            if context.check_for(&key).missing(&value.name, false) {
                out.push(Found::value(
                    Category::Cache,
                    key.clone(),
                    &value.name,
                    format!(
                        "Compatibility record for {}, which no longer exists.",
                        value.name
                    ),
                ));
            }
        }
    }
}

/// `C:\App\a.exe.FriendlyAppName` → `C:\App\a.exe`; `@C:\App\a.dll,-1` → `C:\App\a.dll`.
fn mui_path(name: &str) -> String {
    // ASCII folding keeps byte offsets, and the suffixes are ASCII, so the
    // cut falls on a character boundary.
    let lower = name.to_ascii_lowercase();
    for suffix in [".friendlyappname", ".applicationcompany"] {
        if lower.ends_with(suffix) {
            return name[..name.len() - suffix.len()].to_owned();
        }
    }
    command_target(name).unwrap_or_default()
}

/// Lists Windows keeps of what was recently opened, typed or searched.
const HISTORY: [(&str, &str); 10] = [
    (
        r"Microsoft\Windows\CurrentVersion\Explorer\RecentDocs",
        "Recently opened documents",
    ),
    (
        r"Microsoft\Windows\CurrentVersion\Explorer\RunMRU",
        "Commands typed in the Run box",
    ),
    (
        r"Microsoft\Windows\CurrentVersion\Explorer\TypedPaths",
        "Paths typed in File Explorer's address bar",
    ),
    (
        r"Microsoft\Windows\CurrentVersion\Explorer\WordWheelQuery",
        "File Explorer searches",
    ),
    (
        r"Microsoft\Windows\CurrentVersion\Explorer\ComDlg32\OpenSavePidlMRU",
        "Files picked in Open and Save dialogs",
    ),
    (
        r"Microsoft\Windows\CurrentVersion\Explorer\ComDlg32\LastVisitedPidlMRU",
        "Folders last used by Open and Save dialogs",
    ),
    (
        r"Microsoft\Windows\CurrentVersion\Explorer\ComDlg32\LastVisitedPidlMRULegacy",
        "Folders last used by older Open and Save dialogs",
    ),
    (
        r"Microsoft\Windows\CurrentVersion\Explorer\ComDlg32\OpenSaveMRU",
        "Files picked in older Open and Save dialogs",
    ),
    (
        r"Microsoft\Windows\CurrentVersion\Applets\Paint\Recent File List",
        "Paint's recent pictures",
    ),
    (
        r"Microsoft\Windows\CurrentVersion\Applets\Wordpad\Recent File List",
        "WordPad's recent documents",
    ),
];

/// Each history list that holds anything, offered whole.
pub fn history(context: &Context, out: &mut Vec<Found>) {
    for user in &context.users {
        for (path, what) in HISTORY {
            let key = user.software.child(path);
            let entries = values_or_empty(context.system, &key)
                .iter()
                .filter(|value| !value.name.is_empty())
                .count()
                + subkeys_or_empty(context.system, &key).len();
            if entries > 0 {
                out.push(Found::key(
                    Category::History,
                    key,
                    format!("{what}: {entries} entries."),
                ));
            }
        }
    }
}

/// Sound events, cursors and per-account fonts that name missing files.
/// The machine's font list is checked too, but its file names are inside
/// the Windows folder, so it is skipped while Windows paths are left alone.
pub fn system_settings(context: &Context, out: &mut Vec<Found>) {
    for user in &context.users {
        sounds(context, &user.root.child(r"AppEvents\Schemes\Apps"), out);
        let cursors = user.root.child(r"Control Panel\Cursors");
        for value in values_or_empty(context.system, &cursors) {
            if value.name.is_empty() || value.name.eq_ignore_ascii_case("Scheme Source") {
                continue;
            }
            let Some(path) = value.as_text() else {
                continue;
            };
            if context.check_for(&cursors).missing(&path, false) {
                out.push(Found::value(
                    Category::SystemSettings,
                    cursors.clone(),
                    &value.name,
                    format!(
                        "Cursor \"{}\" uses {path}, which no longer exists.",
                        value.name
                    ),
                ));
            }
        }
        fonts(
            context,
            &user
                .software
                .child(r"Microsoft\Windows NT\CurrentVersion\Fonts"),
            out,
        );
    }
    fonts(
        context,
        &RegPath::new(
            Hive::LocalMachine,
            r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts",
        ),
        out,
    );
}

fn sounds(context: &Context, apps: &RegPath, out: &mut Vec<Found>) {
    for app in subkeys_or_empty(context.system, apps) {
        let app_key = apps.child(&app);
        for event in subkeys_or_empty(context.system, &app_key) {
            let event_key = app_key.child(&event);
            for scheme in subkeys_or_empty(context.system, &event_key) {
                let key = event_key.child(&scheme);
                let values = values_or_empty(context.system, &key);
                let Some(path) = text_value(&values, "") else {
                    continue;
                };
                if context.check_for(&key).missing(&path, false) {
                    out.push(Found::value(
                        Category::SystemSettings,
                        key,
                        "",
                        format!("Sound for \"{event}\" plays {path}, which no longer exists."),
                    ));
                }
            }
        }
    }
}

fn fonts(context: &Context, key: &RegPath, out: &mut Vec<Found>) {
    let windows = context.system.env_var("SystemRoot");
    for value in values_or_empty(context.system, key) {
        let Some(file) = value.as_text() else {
            continue;
        };
        let path = if file.contains('\\') {
            file
        } else if let Some(windows) = &windows {
            format!(r"{windows}\Fonts\{file}")
        } else {
            continue;
        };
        if context.check_for(key).missing(&path, false) {
            out.push(Found::value(
                Category::SystemSettings,
                key.clone(),
                &value.name,
                format!(
                    "Font \"{}\" is in {path}, which no longer exists.",
                    value.name
                ),
            ));
        }
    }
}

#[cfg(test)]
#[cfg(windows)]
mod tests {
    use super::mui_path;

    #[test]
    fn mui_cache_names_give_their_file() {
        assert_eq!(mui_path(r"C:\A\a.exe.FriendlyAppName"), r"C:\A\a.exe");
        assert_eq!(mui_path(r"C:\A\a.exe.applicationcompany"), r"C:\A\a.exe");
        assert_eq!(mui_path(r"@C:\A\b.dll,-101"), r"C:\A\b.dll");
        assert_eq!(mui_path("LangID"), "LangID");
    }
}
