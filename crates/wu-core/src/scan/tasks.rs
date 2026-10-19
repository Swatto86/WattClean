//! Moderate and up: scheduled tasks that run one of the program's files, and
//! event-log sources whose message files are its.
//!
//! A task lives in four places: its record under `TaskCache\Tasks\{id}`, its
//! name under `TaskCache\Tree`, an entry in `Boot`, `Logon`, `Plain` or
//! `Maintenance`, and its definition file under `System32\Tasks`. All are
//! offered together.

use super::{Confidence, Context, Found, Mode};
use crate::identity::Identity;
use crate::model::{Hive, RegPath};
use crate::paths::expand;
use crate::system::{key_exists, subkeys_or_empty, text_value, values_or_empty};

const TASK_CACHE: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Schedule\TaskCache";
const TRIGGER_LISTS: [&str; 4] = ["Boot", "Logon", "Plain", "Maintenance"];

pub(super) fn scan(identity: &Identity, mode: Mode, context: &Context<'_>, found: &mut Found) {
    if mode < Mode::Moderate || identity.install_dirs.is_empty() {
        return;
    }
    scheduled_tasks(identity, context, found);
    event_log_sources(identity, context, found);
}

fn scheduled_tasks(identity: &Identity, context: &Context<'_>, found: &mut Found) {
    let cache = RegPath::new(Hive::LocalMachine, TASK_CACHE);
    let tasks = cache.child("Tasks");
    for id in subkeys_or_empty(context.system, &tasks) {
        let key = tasks.child(&id);
        let values = values_or_empty(context.system, &key);
        let path = text_value(&values, "Path").unwrap_or_default();
        let file = context
            .folders
            .windows
            .join(r"System32\Tasks")
            .join(path.trim_start_matches('\\'));
        let actions = values
            .iter()
            .find(|value| value.name.eq_ignore_ascii_case("Actions"))
            .map(|value| utf16_fragments(&value.bytes))
            .unwrap_or_default();
        let runs_ours = actions.iter().any(|text| identity.references(text))
            || definition_mentions(&file, identity);
        if !runs_ours {
            continue;
        }
        let reason = "Scheduled task running one of its files";
        found.key(context, key, reason, Confidence::High);
        if !path.is_empty() {
            let tree = path
                .trim_start_matches('\\')
                .split('\\')
                .fold(cache.child("Tree"), |key, part| key.child(part));
            found.key(context, tree, reason, Confidence::High);
            found.path(context, identity, file, reason, Confidence::High);
        }
        for list in TRIGGER_LISTS {
            let entry = cache.child(list).child(&id);
            if key_exists(context.system, &entry) {
                found.key(context, entry, reason, Confidence::High);
            }
        }
    }
}

/// Runs of printable text in a UTF-16 blob, such as the command and
/// arguments inside a task's `Actions` value.
fn utf16_fragments(bytes: &[u8]) -> Vec<String> {
    let mut fragments = Vec::new();
    let mut current = String::new();
    for pair in bytes.as_chunks::<2>().0 {
        match char::from_u32(u32::from(u16::from_le_bytes(*pair))) {
            Some(ch) if !ch.is_control() => current.push(ch),
            _ => {
                if current.len() >= 4 {
                    fragments.push(std::mem::take(&mut current));
                } else {
                    current.clear();
                }
            }
        }
    }
    if current.len() >= 4 {
        fragments.push(current);
    }
    fragments
}

/// Does the task's definition file name one of the program's folders?
fn definition_mentions(file: &std::path::Path, identity: &Identity) -> bool {
    let Ok(bytes) = std::fs::read(file) else {
        return false;
    };
    let text = if bytes.starts_with(&[0xFF, 0xFE]) || bytes.get(1) == Some(&0) {
        utf16_fragments(&bytes).join("\n")
    } else {
        String::from_utf8_lossy(&bytes).into_owned()
    };
    text.lines()
        .flat_map(|line| line.split(['<', '>', '"']))
        .any(|part| identity.references(part))
}

fn event_log_sources(identity: &Identity, context: &Context<'_>, found: &mut Found) {
    let event_log = RegPath::new(
        Hive::LocalMachine,
        r"SYSTEM\CurrentControlSet\Services\EventLog",
    );
    for log in subkeys_or_empty(context.system, &event_log) {
        let log_key = event_log.child(&log);
        for source in subkeys_or_empty(context.system, &log_key) {
            let key = log_key.child(&source);
            let values = values_or_empty(context.system, &key);
            let ours = [
                "EventMessageFile",
                "CategoryMessageFile",
                "ParameterMessageFile",
            ]
            .into_iter()
            .filter_map(|name| text_value(&values, name))
            .any(|file| identity.references(&expand(context.system, &file)));
            if ours {
                found.key(
                    context,
                    key,
                    "Event log source using one of its files",
                    Confidence::High,
                );
            }
        }
    }
}

#[cfg(test)]
#[cfg(windows)]
mod tests {
    use super::*;

    #[test]
    fn utf16_blobs_yield_their_readable_runs() {
        let mut bytes = vec![3, 0, 0, 0];
        for unit in r"C:\A\b.exe".encode_utf16() {
            bytes.extend(unit.to_le_bytes());
        }
        bytes.extend([0, 0, 1, 0]);
        for unit in "--x".encode_utf16() {
            bytes.extend(unit.to_le_bytes());
        }
        assert_eq!(utf16_fragments(&bytes), vec![r"C:\A\b.exe".to_owned()]);
    }
}
