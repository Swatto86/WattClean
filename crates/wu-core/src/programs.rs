//! The installed-programs list, read from the three uninstall registries
//! Windows' own "Installed apps" page reads.

use crate::model::{Hive, Program, RawValue, RegPath, Scope, find_value};
use crate::system::{System, subkeys_or_empty, text_value};

pub const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
pub const UNINSTALL_32: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";

pub fn uninstall_roots() -> [(RegPath, Scope); 3] {
    [
        (
            RegPath::new(Hive::LocalMachine, UNINSTALL),
            Scope::Machine64,
        ),
        (
            RegPath::new(Hive::LocalMachine, UNINSTALL_32),
            Scope::Machine32,
        ),
        (RegPath::new(Hive::CurrentUser, UNINSTALL), Scope::User),
    ]
}

/// Every removable program, sorted by name. Windows updates and child
/// entries of another product are never listed; system components only when
/// asked for. A key that cannot be read is skipped, not fatal.
pub fn list_programs(system: &dyn System, include_system_components: bool) -> Vec<Program> {
    let mut programs: Vec<Program> = uninstall_roots()
        .into_iter()
        .flat_map(|(root, scope)| {
            subkeys_or_empty(system, &root)
                .into_iter()
                .filter_map(move |name| read_program(system, root.child(&name), scope))
                .collect::<Vec<_>>()
        })
        .filter(|program| include_system_components || !program.system_component)
        .collect();
    programs.sort_by_cached_key(|program| (program.name.to_lowercase(), program.id.clone()));
    programs
}

/// Find one program by id among all entries, system components included.
pub fn find_program(system: &dyn System, id: &str) -> Option<Program> {
    list_programs(system, true)
        .into_iter()
        .find(|program| program.id.eq_ignore_ascii_case(id))
}

/// The listed program a forced uninstall is about: the one installed in
/// `folder`, else the one with exactly that name. Its own entry and folder
/// must then count as the target's, not as another program's to protect.
pub fn forced_target<'a>(
    system: &dyn System,
    folders: &crate::folders::Folders,
    programs: &'a [Program],
    name: &str,
    folder: Option<&std::path::Path>,
) -> Option<&'a Program> {
    use crate::identity::{Identity, name_key, product_name};
    let by_folder = folder.and_then(|folder| {
        programs.iter().find(|program| {
            Identity::of_entry(system, folders, program)
                .install_dirs
                .iter()
                .any(|dir| crate::paths::same(dir, folder))
        })
    });
    let key = name_key(name);
    by_folder.or_else(|| {
        programs.iter().find(|program| {
            name_key(&program.name) == key || name_key(&product_name(&program.name)) == key
        })
    })
}

pub fn read_program(system: &dyn System, key: RegPath, scope: Scope) -> Option<Program> {
    let values = system.values(&key).ok()?;
    let name = text_value(&values, "DisplayName")?;
    if text_value(&values, "ParentKeyName").is_some() || is_update(&values) {
        return None;
    }
    let msi_product_code = (dword(&values, "WindowsInstaller") == Some(1))
        .then(|| key.name().to_owned())
        .filter(|name| is_guid(name));
    let uninstall_command = text_value(&values, "UninstallString");
    if uninstall_command.is_none() && msi_product_code.is_none() {
        return None;
    }
    let install_date = text_value(&values, "InstallDate")
        .and_then(|text| registry_date(&text))
        .or_else(|| system.key_modified(&key).ok().map(date_from_unix));
    Some(Program {
        id: key.display(),
        version: text_value(&values, "DisplayVersion"),
        publisher: text_value(&values, "Publisher"),
        install_date,
        size_kb: dword(&values, "EstimatedSize")
            .filter(|&kb| kb > 0)
            .map(u64::from),
        install_location: text_value(&values, "InstallLocation")
            .map(|text| text.trim_matches('"').to_owned()),
        uninstall_command,
        quiet_uninstall_command: text_value(&values, "QuietUninstallString"),
        modify_command: text_value(&values, "ModifyPath"),
        icon: text_value(&values, "DisplayIcon"),
        website: text_value(&values, "URLInfoAbout").or_else(|| text_value(&values, "HelpLink")),
        comments: text_value(&values, "Comments"),
        system_component: dword(&values, "SystemComponent") == Some(1),
        bundle_cache: text_value(&values, "BundleCachePath"),
        bundle_provider: text_value(&values, "BundleProviderKey"),
        registry_key: key,
        scope,
        msi_product_code,
        name,
    })
}

fn dword(values: &[RawValue], name: &str) -> Option<u32> {
    find_value(values, name).and_then(RawValue::as_u32)
}

fn is_update(values: &[RawValue]) -> bool {
    text_value(values, "ReleaseType").is_some_and(|kind| {
        ["update", "hotfix", "security update", "service pack"]
            .contains(&kind.to_lowercase().as_str())
    })
}

/// `{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}`, as Windows Installer names products.
pub fn is_guid(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 38
        && bytes[0] == b'{'
        && bytes[37] == b'}'
        && bytes[1..37].iter().enumerate().all(|(at, byte)| {
            if [8, 13, 18, 23].contains(&at) {
                *byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

/// `YYYYMMDD` (what installers write) as `YYYY-MM-DD`; anything else is dropped.
fn registry_date(text: &str) -> Option<String> {
    let digits = text.trim();
    if digits.len() != 8 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let (year, rest) = digits.split_at(4);
    let (month, day) = rest.split_at(2);
    let month_ok = matches!(month.parse::<u8>(), Ok(1..=12));
    let day_ok = matches!(day.parse::<u8>(), Ok(1..=31));
    (month_ok && day_ok).then(|| format!("{year}-{month}-{day}"))
}

/// Civil date of a Unix timestamp (UTC), by Howard Hinnant's algorithm.
pub fn date_from_unix(seconds: u64) -> String {
    let days = i64::try_from(seconds / 86_400).unwrap_or(0);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeSystem;

    #[test]
    fn lists_seeded_programs_with_their_details() {
        let dir = tempfile::tempdir().unwrap();
        let system = FakeSystem::seeded(dir.path()).unwrap();
        let programs = list_programs(&system, false);
        let names: Vec<&str> = programs.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"Acme Widget 2.1 (x64)"), "{names:?}");
        assert!(
            !names.iter().any(|name| name.contains("Security Update")),
            "{names:?}"
        );
        assert!(
            !names.contains(&"Fabrikam Runtime Core"),
            "system component hidden"
        );

        let widget = programs
            .iter()
            .find(|p| p.name.starts_with("Acme Widget"))
            .unwrap();
        assert_eq!(widget.publisher.as_deref(), Some("Acme Software Ltd."));
        assert_eq!(widget.install_date.as_deref(), Some("2026-03-04"));
        assert_eq!(widget.size_kb, Some(20_480));
        assert_eq!(widget.scope, Scope::Machine64);

        let notes = programs.iter().find(|p| p.name == "Contoso Notes").unwrap();
        assert_eq!(notes.scope, Scope::User);
        let msi = programs
            .iter()
            .find(|p| p.name == "Fabrikam Viewer")
            .unwrap();
        assert_eq!(
            msi.msi_product_code.as_deref(),
            Some("{0A1B2C3D-1111-2222-3333-444455556666}")
        );
        assert_eq!(msi.scope, Scope::Machine32);

        let all = list_programs(&system, true);
        assert!(all.iter().any(|p| p.name == "Fabrikam Runtime Core"));
        assert!(find_program(&system, &widget.id.to_lowercase()).is_some());
    }

    #[test]
    fn a_forced_uninstall_finds_the_listed_program_by_folder_or_name() {
        let dir = tempfile::tempdir().unwrap();
        let system = FakeSystem::seeded(dir.path()).unwrap();
        let folders = crate::folders::Folders::load(&system).unwrap();
        let programs = list_programs(&system, true);
        let notes_dir = folders.local_app_data.join(r"Programs\Contoso Notes");
        let by_folder = forced_target(&system, &folders, &programs, "Anything", Some(&notes_dir));
        assert_eq!(by_folder.map(|p| p.name.as_str()), Some("Contoso Notes"));
        let by_name = forced_target(&system, &folders, &programs, "acme widget", None);
        assert_eq!(
            by_name.map(|p| p.name.as_str()),
            Some("Acme Widget 2.1 (x64)")
        );
        assert!(forced_target(&system, &folders, &programs, "Unknown Tool", None).is_none());
    }

    #[test]
    fn registry_dates_are_validated() {
        assert_eq!(registry_date("20260304").as_deref(), Some("2026-03-04"));
        assert_eq!(registry_date("20261304"), None);
        assert_eq!(registry_date("2026-03-04"), None);
        assert_eq!(registry_date("4/3/2026"), None);
    }

    #[test]
    fn unix_dates_convert_to_civil_dates() {
        assert_eq!(date_from_unix(0), "1970-01-01");
        assert_eq!(date_from_unix(951_782_400), "2000-02-29");
        assert_eq!(date_from_unix(1_791_936_000), "2026-10-14");
    }

    #[test]
    fn guids_are_recognised_strictly() {
        assert!(is_guid("{0A1B2C3D-1111-2222-3333-444455556666}"));
        assert!(!is_guid("0A1B2C3D-1111-2222-3333-444455556666"));
        assert!(!is_guid("{0A1B2C3D-1111-2222-3333-44445555666G}"));
        assert!(!is_guid("Acme Widget"));
    }
}
