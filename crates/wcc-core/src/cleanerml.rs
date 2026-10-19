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
            .map(|t| env_style(t.trim()));
        vars.entry(name.to_string()).or_default().extend(values);
    }
    vars
}

/// `$APPDATA` and `${APPDATA}` written as `%APPDATA%`, and a leading `~`
/// (the home folder on Linux and macOS) as `%HOME%`.
fn env_style(path: &str) -> String {
    let mut out = String::new();
    let path = match path.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => {
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

/// One `path` per value of each `$$var$$` it uses.
fn substitute(path: &str, vars: &HashMap<String, Vec<String>>) -> Vec<String> {
    let mut results = vec![path.to_string()];
    for (name, values) in vars {
        let token = format!("$${name}$$");
        if !path.contains(&token) {
            continue;
        }
        let token = &token;
        results = results
            .iter()
            .flat_map(|r| values.iter().map(move |v| r.replace(token, v)))
            .collect();
    }
    results.into_iter().map(|p| env_style(&p)).collect()
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
        return Ok(vec![Action::Registry {
            key: key.to_string(),
            value: node.attribute("name").map(str::to_string),
        }]);
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
    Ok(substitute(path, vars)
        .into_iter()
        .map(|p| build(make(p)))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<cleaner id="example" os="windows">
  <label>Example</label>
  <description>An example program</description>
  <running type="exe">Example.exe</running>
  <var name="base">
    <value>$APPDATA\Example</value>
    <value>${LOCALAPPDATA}\Example</value>
  </var>
  <option id="cache">
    <label>Cache</label>
    <description>Delete the cache</description>
    <action command="delete" search="walk.files" path="$$base$$\Cache" nregex="\.keep$"/>
    <action command="delete" search="file" path="~/.example" os="linux"/>
  </option>
  <option id="mru">
    <label>Recent files</label>
    <warning>Your recent list goes.</warning>
    <action command="winreg" path="HKCU\Software\Example" name="MRU"/>
    <action command="ini" search="file" path="%APPDATA%\Example\ex.ini" section="Recent"/>
    <action command="chrome.history" search="file" path="%APPDATA%\Example\History"/>
    <action command="teleport" search="file" path="x"/>
  </option>
  <option id="linux" os="linux">
    <label>Linux only</label>
    <action command="delete" search="file" path="~/.x"/>
  </option>
</cleaner>"#;

    #[test]
    fn reads_options_actions_and_variables() {
        let (cleaner, notes) = parse(SAMPLE, Platform::Windows).unwrap();
        assert_eq!(cleaner.id, "ml-example");
        assert_eq!(cleaner.name, "Example");
        assert_eq!(cleaner.running, vec!["example.exe".to_string()]);
        assert_eq!(cleaner.options.len(), 2);
        let cache = &cleaner.options[0];
        let paths: Vec<_> = cache
            .actions
            .iter()
            .map(|a| match a {
                Action::Delete(spec) => spec.path.clone(),
                _ => panic!(),
            })
            .collect();
        assert_eq!(
            paths,
            vec![
                r"%APPDATA%\Example\Cache".to_string(),
                r"%LOCALAPPDATA%\Example\Cache".to_string()
            ]
        );
        let mru = &cleaner.options[1];
        assert_eq!(mru.warning.as_deref(), Some("Your recent list goes."));
        assert_eq!(
            mru.actions[0],
            Action::Registry {
                key: r"HKCU\Software\Example".into(),
                value: Some("MRU".into())
            }
        );
        assert!(matches!(
            mru.actions[2],
            Action::Sqlite {
                op: SqliteOp::ChromeHistory,
                ..
            }
        ));
        assert_eq!(
            notes,
            vec![r#"mru: command "teleport" is not supported"#.to_string()]
        );
    }

    #[test]
    fn refuses_what_is_not_a_windows_cleaner() {
        assert!(parse("<nope/>", Platform::Windows).is_err());
        let linux = r#"<cleaner id="x" os="linux"><option id="a"/></cleaner>"#;
        assert!(parse(linux, Platform::Windows).is_err());
        assert!(parse("not xml", Platform::Windows).is_err());
    }

    /// BleachBit's own cleaners are mostly Linux ones.
    #[test]
    fn linux_reads_its_own_parts_with_the_home_folder() {
        let text = r#"<cleaner id="gedit">
  <label>gedit</label>
  <option id="cache">
    <label>Cache</label>
    <action command="delete" search="walk.all" path="~/.cache/gedit"/>
    <action command="delete" search="file" path="$XDG_CONFIG_HOME/gedit/recent" os="linux"/>
    <action command="delete" search="file" path="$APPDATA\gedit\cache" os="windows"/>
    <action command="winreg" path="HKCU\Software\gedit"/>
  </option>
</cleaner>"#;
        let (cleaner, notes) = parse(text, Platform::Linux).unwrap();
        let paths: Vec<_> = cleaner.options[0]
            .actions
            .iter()
            .filter_map(|a| match a {
                Action::Delete(spec) => Some(spec.path.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            paths,
            ["%HOME%/.cache/gedit", "%XDG_CONFIG_HOME%/gedit/recent"]
        );
        assert_eq!(notes, ["cache: registry actions apply to Windows only"]);
        let (windows, _) = parse(text, Platform::Windows).unwrap();
        assert_eq!(windows.options[0].actions.len(), 3, "~ stays literal there");
    }
}
