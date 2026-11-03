//! BleachBit's own cleaner format (CleanerML), so a person's existing custom
//! BleachBit cleaners keep working. Each platform reads its own parts: an
//! element marked for another system (`os="linux"` on Windows,
//! `os="windows"` on Linux) is skipped. On Linux and macOS a path starting
//! with `~` starts in the home folder.
//!
//! Commands understood: `delete`, `shred` (a delete; overwriting follows the
//! setting), `truncate`, `winreg`, `json`, `ini`, `sqlite.vacuum`,
//! `chrome.history`, `chrome.autofill`, `chrome.keywords` and
//! `mozilla.url.history` (also `mozilla_url_history`). Others are reported.

use std::collections::HashMap;

use roxmltree::{Document, Node};

use crate::model::{
    Action, Category, Cleaner, CleanerOption, FileSpec, Kind, Search, Source, SqliteOp, slug,
};
use crate::system::Platform;

/// The cleaner, and what in the file was not understood.
pub fn parse(text: &str, platform: Platform) -> Result<(Cleaner, Vec<String>), String> {
    let doc = Document::parse(text).map_err(|e| e.to_string())?;
    let root = doc.root_element();
    if root.tag_name().name() != "cleaner" {
        return Err("the root element is not <cleaner>".into());
    }
    if !fits(root, platform) {
        return Err("this cleaner is for another system".into());
    }
    let id = slug(root.attribute("id").unwrap_or_default());
    if id.is_empty() {
        return Err("the cleaner has no id".into());
    }
    let mut notes = Vec::new();
    let vars = read_vars(root, platform);
    let mut options = Vec::new();
    for option in children(root, "option", platform) {
        let option_id = slug(option.attribute("id").unwrap_or_default());
        if option_id.is_empty() {
            notes.push("an option without an id was skipped".into());
            continue;
        }
        let mut actions = Vec::new();
        for action in children(option, "action", platform) {
            match read_action(action, &vars, platform) {
                Ok(list) => actions.extend(list),
                Err(note) => notes.push(format!("{option_id}: {note}")),
            }
        }
        if actions.is_empty() {
            continue;
        }
        options.push(CleanerOption {
            id: option_id.clone(),
            name: text_of(option, "label", platform).unwrap_or(option_id),
            description: text_of(option, "description", platform).unwrap_or_default(),
            warning: text_of(option, "warning", platform),
            default: false,
            actions,
        });
    }
    if options.is_empty() {
        return Err("no option has an action for this system".into());
    }
    let cleaner = Cleaner {
        id: format!("ml-{id}"),
        name: text_of(root, "label", platform).unwrap_or_else(|| id.clone()),
        description: text_of(root, "description", platform).unwrap_or_default(),
        category: Category::Custom,
        source: Source::Custom,
        running: children(root, "running", platform)
            .filter_map(|r| r.text())
            .map(|t| t.trim().to_ascii_lowercase())
            .filter(|t| !t.is_empty())
            .collect(),
        detect: Vec::new(),
        options,
    };
    Ok((cleaner, notes))
}

/// Whether an element is for this platform (no `os` means every one).
fn fits(node: Node, platform: Platform) -> bool {
    node.attribute("os").is_none_or(|os| {
        let os = os.to_ascii_lowercase();
        match platform {
            Platform::Windows => os == "windows",
            Platform::Linux => matches!(os.as_str(), "linux" | "unix" | "posix"),
            Platform::Macos => matches!(os.as_str(), "darwin" | "macos" | "osx" | "unix" | "posix"),
        }
    })
}

fn children<'a, 'input>(
    node: Node<'a, 'input>,
    name: &'a str,
    platform: Platform,
) -> impl Iterator<Item = Node<'a, 'input>> + 'a {
    node.children()
        .filter(move |c| c.is_element() && c.tag_name().name() == name && fits(*c, platform))
}

fn text_of(node: Node, name: &str, platform: Platform) -> Option<String> {
    children(node, name, platform)
        .next()
        .and_then(|n| n.text())
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// `<var name="x"><value>…</value></var>`, used as `$$x$$` in paths.
fn read_vars(root: Node, platform: Platform) -> HashMap<String, Vec<String>> {
    let mut vars: HashMap<String, Vec<String>> = HashMap::new();
    for var in children(root, "var", platform) {
        let Some(name) = var.attribute("name") else {
            continue;
        };
        let values = children(var, "value", platform)
            .filter_map(|v| v.text())
            .map(|t| env_style(t.trim(), platform));
        vars.entry(name.to_string()).or_default().extend(values);
    }
    vars
}

/// `$APPDATA` and `${APPDATA}` written as `%APPDATA%`, and on Linux and
/// macOS a leading `~` (the home folder) as `%HOME%`. On Windows `~` is
/// not a folder: a definition naming it stays literal there.
fn env_style(path: &str, platform: Platform) -> String {
    let mut out = String::new();
    let path = match path.strip_prefix('~') {
        Some(rest)
            if platform != Platform::Windows && (rest.is_empty() || rest.starts_with('/')) =>
        {
            out.push_str("%HOME%");
            rest
        }
        _ => path,
    };
    let mut chars = path.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        // A `$$name$$` is one of the cleaner's own variables: leave it for
        // `substitute`, never read it as an environment variable.
        if chars.peek() == Some(&'$') {
            chars.next();
            out.push_str("$$");
            continue;
        }
        let braced = chars.peek() == Some(&'{');
        if braced {
            chars.next();
        }
        let mut name = String::new();
        while let Some(&n) = chars.peek() {
            if n.is_ascii_alphanumeric() || n == '_' {
                name.push(n);
                chars.next();
            } else {
                break;
            }
        }
        if braced && chars.peek() == Some(&'}') {
            chars.next();
        }
        if name.is_empty() {
            out.push('$');
        } else {
            out.push('%');
            out.push_str(&name);
            out.push('%');
        }
    }
    out
}

/// One `path` per value of each `$$var$$` it uses. A value may itself name
/// another variable, so this repeats until nothing changes; names are
/// visited in a fixed order so the actions are listed the same way every
/// run, whatever order the map happens to have.
fn substitute(path: &str, vars: &HashMap<String, Vec<String>>, platform: Platform) -> Vec<String> {
    let mut names: Vec<&String> = vars.keys().collect();
    names.sort();
    let mut results = vec![path.to_string()];
    for _ in 0..8 {
        let before = results.clone();
        for name in &names {
            let token = format!("$${name}$$");
            let values = &vars[*name];
            results = results
                .into_iter()
                .flat_map(|r| {
                    if r.contains(&token) {
                        values
                            .iter()
                            .map(|v| r.replace(&token, v))
                            .collect::<Vec<_>>()
                    } else {
                        vec![r]
                    }
                })
                .collect();
        }
        if results == before {
            break;
        }
    }
    results
        .into_iter()
        .map(|p| env_style(&p, platform))
        .collect()
}

fn read_action(
    node: Node,
    vars: &HashMap<String, Vec<String>>,
    platform: Platform,
) -> Result<Vec<Action>, String> {
    let command = node
        .attribute("command")
        .ok_or("an action without a command")?;
    if command == "winreg" && platform != Platform::Windows {
        return Err("registry actions apply to Windows only".into());
    }
    if command == "winreg" {
        let key = node.attribute("path").ok_or("winreg without a path")?;
        let value = node.attribute("name").map(str::to_string);
        return Ok(substitute(key, vars, platform)
            .into_iter()
            .map(|key| Action::Registry {
                key,
                value: value.clone(),
            })
            .collect());
    }
    if command == "win.shell.change.notify" {
        // Explorer is told after every clean of recent-file lists anyway.
        return Ok(Vec::new());
    }
    let path = node
        .attribute("path")
        .ok_or_else(|| format!("{command} without a path"))?;
    let search = match node.attribute("search").unwrap_or("file") {
        "file" => Search::File,
        "glob" => Search::Glob,
        "walk.files" => Search::WalkFiles,
        "walk.all" => Search::WalkAll,
        "walk.top" => Search::WalkTop,
        other => return Err(format!("search {other:?} is not supported")),
    };
    let make = |path: String| {
        let mut spec = FileSpec::new(path, search);
        spec.regex = node.attribute("regex").map(str::to_string);
        spec.nregex = node.attribute("nregex").map(str::to_string);
        spec.wholeregex = node.attribute("wholeregex").map(str::to_string);
        spec.nwholeregex = node.attribute("nwholeregex").map(str::to_string);
        spec.kind = match node.attribute("type") {
            Some("f") => Some(Kind::File),
            Some("d") => Some(Kind::Dir),
            _ => None,
        };
        spec
    };
    let sqlite = |op| -> Box<dyn Fn(FileSpec) -> Action> {
        Box::new(move |files| Action::Sqlite { files, op })
    };
    let build: Box<dyn Fn(FileSpec) -> Action> = match command {
        "delete" | "shred" => Box::new(Action::Delete),
        "truncate" => Box::new(Action::Truncate),
        "sqlite.vacuum" => sqlite(SqliteOp::Vacuum),
        "chrome.history" => sqlite(SqliteOp::ChromeHistory),
        "chrome.autofill" => sqlite(SqliteOp::ChromeAutofill),
        "chrome.keywords" => sqlite(SqliteOp::ChromeKeywords),
        "mozilla.url.history" | "mozilla_url_history" => sqlite(SqliteOp::MozillaHistory),
        "json" => {
            let address = node
                .attribute("address")
                .ok_or("json without an address")?
                .to_string();
            Box::new(move |files| Action::Json {
                files,
                address: address.clone(),
            })
        }
        "ini" => {
            let section = node
                .attribute("section")
                .ok_or("ini without a section")?
                .to_string();
            let parameter = node.attribute("parameter").map(str::to_string);
            Box::new(move |files| Action::Ini {
                files,
                section: section.clone(),
                parameter: parameter.clone(),
            })
        }
        other => return Err(format!("command {other:?} is not supported")),
    };
    Ok(substitute(path, vars, platform)
        .into_iter()
        .map(|p| build(make(p)))
        .collect())
}

#[cfg(test)]
#[path = "cleanerml_tests.rs"]
mod tests;
