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
    // once, as the cask, which takes the app's bundle identifier.
    let mut apps = applications(system);
    let mut taken = HashSet::new();
    for cask in packages
        .iter_mut()
        .filter(|p| p.manager == Manager::HomebrewCask)
    {
        let app_file = cask.bundle_id.take();
        if let Some(app) = apps
            .iter()
            .find(|app| Some(bundle_file(&app.id)) == app_file)
        {
            cask.bundle_id.clone_from(&app.bundle_id);
            taken.insert(app.id.clone());
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
            _ => package
                .bundle_id
                .as_ref()
                .map(|app| system.at("/Applications").join(app))
                .filter(|app| app.exists())
                .map(|app| tree_size(&app)),
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
        package.version = cask.installed;
        package.summary = cask.desc;
        package.app = true;
        package.bundle_id = cask
            .artifacts
            .iter()
            .filter_map(|artifact| artifact.get("app")?.as_array()?.first()?.as_str())
            .map(str::to_lowercase)
            .next();
        out.push(package);
    }
    Some(out)
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
        assert_eq!(
            (firefox.manager, firefox.name.as_str()),
            (Manager::HomebrewCask, "Mozilla Firefox")
        );
        assert_eq!(firefox.bundle_id.as_deref(), Some("firefox.app"));
        assert!(parse_brew("not json").is_none());
    }

    #[test]
    fn ports_are_read_with_their_versions() {
        let text = "  ffmpeg @7.0.1_0+gpl2 (active)\n  python312 @3.12.4_0 (active)\n";
        let ports = parse_ports(text);
        assert_eq!(ports.len(), 2);
        assert_eq!(ports[0].version.as_deref(), Some("7.0.1_0"));
    }
}
