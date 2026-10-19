//! The kinds of registry problems the cleaner looks for, and which scan
//! mode includes which. Each mode includes the one before it.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Category {
    Startup,
    AppPaths,
    Cache,
    MissingSharedDlls,
    Uninstall,
    ContextMenus,
    ComObjects,
    MsiFolders,
    Firewall,
    SystemSettings,
    Extensions,
    UnusedSharedDlls,
    Services,
    EmptyKeys,
    ClassKeys,
    History,
    IncludeList,
    /// Matches of a registry search; never part of a cleaner scan.
    Search,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    Safe,
    Moderate,
    Advanced,
    /// The categories the person ticked.
    Custom,
}

impl Category {
    /// Every category a person can choose for a custom scan.
    pub const CHOOSABLE: [Self; 17] = [
        Self::Startup,
        Self::AppPaths,
        Self::Cache,
        Self::MissingSharedDlls,
        Self::Uninstall,
        Self::ContextMenus,
        Self::ComObjects,
        Self::MsiFolders,
        Self::Firewall,
        Self::SystemSettings,
        Self::Extensions,
        Self::UnusedSharedDlls,
        Self::Services,
        Self::EmptyKeys,
        Self::ClassKeys,
        Self::History,
        Self::IncludeList,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Startup => "Missing startup items",
            Self::AppPaths => "Wrong application paths",
            Self::Cache => "Invalid cache entries",
            Self::MissingSharedDlls => "Missing shared DLLs",
            Self::Uninstall => "Incorrect uninstaller data",
            Self::ContextMenus => "Broken context menu commands",
            Self::ComObjects => "Missing COM and ActiveX components",
            Self::MsiFolders => "Invalid Windows Installer folders",
            Self::Firewall => "Invalid firewall rules",
            Self::SystemSettings => "Missing sounds and fonts",
            Self::Extensions => "Unused file extensions",
            Self::UnusedSharedDlls => "Unused shared DLLs",
            Self::Services => "Services with missing programs",
            Self::EmptyKeys => "Empty software keys",
            Self::ClassKeys => "Broken file types",
            Self::History => "Recently used lists and history",
            Self::IncludeList => "Include list",
            Self::Search => "Search results",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Startup => {
                "Programs set to start with Windows whose files no longer exist, and startup switches left after their program was removed."
            }
            Self::AppPaths => {
                "Run-dialog shortcuts (App Paths) and application registrations whose program is missing."
            }
            Self::Cache => {
                "Windows' caches of program names and compatibility settings for programs that no longer exist."
            }
            Self::MissingSharedDlls => "Shared DLL counters for files that no longer exist.",
            Self::Uninstall => {
                "Entries in Installed apps whose uninstaller and folder are both gone."
            }
            Self::ContextMenus => {
                "Right-click commands in File Explorer that run a program that no longer exists."
            }
            Self::ComObjects => {
                "COM and ActiveX registrations and type libraries whose file is missing."
            }
            Self::MsiFolders => "Folders Windows Installer recorded that no longer exist.",
            Self::Firewall => "Windows Firewall rules for programs that no longer exist.",
            Self::SystemSettings => "Sound events and fonts that point at missing files.",
            Self::Extensions => {
                "File extensions that name a file type no longer registered, and hold nothing else."
            }
            Self::UnusedSharedDlls => {
                "Shared DLL counters already at zero: no installed program claims the file. Not ticked by default."
            }
            Self::Services => {
                "Windows services whose program file is missing. Not ticked by default: check each one."
            }
            Self::EmptyKeys => {
                "Software keys of a vendor or product that hold no data at all. Not ticked by default: some programs use an empty key as a marker."
            }
            Self::ClassKeys => {
                "File types whose every command runs a program that no longer exists."
            }
            Self::History => {
                "Lists of recently opened files, typed paths, Run-box commands and searches. Windows starts them again empty."
            }
            Self::IncludeList => {
                "Keys you added to the include list in Settings: their contents are offered for removal."
            }
            Self::Search => "Keys and values matching a registry search.",
        }
    }

    /// The categories a mode scans.
    pub fn for_mode(mode: Mode, custom: &[Self]) -> Vec<Self> {
        const SAFE: [Category; 6] = [
            Category::Startup,
            Category::AppPaths,
            Category::Cache,
            Category::MissingSharedDlls,
            Category::Uninstall,
            Category::ContextMenus,
        ];
        const MODERATE: [Category; 5] = [
            Category::ComObjects,
            Category::MsiFolders,
            Category::Firewall,
            Category::SystemSettings,
            Category::Extensions,
        ];
        const ADVANCED: [Category; 5] = [
            Category::UnusedSharedDlls,
            Category::Services,
            Category::EmptyKeys,
            Category::ClassKeys,
            Category::History,
        ];
        let mut chosen: Vec<Self> = match mode {
            Mode::Safe => SAFE.to_vec(),
            Mode::Moderate => [SAFE.as_slice(), &MODERATE].concat(),
            Mode::Advanced => [SAFE.as_slice(), &MODERATE, &ADVANCED].concat(),
            Mode::Custom => custom
                .iter()
                .copied()
                .filter(|category| *category != Self::Search)
                .collect(),
        };
        chosen.sort();
        chosen.dedup();
        chosen
    }
}

#[cfg(test)]
#[cfg(windows)]
mod tests {
    use super::*;

    #[test]
    fn each_mode_includes_the_one_before() {
        let safe = Category::for_mode(Mode::Safe, &[]);
        let moderate = Category::for_mode(Mode::Moderate, &[]);
        let advanced = Category::for_mode(Mode::Advanced, &[]);
        assert!(safe.iter().all(|c| moderate.contains(c)));
        assert!(moderate.iter().all(|c| advanced.contains(c)));
        assert!(safe.len() < moderate.len() && moderate.len() < advanced.len());
        assert!(!advanced.contains(&Category::IncludeList), "custom only");
    }

    #[test]
    fn custom_takes_the_choice_but_never_search() {
        let chosen = Category::for_mode(
            Mode::Custom,
            &[Category::History, Category::Search, Category::History],
        );
        assert_eq!(chosen, vec![Category::History]);
    }

    #[test]
    fn categories_serialise_in_camel_case() {
        let text = serde_json::to_string(&Category::MissingSharedDlls).unwrap();
        assert_eq!(text, "\"missingSharedDlls\"");
        assert!(serde_json::from_str::<Mode>("\"reckless\"").is_err());
    }
}
