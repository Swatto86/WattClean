//! Every cleaner the app knows: built in, custom files (TOML or CleanerML)
//! from the cleaners folder, and an imported winapp2.ini. Also whether each
//! one is relevant on this machine and whether its program is running.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use serde::Serialize;

use crate::model::{Category, Cleaner, Detect, OptionKey, Source};
use crate::system::{Platform, Registry, System};
use crate::wildcard::{Wildcard, glob};
use crate::{cleanerml, defs, vars, winapp2};

/// The file a winapp2.ini import is kept as, inside the cleaners folder.
pub const WINAPP2_FILE: &str = "winapp2.ini";

#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub cleaners: Vec<Cleaner>,
    /// What could not be loaded, for Settings to show.
    pub problems: Vec<String>,
}

impl Catalog {
    /// The platform's built-in cleaners, then custom files, then
    /// winapp2.ini when `community` is on (Windows only: its entries are
    /// Windows paths). A later cleaner with an id already taken is reported
    /// and left out.
    pub fn load(platform: Platform, cleaners_dir: Option<&Path>, community: bool) -> Self {
        let (mut cleaners, problems) = defs::built_in(platform);
        let mut catalog = Self {
            cleaners: Vec::new(),
            problems: problems.iter().map(ToString::to_string).collect(),
        };
        if let Some(dir) = cleaners_dir {
            cleaners.extend(catalog.load_custom(dir, platform));
            if community
                && platform == Platform::Windows
                && let Ok(text) = fs::read_to_string(dir.join(WINAPP2_FILE))
            {
                cleaners.extend(winapp2::parse(&text));
            }
        }
        let mut ids = HashSet::new();
        for cleaner in cleaners {
            if ids.insert(cleaner.id.clone()) {
                catalog.cleaners.push(cleaner);
            } else {
                catalog.problems.push(format!(
                    "{}: another cleaner already has this id",
                    cleaner.id
                ));
            }
        }
        catalog
    }

    fn load_custom(&mut self, dir: &Path, platform: Platform) -> Vec<Cleaner> {
        let Ok(entries) = fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut files: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        files.sort();
        let mut toml_files = Vec::new();
        let mut out = Vec::new();
        for path in files {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let extension = path
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase());
            match extension.as_deref() {
                Some("xml") => match fs::read_to_string(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|t| cleanerml::parse(&t, platform))
                {
                    Ok((cleaner, notes)) => {
                        self.problems
                            .extend(notes.into_iter().map(|n| format!("{name}: {n}")));
                        out.push(cleaner);
                    }
                    Err(error) => self.problems.push(format!("{name}: {error}")),
                },
                Some("toml") => match fs::read_to_string(&path) {
                    Ok(text) => toml_files.push((name, text)),
                    Err(error) => self.problems.push(format!("{name}: {error}")),
                },
                _ => {}
            }
        }
        let borrowed: Vec<(&str, &str)> = toml_files
            .iter()
            .map(|(n, t)| (n.as_str(), t.as_str()))
            .collect();
        let (mut cleaners, problems) = defs::parse_files(&borrowed, Source::Custom);
        // Custom files may name any category but are listed under Custom.
        for cleaner in &mut cleaners {
            cleaner.category = Category::Custom;
        }
        self.problems
            .extend(problems.iter().map(ToString::to_string));
        out.extend(cleaners);
        out
    }

    pub fn cleaner(&self, id: &str) -> Option<&Cleaner> {
        self.cleaners.iter().find(|c| c.id == id)
    }

    pub fn contains(&self, key: &OptionKey) -> bool {
        self.cleaner(&key.cleaner)
            .is_some_and(|c| c.options.iter().any(|o| o.id == key.option))
    }

    /// The options ticked in a fresh installation.
    pub fn defaults(&self) -> Vec<OptionKey> {
        self.cleaners
            .iter()
            .flat_map(|c| {
                c.options
                    .iter()
                    .filter(|o| o.default)
                    .map(|o| OptionKey::new(&c.id, &o.id))
            })
            .collect()
    }

    /// Every option without a warning (the command line's --all-but-warning).
    pub fn all_but_warning(&self) -> Vec<OptionKey> {
        self.cleaners
            .iter()
            .flat_map(|c| {
                c.options
                    .iter()
                    .filter(|o| o.warning.is_none())
                    .map(|o| OptionKey::new(&c.id, &o.id))
            })
            .collect()
    }
}

/// Whether the cleaner's program seems installed (always, with no rules).
pub fn is_present(cleaner: &Cleaner, system: &dyn System) -> bool {
    if cleaner.detect.is_empty() {
        return true;
    }
    let lookup = |name: &str| system.var(name);
    cleaner.detect.iter().any(|detect| match detect {
        Detect::Path(pattern) => vars::expand(pattern, &lookup)
            .iter()
            .any(|p| !glob(p).is_empty()),
        Detect::Registry(key) => key_found(system.registry(), key),
        Detect::Program(name) => system.find_program(name).is_some(),
    })
}

/// Whether a registry key exists; `*` and `?` may stand in any part after
/// the hive, as winapp2.ini writes some `Detect=` lines.
fn key_found(registry: &dyn Registry, pattern: &str) -> bool {
    if !Wildcard::has_wildcards(pattern) {
        return registry.key_exists(pattern);
    }
    let mut parts = pattern.trim().trim_end_matches('\\').split('\\');
    let mut keys: Vec<String> = parts.next().map(str::to_string).into_iter().collect();
    for part in parts {
        let wildcard = Wildcard::new(part);
        keys = keys
            .iter()
            .flat_map(|key| {
                registry
                    .subkeys(key)
                    .into_iter()
                    .filter(|name| wildcard.matches(name))
                    .map(move |name| format!(r"{key}\{name}"))
            })
            .collect();
        if keys.is_empty() {
            return false;
        }
    }
    !keys.is_empty()
}

/// The cleaner and option list as the window shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanerView {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: Category,
    pub source: Source,
    pub present: bool,
    pub running: bool,
    pub options: Vec<OptionView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionView {
    pub id: String,
    pub name: String,
    pub description: String,
    pub warning: Option<String>,
    pub default: bool,
}

pub fn views(catalog: &Catalog, system: &dyn System) -> Vec<CleanerView> {
    let running = system.running();
    catalog
        .cleaners
        .iter()
        .map(|c| CleanerView {
            id: c.id.clone(),
            name: c.name.clone(),
            description: c.description.clone(),
            category: c.category,
            source: c.source,
            present: is_present(c, system),
            running: c.running.iter().any(|exe| running.contains(exe)),
            options: c
                .options
                .iter()
                .map(|o| OptionView {
                    id: o.id.clone(),
                    name: o.name.clone(),
                    description: o.description.clone(),
                    warning: o.warning.clone(),
                    default: o.default,
                })
                .collect(),
        })
        .collect()
}
