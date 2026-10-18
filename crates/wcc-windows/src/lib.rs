//! The real Windows machine for WattClean's engine.

#[cfg(windows)]
mod drives;
#[cfg(windows)]
mod registry;
#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use windows_impl::*;

#[cfg(windows)]
mod windows_impl {
    use std::collections::HashSet;
    use std::io;
    use std::path::Path;

    use wcc_core::system::{Drive, Registry, System};
    use windows_sys::Win32::UI::Shell::{
        FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Favorites,
        FOLDERID_LocalAppDataLow, FOLDERID_Music, FOLDERID_Pictures, FOLDERID_StartMenu,
        FOLDERID_Videos,
    };

    use crate::registry::WindowsRegistry;
    use crate::win;

    pub use crate::win::{is_elevated, run_elevated};

    /// This PC, as the signed-in user sees it.
    pub struct WindowsSystem {
        registry: WindowsRegistry,
    }

    impl WindowsSystem {
        pub fn new() -> Self {
            Self {
                registry: WindowsRegistry,
            }
        }
    }

    impl Default for WindowsSystem {
        fn default() -> Self {
            Self::new()
        }
    }

    fn env(name: &str) -> Option<String> {
        std::env::var(name)
            .ok()
            .filter(|v| !v.trim().is_empty())
            .map(|v| win::long_path(&v))
    }

    fn first(names: &[&str]) -> Vec<String> {
        names.iter().find_map(|n| env(n)).into_iter().collect()
    }

    impl System for WindowsSystem {
        fn var(&self, name: &str) -> Vec<String> {
            let upper = name.to_ascii_uppercase();
            let folder = |id| win::known_folder(id).into_iter().collect::<Vec<_>>();
            let mut values = match upper.as_str() {
                "DOCUMENTS" => folder(&FOLDERID_Documents),
                "DESKTOP" => folder(&FOLDERID_Desktop),
                "DOWNLOADS" => folder(&FOLDERID_Downloads),
                "PICTURES" => folder(&FOLDERID_Pictures),
                "MUSIC" => folder(&FOLDERID_Music),
                "VIDEO" => folder(&FOLDERID_Videos),
                "FAVORITES" => folder(&FOLDERID_Favorites),
                "STARTMENU" => folder(&FOLDERID_StartMenu),
                "LOCALLOWAPPDATA" => folder(&FOLDERID_LocalAppDataLow),
                "COMMONAPPDATA" => first(&["PROGRAMDATA", "ALLUSERSPROFILE"]),
                "ONEDRIVE" => first(&["OneDrive", "OneDriveConsumer", "OneDriveCommercial"]),
                // Both Program Files folders, as winapp2.ini expects.
                "PROGRAMFILES" => {
                    let mut both = first(&["ProgramW6432", "ProgramFiles"]);
                    both.extend(env("ProgramFiles(x86)"));
                    both
                }
                "COMMONPROGRAMFILES" => {
                    let mut both = first(&["CommonProgramW6432", "CommonProgramFiles"]);
                    both.extend(env("CommonProgramFiles(x86)"));
                    both
                }
                _ => env(name).into_iter().collect(),
            };
            values.dedup();
            values
        }

        fn registry(&self) -> &dyn Registry {
            &self.registry
        }

        fn recycle_bin(&self) -> io::Result<(u64, u64)> {
            win::recycle_bin()
        }

        fn empty_recycle_bin(&self) -> io::Result<()> {
            win::empty_recycle_bin()
        }

        fn clipboard_has_data(&self) -> bool {
            win::clipboard_has_data()
        }

        fn clear_clipboard(&self) -> io::Result<()> {
            win::clear_clipboard()
        }

        fn running(&self) -> HashSet<String> {
            win::running_processes()
        }

        fn is_elevated(&self) -> bool {
            win::is_elevated()
        }

        fn drives(&self) -> Vec<Drive> {
            crate::drives::list()
        }

        fn free_space(&self, path: &Path) -> io::Result<u64> {
            win::free_space(path)
        }

        fn delete_on_reboot(&self, path: &Path) -> io::Result<()> {
            win::delete_on_reboot(path)
        }

        fn shell_changed(&self) {
            win::shell_changed();
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use wcc_core::system::System;

    #[test]
    fn variables_resolve_on_this_pc() {
        let system = WindowsSystem::new();
        let profile = system.var("USERPROFILE");
        assert_eq!(profile.len(), 1);
        assert!(std::path::Path::new(&profile[0]).is_dir());
        let local = system.var("localappdata");
        assert!(
            local[0]
                .to_lowercase()
                .starts_with(&profile[0].to_lowercase())
        );
        assert!(!system.var("DOCUMENTS").is_empty());
        assert!(!system.var("LOCALLOWAPPDATA").is_empty());
        assert!(
            system.var("PROGRAMFILES").len() >= 2,
            "both Program Files folders"
        );
        assert!(system.var("NO_SUCH_VARIABLE_HERE").is_empty());
        let temp = &system.var("TEMP")[0];
        assert!(!temp.contains('~'), "short names are expanded: {temp}");
    }

    #[test]
    fn existing_registry_keys_and_values_are_seen() {
        let system = WindowsSystem::new();
        let registry = system.registry();
        assert!(registry.key_exists(r"HKCU\Software"));
        assert!(registry.key_exists(r"HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft"));
        assert!(registry.value_exists(r"HKCU\Environment", "TEMP"));
        assert!(!registry.value_exists(r"HKCU\Environment", "NoSuchValue-WattClean"));
        assert!(!registry.key_exists(r"HKCU\Software\NoSuchKey-WattClean"));
    }

    #[test]
    fn machine_queries_answer() {
        let system = WindowsSystem::new();
        assert!(system.running().contains("explorer.exe") || !system.running().is_empty());
        assert!(system.recycle_bin().is_ok());
        let drives = system.drives();
        assert!(
            drives
                .iter()
                .any(|d| d.root.eq_ignore_ascii_case(r"C:\") && d.total > 0),
            "{drives:?}"
        );
        let _ = system.is_elevated();
    }

    /// Writes (then removes) its own scratch key in the real registry, so it
    /// runs only on request: `cargo test -p wcc-windows -- --ignored`.
    #[test]
    #[ignore = "writes a scratch key to the real registry"]
    fn the_registry_is_read_and_written_under_a_scratch_key() {
        let system = WindowsSystem::new();
        let registry = system.registry();
        let key = r"HKCU\Software\WattClean-Test";
        registry::create_for_test(key, "Value").unwrap();
        assert!(registry.key_exists(key));
        assert!(registry.value_exists(key, "value"));
        registry.delete_value(key, "Value").unwrap();
        assert!(!registry.value_exists(key, "Value"));
        registry.delete_key(key).unwrap();
        assert!(!registry.key_exists(key));
        assert!(!registry.key_exists(r"HKXX\Nope"));
    }
}
