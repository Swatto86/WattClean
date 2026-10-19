//! Registry keys and values a scan may never offer: Windows' own trees,
//! the roots every program shares, and other installed programs' keys. A
//! key in another account's hive is judged as its `HKCU` twin would be.

use super::Context;
use crate::model::{Hive, RegPath};

/// Keys that are never deleted, nor any key above them.
const PROTECTED_KEYS: &[&str] = &[
    "software",
    "software\\wow6432node",
    "software\\classes",
    "software\\wow6432node\\classes",
    "software\\classes\\wow6432node",
    "software\\classes\\clsid",
    "software\\classes\\wow6432node\\clsid",
    "software\\wow6432node\\classes\\clsid",
    "software\\classes\\typelib",
    "software\\classes\\appid",
    "software\\classes\\interface",
    "software\\classes\\applications",
    "software\\classes\\installer\\products",
    "software\\classes\\installer\\features",
    "software\\classes\\installer\\upgradecodes",
    "software\\classes\\installer\\dependencies",
    "software\\classes\\local settings",
    "software\\microsoft",
    "software\\wow6432node\\microsoft",
    "software\\policies",
    "software\\wow6432node\\policies",
    "software\\clients",
    "software\\registeredapplications",
    "software\\wow6432node\\registeredapplications",
    "system\\currentcontrolset\\services",
    "system\\currentcontrolset\\services\\eventlog",
];

/// Subtrees never deleted at any depth: Windows' own scheduled tasks.
const PROTECTED_SUBTREES: &[&str] =
    &["software\\microsoft\\windows nt\\currentversion\\schedule\\taskcache\\tree\\microsoft"];

const TASK_CACHE: &str = "software\\microsoft\\windows nt\\currentversion\\schedule\\taskcache";

/// Inside Microsoft's own trees, only these registrations name a program.
const MICROSOFT_REGISTRATIONS: &[&str] = &[
    "software\\microsoft\\windows\\currentversion\\uninstall",
    "software\\wow6432node\\microsoft\\windows\\currentversion\\uninstall",
    "software\\microsoft\\windows\\currentversion\\app paths",
    "software\\wow6432node\\microsoft\\windows\\currentversion\\app paths",
    "software\\microsoft\\windows\\currentversion\\installer\\userdata\\s-1-5-18\\products",
    "software\\microsoft\\installer\\products",
    "software\\microsoft\\installer\\features",
    "software\\microsoft\\windows\\currentversion\\app management\\arpcache",
    "software\\wow6432node\\microsoft\\windows\\currentversion\\app management\\arpcache",
    "software\\microsoft\\windows nt\\currentversion\\schedule\\taskcache\\tasks",
    "software\\microsoft\\windows nt\\currentversion\\schedule\\taskcache\\tree",
    "software\\microsoft\\windows nt\\currentversion\\schedule\\taskcache\\boot",
    "software\\microsoft\\windows nt\\currentversion\\schedule\\taskcache\\logon",
    "software\\microsoft\\windows nt\\currentversion\\schedule\\taskcache\\plain",
    "software\\microsoft\\windows nt\\currentversion\\schedule\\taskcache\\maintenance",
];

/// Class names Windows itself relies on: the shell's own, and the file types
/// and URL protocols a program may take over but never owns.
const SHELL_CLASSES: &[&str] = &[
    "*",
    "allfilesystemobjects",
    "directory",
    "drive",
    "folder",
    "exefile",
    "dllfile",
    "txtfile",
    "htmlfile",
    "mhtmlfile",
    "xmlfile",
    "http",
    "https",
    "ftp",
    "file",
    "mailto",
    "batfile",
    "cmdfile",
    "regfile",
    "inffile",
    "inifile",
    "jsfile",
    "jsefile",
    "vbsfile",
    "vbefile",
    "wsffile",
    "wshfile",
    "lnkfile",
    "piffile",
    "cplfile",
    "scrfile",
    "comfile",
    "sysfile",
    "drvfile",
    "cabfile",
    "chm.file",
    "microsoft-edge",
    "ms-settings",
    "msi.package",
];

/// Inside Microsoft's trees, the only value lists a deep search may clean:
/// those the dedicated scanners read (Wow6432Node folded away).
const MICROSOFT_VALUE_LISTS: &[&str] = &[
    "software\\microsoft\\windows\\currentversion\\run",
    "software\\microsoft\\windows\\currentversion\\runonce",
    "software\\microsoft\\windows\\currentversion\\sharedlls",
    "software\\microsoft\\windows\\currentversion\\installer\\folders",
    "software\\microsoft\\windows\\currentversion\\explorer\\startupapproved\\run",
    "software\\microsoft\\windows\\currentversion\\explorer\\startupapproved\\run32",
    "software\\microsoft\\windows\\currentversion\\shell extensions\\approved",
    "software\\microsoft\\windows nt\\currentversion\\fonts",
    "software\\microsoft\\windows nt\\currentversion\\appcompatflags\\layers",
    "software\\microsoft\\windows nt\\currentversion\\appcompatflags\\compatibility assistant\\store",
    "software\\classes\\local settings\\software\\microsoft\\windows\\shell\\muicache",
];

fn strictly_under(path: &str, base: &str) -> bool {
    path.len() > base.len() + 1 && path.starts_with(base) && path.as_bytes()[base.len()] == b'\\'
}

/// The path to judge, as if under HKLM/HKCU, and its depth. A key in another
/// account's hive maps to that account's `Software\…`; its classes hive to
/// `Software\Classes\…`. Hives addressed any other way are never touched.
fn effective(key: &RegPath) -> Option<(String, usize)> {
    match key.hive {
        Hive::LocalMachine | Hive::CurrentUser => Some((key.path.to_lowercase(), key.depth())),
        Hive::Users => {
            let (_, classes, rest) = key.account_part()?;
            if rest.is_empty() {
                return None;
            }
            let path = if classes {
                format!("software\\classes\\{}", rest.to_lowercase())
            } else {
                rest.to_lowercase()
            };
            let depth = path.split('\\').count();
            Some((path, depth))
        }
    }
}

pub fn key_allowed(context: &Context<'_>, key: &RegPath) -> bool {
    let Some((path, depth)) = effective(key) else {
        return false;
    };
    if depth < 2 {
        return false;
    }
    if PROTECTED_KEYS
        .iter()
        .any(|protected| *protected == path || strictly_under(protected, &path))
        || PROTECTED_SUBTREES
            .iter()
            .any(|subtree| *subtree == path || strictly_under(&path, subtree))
    {
        return false;
    }
    let microsoft = [
        "software\\microsoft",
        "software\\wow6432node\\microsoft",
        "software\\classes\\local settings",
    ];
    if microsoft.iter().any(|tree| strictly_under(&path, tree))
        && !MICROSOFT_REGISTRATIONS
            .iter()
            .any(|allowed| strictly_under(&path, allowed))
    {
        return false;
    }
    // Under SYSTEM only a service's own key, or an event-log source.
    let service = strictly_under(&path, "system\\currentcontrolset\\services") && depth == 4;
    let log_source =
        strictly_under(&path, "system\\currentcontrolset\\services\\eventlog") && depth == 6;
    if path.starts_with("system\\") && !service && !log_source {
        return false;
    }
    // `Clients` and its `<category>` lists are shared; one client's key
    // below is not.
    let clients = ["software\\clients", "software\\wow6432node\\clients"];
    if clients.iter().any(|base| {
        path.strip_prefix(base).is_some_and(|rest| {
            rest.is_empty() || (rest.starts_with('\\') && rest.matches('\\').count() == 1)
        })
    }) {
        return false;
    }
    // Task-cache entries are whole only at one task's own key.
    let task_entry = path.strip_prefix(TASK_CACHE).is_some_and(|rest| {
        let parts: Vec<&str> = rest.trim_start_matches('\\').split('\\').collect();
        matches!(
            parts.as_slice(),
            ["tasks" | "boot" | "logon" | "plain" | "maintenance", _]
        ) || (parts.first() == Some(&"tree") && parts.len() >= 2)
    });
    if path.starts_with(TASK_CACHE) && !task_entry {
        return false;
    }
    for classes in [
        "software\\classes",
        "software\\wow6432node\\classes",
        "software\\classes\\wow6432node",
    ] {
        if let Some(rest) = path
            .strip_prefix(classes)
            .and_then(|rest| rest.strip_prefix('\\'))
        {
            let class = rest.split('\\').next().unwrap_or_default();
            // `.ext` keys are shared by every program that opens the type.
            if class.starts_with('.') || (SHELL_CLASSES.contains(&class) && !is_shell_handler(rest))
            {
                return false;
            }
        }
    }
    !context
        .others
        .iter()
        .any(|other| other.key.is_within(key) || key.is_within(&other.key))
}

/// `*\shellex\ContextMenuHandlers\<name>` and the like: one extension's own
/// entry inside a shell class, never the class or its handler list.
fn is_shell_handler(rest: &str) -> bool {
    let parts: Vec<&str> = rest.split('\\').collect();
    matches!(
        parts.as_slice(),
        [_, "shellex", _, _] | [_, "background", "shellex", _, _]
    )
}

pub fn value_allowed(context: &Context<'_>, key: &RegPath) -> bool {
    let Some((path, _)) = effective(key) else {
        return false;
    };
    // Environment variables (PATH and the like) are shared by everything.
    let environment =
        path == "environment" || path.ends_with("\\control\\session manager\\environment");
    !environment
        && !path.contains("\\policies\\")
        && !context.others.iter().any(|other| key.is_within(&other.key))
}

/// Values a deep search may offer: those `value_allowed` accepts, outside
/// Microsoft's trees (Winlogon's Shell, Userinit and the like) except the
/// lists the scanners clean, and not inside another installed program's own
/// key under a Software root.
pub fn deep_value_allowed(context: &Context<'_>, key: &RegPath) -> bool {
    if !value_allowed(context, key) {
        return false;
    }
    let Some((path, _)) = effective(key) else {
        return false;
    };
    let path = path.replacen("software\\wow6432node\\", "software\\", 1);
    let microsoft = path.starts_with("software\\microsoft")
        || path.starts_with("software\\classes\\local settings");
    let listed = MICROSOFT_VALUE_LISTS.contains(&path.as_str())
        || strictly_under(
            &path,
            "software\\microsoft\\windows\\currentversion\\app paths",
        );
    if microsoft && !listed {
        return false;
    }
    !path
        .split('\\')
        .skip(1)
        .any(|part| context.belongs_to_other(part))
}
