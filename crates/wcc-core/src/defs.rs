//! Cleaner definitions in TOML: the built-in ones (compiled in from
//! `src/defs/*.toml`) and any custom `.toml` file a person adds.
//!
//! A definition file holds `[[template]]` blocks (options shared by several
//! applications, such as every Chromium browser) and `[[cleaner]]` blocks.
//! A cleaner names a template and fills its `{placeholders}` from `vars`.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::Deserialize;

use crate::model::{
    Action, Category, Cleaner, CleanerOption, Detect, FileSpec, Kind, Search, Source, SqliteOp,
};
use crate::vars::fill_placeholders;

/// The built-in definitions, by file name.
pub const BUILT_IN: &[(&str, &str)] = &[
    ("system.toml", include_str!("defs/system.toml")),
    ("browsers.toml", include_str!("defs/browsers.toml")),
    ("applications.toml", include_str!("defs/applications.toml")),
    ("developer.toml", include_str!("defs/developer.toml")),
    ("games.toml", include_str!("defs/games.toml")),
    ("deepscan.toml", include_str!("defs/deepscan.toml")),
];

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DefError {
    #[error("{file}: {message}")]
    Invalid { file: String, message: String },
}

fn invalid(file: &str, message: impl Into<String>) -> DefError {
    DefError::Invalid {
        file: file.to_string(),
        message: message.into(),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileDef {
    #[serde(default)]
    template: Vec<TemplateDef>,
    #[serde(default)]
    cleaner: Vec<CleanerDef>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TemplateDef {
    id: String,
    /// Placeholder values a cleaner may override.
    #[serde(default)]
    vars: BTreeMap<String, String>,
    option: Vec<OptionDef>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CleanerDef {
    id: String,
    name: String,
    #[serde(default)]
    description: String,
    category: Category,
    template: Option<String>,
    #[serde(default)]
    vars: BTreeMap<String, String>,
    #[serde(default)]
    detect: Vec<String>,
    #[serde(default)]
    detect_registry: Vec<String>,
    #[serde(default)]
    running: Vec<String>,
    /// Template options this cleaner leaves out.
    #[serde(default)]
    skip: Vec<String>,
    #[serde(default)]
    option: Vec<OptionDef>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct OptionDef {
    id: String,
    name: String,
    #[serde(default)]
    description: String,
    warning: Option<String>,
    #[serde(default)]
    default: bool,
    actions: Vec<ActionDef>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Op {
    Delete,
    Truncate,
    Sqlite,
    Json,
    Ini,
    Registry,
    RecycleBin,
    Clipboard,
    CustomPaths,
    FreeDiskSpace,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActionDef {
    op: Op,
    path: Option<String>,
    search: Option<Search>,
    regex: Option<String>,
    nregex: Option<String>,
    wholeregex: Option<String>,
    nwholeregex: Option<String>,
    names: Option<String>,
    beside: Option<String>,
    kind: Option<Kind>,
    older_than_days: Option<u32>,
    #[serde(default)]
    exclude: Vec<String>,
    sql: Option<SqliteOp>,
    address: Option<String>,
    section: Option<String>,
    parameter: Option<String>,
    key: Option<String>,
    value: Option<String>,
}

/// Parses every built-in file; a problem in one file leaves the others.
pub fn built_in() -> (Vec<Cleaner>, Vec<DefError>) {
    parse_files(BUILT_IN, Source::BuiltIn)
}

/// Parses several files that may share templates.
pub fn parse_files(files: &[(&str, &str)], source: Source) -> (Vec<Cleaner>, Vec<DefError>) {
    let mut problems = Vec::new();
    let mut parsed = Vec::new();
    let mut templates: HashMap<String, (BTreeMap<String, String>, Vec<OptionDef>)> = HashMap::new();
    for (name, text) in files {
        match toml::from_str::<FileDef>(text) {
            Ok(file) => {
                for template in &file.template {
                    templates.insert(
                        template.id.clone(),
                        (template.vars.clone(), template.option.clone()),
                    );
                }
                parsed.push((*name, file));
            }
            Err(error) => problems.push(invalid(name, error.to_string())),
        }
    }
    let mut cleaners = Vec::new();
    for (name, file) in parsed {
        for def in file.cleaner {
            match build_cleaner(name, def, &templates, source) {
                Ok(cleaner) => cleaners.push(cleaner),
                Err(error) => problems.push(error),
            }
        }
    }
    (cleaners, problems)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}

fn build_cleaner(
    file: &str,
    def: CleanerDef,
    templates: &HashMap<String, (BTreeMap<String, String>, Vec<OptionDef>)>,
    source: Source,
) -> Result<Cleaner, DefError> {
    if !valid_id(&def.id) {
        return Err(invalid(
            file,
            format!("cleaner id {:?} must be letters, digits, _ or -", def.id),
        ));
    }
    let mut merged = BTreeMap::new();
    let mut option_defs = Vec::new();
    if let Some(template) = &def.template {
        let (defaults, options) = templates
            .get(template)
            .ok_or_else(|| invalid(file, format!("{}: no template {template:?}", def.id)))?;
        merged.extend(defaults.clone());
        option_defs.extend(
            options
                .iter()
                .filter(|o| !def.skip.contains(&o.id))
                .cloned(),
        );
    }
    merged.extend(def.vars);
    let vars: Vec<(String, String)> = merged.into_iter().collect();
    let fill = |text: &str| fill_placeholders(text, &vars);
    option_defs.extend(def.option);
    let mut seen = HashSet::new();
    let mut options = Vec::new();
    for option in option_defs {
        if !valid_id(&option.id) || !seen.insert(option.id.clone()) {
            return Err(invalid(
                file,
                format!(
                    "{}: option id {:?} is invalid or repeated",
                    def.id, option.id
                ),
            ));
        }
        let actions = option
            .actions
            .iter()
            .map(|a| build_action(a, &fill))
            .collect::<Result<Vec<_>, String>>()
            .map_err(|m| invalid(file, format!("{}.{}: {m}", def.id, option.id)))?;
        options.push(CleanerOption {
            id: option.id,
            name: option.name,
            description: option.description,
            warning: option.warning,
            default: option.default,
            actions,
        });
    }
    if options.is_empty() {
        return Err(invalid(file, format!("{}: no options", def.id)));
    }
    let mut detect: Vec<Detect> = def.detect.iter().map(|d| Detect::Path(fill(d))).collect();
    detect.extend(
        def.detect_registry
            .iter()
            .map(|d| Detect::Registry(fill(d))),
    );
    Ok(Cleaner {
        id: def.id,
        name: def.name,
        description: def.description,
        category: def.category,
        source,
        running: def.running.iter().map(|r| r.to_ascii_lowercase()).collect(),
        detect,
        options,
    })
}

fn build_action(def: &ActionDef, fill: &dyn Fn(&str) -> String) -> Result<Action, String> {
    let files = || -> Result<FileSpec, String> {
        let path = def.path.as_deref().ok_or("a file action needs a path")?;
        let search = def.search.unwrap_or(Search::File);
        if search == Search::FindDirs && def.names.is_none() {
            return Err("find.dirs needs the names of the folders to find".into());
        }
        if search != Search::FindDirs && def.beside.is_some() {
            return Err("beside works only with find.dirs".into());
        }
        Ok(FileSpec {
            path: fill(path),
            search,
            regex: def.regex.clone(),
            nregex: def.nregex.clone(),
            wholeregex: def.wholeregex.clone(),
            nwholeregex: def.nwholeregex.clone(),
            names: def.names.clone(),
            beside: def.beside.clone(),
            kind: def.kind,
            older_than_days: def.older_than_days,
            exclude: def.exclude.iter().map(|e| fill(e)).collect(),
        })
    };
    Ok(match def.op {
        Op::Delete => Action::Delete(files()?),
        Op::Truncate => Action::Truncate(files()?),
        Op::Sqlite => Action::Sqlite {
            files: files()?,
            op: def.sql.ok_or("an sqlite action needs sql")?,
        },
        Op::Json => Action::Json {
            files: files()?,
            address: def
                .address
                .clone()
                .ok_or("a json action needs an address")?,
        },
        Op::Ini => Action::Ini {
            files: files()?,
            section: def.section.clone().ok_or("an ini action needs a section")?,
            parameter: def.parameter.clone(),
        },
        Op::Registry => Action::Registry {
            key: fill(def.key.as_deref().ok_or("a registry action needs a key")?),
            value: def.value.clone(),
        },
        Op::RecycleBin => Action::RecycleBin,
        Op::Clipboard => Action::Clipboard,
        Op::CustomPaths => Action::CustomPaths,
        Op::FreeDiskSpace => Action::FreeDiskSpace,
    })
}

#[cfg(test)]
#[path = "defs_tests.rs"]
mod tests;
