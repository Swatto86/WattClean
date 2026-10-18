//! Windows' own activity that happens during any install and belongs to no
//! program: temp files, caches, Restart Manager sessions, compatibility and
//! usage records. None of it is kept in an install log.

use std::path::Path;

use crate::folders::Folders;
use crate::model::Hive;
use crate::paths::is_within;
use crate::scan::Target;

/// Registry subtrees written by Windows components, not by installers.
const KEYS: &[(Hive, &str)] = &[
    (Hive::CurrentUser, r"Software\Microsoft\RestartManager"),
    (
        Hive::CurrentUser,
        r"Software\Microsoft\Windows\CurrentVersion\Internet Settings",
    ),
    (
        Hive::CurrentUser,
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\FeatureUsage",
    ),
    (
        Hive::CurrentUser,
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\RecentDocs",
    ),
    (
        Hive::CurrentUser,
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\UserAssist",
    ),
    (
        Hive::CurrentUser,
        r"Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags",
    ),
    (
        Hive::CurrentUser,
        r"Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\BagMRU",
    ),
    (
        Hive::CurrentUser,
        r"Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\Bags",
    ),
    (Hive::LocalMachine, r"SYSTEM\Software\Microsoft\TIP"),
    (Hive::LocalMachine, r"SYSTEM\ControlSet001\Services\bam"),
    (Hive::LocalMachine, r"SYSTEM\CurrentControlSet\Services\bam"),
    (Hive::LocalMachine, r"SOFTWARE\Microsoft\Tracing"),
    (Hive::LocalMachine, r"SOFTWARE\Microsoft\Cryptography\RNG"),
    (
        Hive::LocalMachine,
        r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\AppCompatFlags",
    ),
    (
        Hive::LocalMachine,
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartPage",
    ),
    (
        Hive::LocalMachine,
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Explorer\StartPage",
    ),
    (
        Hive::LocalMachine,
        r"SOFTWARE\Microsoft\Windows\Windows Error Reporting",
    ),
];

/// Keys whose own values Windows rewrites (PendingFileRenameOperations);
/// their subkeys, such as Session Manager\Environment, are not noise.
const VALUES_OF: &[&str] = &[
    r"SYSTEM\ControlSet001\Control\Session Manager",
    r"SYSTEM\CurrentControlSet\Control\Session Manager",
];

pub fn is_noise(target: &Target, folders: &Folders) -> bool {
    match target {
        Target::Value { key, .. }
            if key.hive == Hive::LocalMachine
                && VALUES_OF
                    .iter()
                    .any(|path| key.path.eq_ignore_ascii_case(path)) =>
        {
            true
        }
        Target::Key { key } | Target::Value { key, .. } => KEYS
            .iter()
            .any(|(hive, path)| key.is_within(&crate::RegPath::new(*hive, path))),
        Target::Folder { path } | Target::File { path } => noisy_path(path, folders),
    }
}

fn noisy_path(path: &Path, folders: &Folders) -> bool {
    let places = [
        folders.temp.clone(),
        folders.windows.join("Temp"),
        folders.windows.join("Prefetch"),
        folders.windows.join("Logs"),
        folders.windows.join("SoftwareDistribution"),
        folders
            .windows
            .join(r"ServiceProfiles\LocalService\AppData\Local\FontCache"),
        folders.program_data.join(r"Microsoft\Windows\WER"),
        folders.local_app_data.join(r"Microsoft\Windows\INetCache"),
        folders.local_app_data.join(r"Microsoft\Windows\WebCache"),
        folders.local_app_data.join(r"Microsoft\Windows\Explorer"),
        folders.local_app_data.join(r"Microsoft\CLR_v4.0\UsageLogs"),
        folders
            .local_app_data
            .join(r"Microsoft\CLR_v4.0_32\UsageLogs"),
        folders.app_data.join(r"Microsoft\Windows\Recent"),
    ];
    let system_folders = ["$recycle.bin", "system volume information"];
    let lower = path.to_string_lossy().to_lowercase();
    places.iter().any(|place| is_within(path, place))
        || system_folders.iter().any(|name| lower.contains(name))
}
