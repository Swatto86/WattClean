//! What is installed on macOS: application bundles in the Applications
//! folders, Homebrew's formulae and casks (Homebrew runs on Linux too) and
//! MacPorts' ports. Apple's own applications are left out: macOS protects
//! them.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::{Manager, Package, PackageSystem, query, tree_size};

pub fn list(system: &dyn PackageSystem) -> (Vec<Package>, Vec<String>) {
    let mut packages = Vec::new();
    let mut problems = Vec::new();
    if system.find_program("brew").is_some() {
        match homebrew(system) {
            Some(found) => packages.extend(found),
            None => problems.push("Homebrew could not list its packages".to_string()),
        }
    }
    if system.find_program("port").is_some() {
        match query(system, "port", &["-q", "installed", "requested"]) {
            Some(text) => packages.extend(parse_ports(&text)),
            None => problems.push("MacPorts could not list its ports".to_string()),
        }
    }
    // An app a cask installed is removed through Homebrew, so it is listed
    // once, as the cask, which takes the app's bundle identifier, version
    // and names.
    let mut apps = applications(system);
    let mut taken = HashSet::new();
    for cask in packages
        .iter_mut()
        .filter(|p| p.manager == Manager::HomebrewCask)
    {
        let found = apps
            .iter()
            .find(|app| cask.app_files.contains(&bundle_file(&app.id)));
        match found {
            Some(app) => {
                cask.bundle_id.clone_from(&app.bundle_id);
                if app.version.is_some() {
                    cask.version.clone_from(&app.version);
                }
                cask.size = app.size;
                cask.known_as.push(app.name.clone());
                if let Some(stem) = Path::new(&app.id).file_stem() {
                    cask.known_as.push(stem.to_string_lossy().into_owned());
                }
                taken.insert(app.id.clone());
            }
            None if !cask.app_files.is_empty() => {
                cask.summary =
                    Some("App not found: only Homebrew's record of it remains".to_string());
            }
            None => {}
        }
    }
    apps.retain(|app| !taken.contains(&app.id));
    packages.extend(apps);
    (packages, problems)
}

fn bundle_file(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

#[derive(Debug, Deserialize)]
struct BrewInfo {
    #[serde(default)]
    formulae: Vec<Formula>,
    #[serde(default)]
    casks: Vec<Cask>,
}

#[derive(Debug, Deserialize)]
struct Formula {
    name: String,
    #[serde(default)]
    desc: Option<String>,
    #[serde(default)]
    installed: Vec<Installed>,
}

#[derive(Debug, Deserialize)]
struct Installed {
    version: String,
    #[serde(default)]
    installed_on_request: bool,
}

#[derive(Debug, Deserialize)]
struct Cask {
    token: String,
    #[serde(default)]
    name: Vec<String>,
    #[serde(default)]
    desc: Option<String>,
    #[serde(default)]
    installed: Option<String>,
    #[serde(default)]
    artifacts: Vec<serde_json::Value>,
}

/// Homebrew's formulae and casks. A formula you asked for counts as an
/// application; one installed only for another does not.
pub fn homebrew(system: &dyn PackageSystem) -> Option<Vec<Package>> {
    let text = query(system, "brew", &["info", "--json=v2", "--installed"])?;
    let mut packages = parse_brew(&text)?;
    let cellars = [
        "/opt/homebrew/Cellar",
        "/usr/local/Cellar",
        "/home/linuxbrew/.linuxbrew/Cellar",
    ];
    for package in &mut packages {
        package.size = match package.manager {
            Manager::Homebrew => cellars
                .iter()
                .map(|cellar| system.at(cellar).join(&package.id))
                .find(|folder| folder.is_dir())
                .map(|folder| tree_size(&folder)),
            // A cask's size is its app's, set when the app is matched.
            _ => None,
        };
    }
    Some(packages)
}

/// `brew info --json=v2 --installed`. A cask's `bundle_id` holds the file
/// name of the app it installed, in lower case, until the app is matched.
pub(crate) fn parse_brew(text: &str) -> Option<Vec<Package>> {
    let info: BrewInfo = serde_json::from_str(text).ok()?;
    let mut out = Vec::new();
    for formula in info.formulae {
        let mut package = Package::new(Manager::Homebrew, &formula.name, &formula.name);
        package.version = formula.installed.last().map(|i| i.version.clone());
        package.app = formula.installed.iter().any(|i| i.installed_on_request);
        package.summary = formula.desc;
        out.push(package);
    }
    for cask in info.casks {
        let name = cask
            .name
            .first()
            .cloned()
            .unwrap_or_else(|| cask.token.clone());
        let mut package = Package::new(Manager::HomebrewCask, &cask.token, &name);
        // Homebrew writes `version,build`; the build is not a version.
        package.version = cask
            .installed
            .map(|v| v.split(',').next().unwrap_or_default().to_string())
            .filter(|v| !v.is_empty());
        package.summary = cask.desc;
        // Fonts, plugins and command-line tools are not applications.
        package.app = cask
            .artifacts
            .iter()
            .any(|a| LAUNCHABLE.iter().any(|kind| a.get(*kind).is_some()));
        package.app_files = app_files(&cask.artifacts);
        package.zap = zap_trash(&cask.artifacts);
        out.push(package);
    }
    Some(out)
}

/// Cask artifacts that put something people start on the Mac.
const LAUNCHABLE: [&str; 4] = ["app", "suite", "pkg", "installer"];

/// The app bundles a cask installs, by file name in lower case: each `app`
/// artifact (its `target` when it renames the app), else the `.app` its
/// uninstall step deletes (casks that run an installer).
fn app_files(artifacts: &[serde_json::Value]) -> Vec<String> {
    let file = |path: &str| {
        let name = path.rsplit('/').next().unwrap_or(path).to_lowercase();
        name.ends_with(".app").then_some(name)
    };
    let mut out: Vec<String> = artifacts
        .iter()
        .filter_map(|artifact| {
            let target = artifact.get("target").and_then(serde_json::Value::as_str);
            let app = artifact.get("app")?.as_array()?.first()?.as_str();
            target.or(app).and_then(file)
        })
        .collect();
    if out.is_empty() {
        out = artifacts
            .iter()
            .filter_map(|artifact| artifact.get("uninstall")?.as_array())
            .flatten()
            .filter_map(|step| step.get("delete"))
            .flat_map(strings)
            .filter_map(|path| file(&path))
            .collect();
    }
    out
}

/// What a cask's zap step trashes; its other steps (rmdir of shared vendor
/// folders, scripts) are never taken.
fn zap_trash(artifacts: &[serde_json::Value]) -> Vec<String> {
    artifacts
        .iter()
        .filter_map(|artifact| artifact.get("zap")?.as_array())
        .flatten()
        .filter_map(|step| step.get("trash"))
        .flat_map(strings)
        .collect()
}

/// A JSON string or array of strings.
fn strings(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::String(text) => vec![text.clone()],
        serde_json::Value::Array(items) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

/// `port -q installed requested`: `  name @1.2.3_0+variant (active)`.
pub(crate) fn parse_ports(text: &str) -> Vec<Package> {
    text.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let name = words.next()?;
            let version = words.next()?.strip_prefix('@')?;
            let mut package = Package::new(Manager::MacPorts, name, name);
            package.version = Some(version.split('+').next().unwrap_or(version).to_string());
            package.app = true;
            Some(package)
        })
        .collect()
}

/// Application bundles in /Applications and ~/Applications, and one folder
/// deeper (suites keep their apps in a folder of their own).
fn applications(system: &dyn PackageSystem) -> Vec<Package> {
    let mut folders: Vec<PathBuf> = vec![system.at("/Applications")];
    folders.extend(system.home().map(|home| home.join("Applications")));
    let mut bundles = Vec::new();
    for folder in &folders {
        for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
            let path = entry.path();
            if is_bundle(&path) {
                bundles.push(path);
            } else if path.is_dir() && !path.is_symlink() {
                bundles.extend(
                    std::fs::read_dir(&path)
                        .into_iter()
                        .flatten()
                        .flatten()
                        .map(|e| e.path())
                        .filter(|p| is_bundle(p)),
                );
            }
        }
    }
    bundles
        .into_iter()
        .filter_map(|bundle| {
            let info = system.bundle_info(&bundle).unwrap_or_default();
            if info
                .bundle_id
                .as_deref()
                .is_some_and(|id| id.starts_with("com.apple."))
            {
                return None;
            }
            let stem = bundle.file_stem()?.to_string_lossy().to_string();
            let mut package = Package::new(
                Manager::MacApp,
                &bundle.to_string_lossy(),
                &info.name.unwrap_or(stem),
            );
            package.version = info.version;
            package.bundle_id = info.bundle_id;
            package.size = Some(tree_size(&bundle));
            package.app = true;
            Some(package)
        })
        .collect()
}

fn is_bundle(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
        && path.is_dir()
        && !path.is_symlink()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn homebrew_lists_formulae_and_casks() {
        let json = r#"{"formulae":[
            {"name":"wget","desc":"Internet file retriever","installed":[{"version":"1.24.5","installed_on_request":true}]},
            {"name":"libidn2","installed":[{"version":"2.3.7","installed_on_request":false}]}],
          "casks":[{"token":"firefox","name":["Mozilla Firefox"],"desc":"Web browser","installed":"131.0",
            "artifacts":[{"uninstall":[]},{"app":["Firefox.app"]},{"zap":[]}]}]}"#;
        let packages = parse_brew(json).unwrap();
        assert_eq!(packages.len(), 3);
        assert!(packages[0].app && !packages[1].app);
        assert_eq!(packages[0].version.as_deref(), Some("1.24.5"));
        let firefox = &packages[2];
        assert_eq!(firefox.app_files, ["firefox.app"]);
        assert_eq!(
            (firefox.manager, firefox.name.as_str()),
            (Manager::HomebrewCask, "Mozilla Firefox")
        );
        assert!(parse_brew("not json").is_none());
    }

    #[test]
    fn casks_know_their_apps_versions_and_zap_lists() {
        let json = r#"{"casks":[
          {"token":"battle-net","name":["Blizzard Battle.net"],"installed":"1.19.3.3219",
           "artifacts":[{"installer":[{"manual":"Battle.net-Setup.app"}]},
             {"uninstall":[{"delete":"/Applications/Battle.net.app"}]},
             {"zap":[{"trash":["~/Library/Application Support/Battle.net","~/Library/Caches/com.blizzard.Battle.net"],"rmdir":"~/Blizzard"}]}]},
          {"token":"claude","name":["Claude"],"installed":"2.42304.2,af65c0e3",
           "artifacts":[{"app":["Claude.app"],"target":"/Applications/Claude.app"},{"zap":[{"trash":"~/Library/Caches/com.anthropic.claudefordesktop.ShipIt"}]}]},
          {"token":"font-x","name":["X Font"],"installed":"3.5.1","artifacts":[{"font":["X.ttf"]}]},
          {"token":"tool","name":["Tool"],"installed":"1","artifacts":[{"pkg":["Tool.pkg"]},{"uninstall":[{"delete":["/Applications/Tool.app","/usr/local/bin/tool"]}]}]}]}"#;
        let casks = parse_brew(json).unwrap();
        let battle = &casks[0];
        assert!(battle.app);
        assert_eq!(battle.app_files, ["battle.net.app"]);
        assert_eq!(
            battle.zap.len(),
            2,
            "rmdir is never taken: {:?}",
            battle.zap
        );
        assert_eq!(casks[1].version.as_deref(), Some("2.42304.2"));
        assert_eq!(casks[1].app_files, ["claude.app"]);
        assert_eq!(
            casks[1].zap,
            ["~/Library/Caches/com.anthropic.claudefordesktop.ShipIt"]
        );
        assert!(!casks[2].app, "a font is not an application");
        assert!(casks[3].app && casks[3].app_files == ["tool.app"]);
    }

    #[test]
    fn ports_are_read_with_their_versions() {
        let text = "  ffmpeg @7.0.1_0+gpl2 (active)\n  python312 @3.12.4_0 (active)\n";
        let ports = parse_ports(text);
        assert_eq!(ports.len(), 2);
        assert_eq!(ports[0].version.as_deref(), Some("7.0.1_0"));
    }
}
