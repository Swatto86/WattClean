//! The machine's well-known folders, read from the registry the way Explorer
//! resolves them, so a Desktop moved to OneDrive or a second drive is found.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::model::{Hive, RegPath};
use crate::paths::expand;
use crate::system::{System, text_value, values_or_empty};
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Folders {
    pub windows: PathBuf,
    pub program_files: PathBuf,
    pub program_files_x86: Option<PathBuf>,
    pub common_files: Vec<PathBuf>,
    pub program_data: PathBuf,
    pub users: PathBuf,
    pub user_profile: PathBuf,
    pub app_data: PathBuf,
    pub local_app_data: PathBuf,
    pub local_low: PathBuf,
    pub temp: PathBuf,
    pub desktop: PathBuf,
    pub public_desktop: PathBuf,
    pub start_menu_programs: PathBuf,
    pub common_start_menu_programs: PathBuf,
    pub documents: PathBuf,
    pub downloads: Option<PathBuf>,
}

const CURRENT_VERSION: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion";
const NT_CURRENT_VERSION: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
const USER_SHELL_FOLDERS: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders";
const DOWNLOADS_ID: &str = "{374DE290-123F-4565-9164-39C4925E467B}";

impl Folders {
    pub fn load(system: &dyn System) -> Result<Self> {
        let machine = values_or_empty(system, &RegPath::new(Hive::LocalMachine, CURRENT_VERSION));
        let nt = values_or_empty(
            system,
            &RegPath::new(Hive::LocalMachine, NT_CURRENT_VERSION),
        );
        let user_shell =
            values_or_empty(system, &RegPath::new(Hive::CurrentUser, USER_SHELL_FOLDERS));
        // The machine-wide twin holds the "Common …" folders.
        let common_shell = values_or_empty(
            system,
            &RegPath::new(Hive::LocalMachine, USER_SHELL_FOLDERS),
        );
        let resolve = |values: &[crate::RawValue], name: &str, env: &str| -> Option<PathBuf> {
            text_value(values, name)
                .or_else(|| system.env_var(env))
                .map(|text| PathBuf::from(expand(system, &text)))
        };
        let need = |path: Option<PathBuf>, what: &str| {
            path.ok_or_else(|| Error::Invalid(format!("Windows does not report the {what} folder")))
        };

        let windows = need(resolve(&nt, "SystemRoot", "SystemRoot"), "Windows")?;
        let program_files = need(
            resolve(&machine, "ProgramFilesDir", "ProgramFiles"),
            "Program Files",
        )?;
        let program_files_x86 = resolve(&machine, "ProgramFilesDir (x86)", "ProgramFiles(x86)")
            .filter(|path| !crate::paths::same(path, &program_files));
        let common_files = [
            resolve(&machine, "CommonFilesDir", "CommonProgramFiles"),
            resolve(&machine, "CommonFilesDir (x86)", "CommonProgramFiles(x86)"),
        ]
        .into_iter()
        .flatten()
        .collect();
        let program_data = need(
            resolve(&common_shell, "Common AppData", "ProgramData"),
            "ProgramData",
        )?;
        let user_profile = need(
            system.env_var("USERPROFILE").map(PathBuf::from),
            "user profile",
        )?;
        let users = user_profile
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| user_profile.clone());
        let app_data = need(resolve(&user_shell, "AppData", "APPDATA"), "AppData")?;
        let local_app_data = need(
            resolve(&user_shell, "Local AppData", "LOCALAPPDATA"),
            "Local AppData",
        )?;
        let local_low = user_profile.join(r"AppData\LocalLow");
        let temp = system
            .env_var("TEMP")
            .map(PathBuf::from)
            .unwrap_or_else(|| local_app_data.join("Temp"));
        let desktop =
            resolve(&user_shell, "Desktop", "").unwrap_or_else(|| user_profile.join("Desktop"));
        let public_desktop = resolve(&common_shell, "Common Desktop", "")
            .or_else(|| {
                system
                    .env_var("PUBLIC")
                    .map(|public| PathBuf::from(public).join("Desktop"))
            })
            .unwrap_or_else(|| users.join(r"Public\Desktop"));
        let start_menu_programs = resolve(&user_shell, "Programs", "")
            .unwrap_or_else(|| app_data.join(r"Microsoft\Windows\Start Menu\Programs"));
        let common_start_menu_programs = resolve(&common_shell, "Common Programs", "")
            .unwrap_or_else(|| program_data.join(r"Microsoft\Windows\Start Menu\Programs"));
        let documents =
            resolve(&user_shell, "Personal", "").unwrap_or_else(|| user_profile.join("Documents"));
        let downloads = resolve(&user_shell, DOWNLOADS_ID, "");

        Ok(Self {
            windows,
            program_files,
            program_files_x86,
            common_files,
            program_data,
            users,
            user_profile,
            app_data,
            local_app_data,
            local_low,
            temp,
            desktop,
            public_desktop,
            start_menu_programs,
            common_start_menu_programs,
            documents,
            downloads,
        })
    }

    /// Folders whose direct children are programs' own folders.
    pub fn program_roots(&self) -> Vec<&Path> {
        let mut roots = vec![self.program_files.as_path()];
        roots.extend(self.program_files_x86.as_deref());
        roots.extend([
            self.program_data.as_path(),
            self.app_data.as_path(),
            self.local_app_data.as_path(),
            self.local_low.as_path(),
        ]);
        roots
    }

    pub fn start_menus(&self) -> [&Path; 2] {
        [&self.start_menu_programs, &self.common_start_menu_programs]
    }

    pub fn desktops(&self) -> [&Path; 2] {
        [&self.desktop, &self.public_desktop]
    }

    pub fn quick_launch(&self) -> PathBuf {
        self.app_data
            .join(r"Microsoft\Internet Explorer\Quick Launch")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeSystem;

    #[test]
    fn reads_folders_from_the_registry_and_expands_them() {
        let dir = tempfile::tempdir().unwrap();
        let system = FakeSystem::seeded(dir.path()).unwrap();
        let folders = Folders::load(&system).unwrap();
        let root = dir.path();
        assert_eq!(folders.program_files, root.join("Program Files"));
        assert_eq!(
            folders.program_files_x86,
            Some(root.join("Program Files (x86)"))
        );
        assert_eq!(folders.app_data, root.join(r"Users\tester\AppData\Roaming"));
        assert_eq!(folders.desktop, root.join(r"Users\tester\OneDrive\Desktop"));
        assert_eq!(
            folders.local_low,
            root.join(r"Users\tester\AppData\LocalLow")
        );
        assert_eq!(folders.users, root.join("Users"));
        assert_eq!(folders.common_files.len(), 2);
    }

    #[test]
    fn a_missing_windows_folder_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let system = FakeSystem::empty(dir.path());
        let error = Folders::load(&system).unwrap_err();
        assert!(error.to_string().contains("Windows"), "{error}");
    }
}
