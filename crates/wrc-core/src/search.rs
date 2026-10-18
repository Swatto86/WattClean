//! Searching the registry for a word, as Registry Editor's Find does, so the
//! matches can be removed with a backup like any cleaner finding.

use serde::{Deserialize, Serialize};

use crate::keyname::parse_key;
use crate::model::{Hive, RegPath};
use crate::scan::{Category, Finding, Found, Options, Progress, Report, finish};
use crate::system::System;

/// Searches stop after this many matches; a wider search needs narrowing.
pub const MAX_MATCHES: usize = 5_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchRequest {
    pub text: String,
    /// Key to search beneath, as a person writes it; empty for the whole
    /// registry (this account's and the machine's).
    #[serde(default)]
    pub within: String,
    pub keys: bool,
    pub values: bool,
    pub data: bool,
    pub match_case: bool,
    pub whole: bool,
}

impl SearchRequest {
    /// The keys to search beneath, or why the request cannot run.
    pub fn roots(&self) -> Result<Vec<RegPath>, String> {
        let length = self.text.trim().chars().count();
        if length < 3 {
            return Err("Search for at least 3 characters.".into());
        }
        if length > 256 {
            return Err("Search for at most 256 characters.".into());
        }
        if !(self.keys || self.values || self.data) {
            return Err("Choose at least one of key names, value names and data.".into());
        }
        if self.within.trim().is_empty() {
            return Ok(vec![
                RegPath::new(Hive::LocalMachine, ""),
                RegPath::new(Hive::CurrentUser, ""),
            ]);
        }
        Ok(vec![parse_key(&self.within)?])
    }

    fn matches(&self, text: &str) -> bool {
        let wanted = self.text.trim();
        if self.match_case {
            if self.whole {
                text == wanted
            } else {
                text.contains(wanted)
            }
        } else {
            let text = text.to_lowercase();
            let wanted = wanted.to_lowercase();
            if self.whole {
                text == wanted
            } else {
                text.contains(&wanted)
            }
        }
    }
}

/// Links that would make the walk visit the same keys twice.
const LINKS: [&str; 2] = [r"SYSTEM\CurrentControlSet", r"SOFTWARE\WOW6432Node\Classes"];

/// Every key and value matching the request. Matches start unticked: the
/// person chooses what goes. Exclusions from the settings still apply.
pub fn search(
    system: &dyn System,
    request: &SearchRequest,
    options: &Options,
    progress: &Progress,
) -> Result<Report, String> {
    let roots = request.roots()?;
    let mut found: Vec<Found> = Vec::new();
    let mut notes = Vec::new();
    let mut visited = 0usize;
    let mut stack: Vec<RegPath> = roots.into_iter().rev().collect();
    while let Some(key) = stack.pop() {
        if progress.cancelled() {
            break;
        }
        if found.len() >= MAX_MATCHES {
            notes.push(format!(
                "The search stopped after {MAX_MATCHES} matches. Search for something more specific, or within a key."
            ));
            break;
        }
        visited += 1;
        if visited % 500 == 1 {
            progress.set_phase(&format!("Searching {}", key.display()));
        }
        if request.keys && !key.path.is_empty() && request.matches(key.name()) {
            found.push(
                Found::key(Category::Search, key.clone(), "Key name matches.".into()).unticked(),
            );
        }
        if request.values || request.data {
            for value in system.values(&key).unwrap_or_default() {
                let by_name = request.values && request.matches(&value.name);
                let by_data =
                    request.data && value.as_text().is_some_and(|text| request.matches(&text));
                if by_name || by_data {
                    let detail = if by_name {
                        "Value name matches."
                    } else {
                        "Value data matches."
                    };
                    found.push(
                        Found::value(Category::Search, key.clone(), &value.name, detail.into())
                            .unticked(),
                    );
                }
            }
        }
        let mut children = system.subkeys(&key).unwrap_or_default();
        children.reverse();
        for child in children {
            let next = key.child(&child);
            let link = key.hive == Hive::LocalMachine
                && LINKS
                    .iter()
                    .any(|link| next.path.eq_ignore_ascii_case(link));
            if !link {
                stack.push(next);
            }
        }
    }
    if let Err(error) = system.release_account_hives() {
        notes.push(format!(
            "Another account's registry could not be closed: {error}"
        ));
    }
    let findings: Vec<Finding> = finish(found, options, system.is_elevated(), false);
    Ok(Report {
        findings,
        notes,
        cancelled: progress.cancelled(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeSystem;
    use crate::model::RawValue;

    fn request(text: &str) -> SearchRequest {
        SearchRequest {
            text: text.into(),
            within: String::new(),
            keys: true,
            values: true,
            data: true,
            match_case: false,
            whole: false,
        }
    }

    #[test]
    fn finds_keys_value_names_and_data() {
        let dir = tempfile::tempdir().unwrap();
        let system = FakeSystem::empty(dir.path());
        let acme = RegPath::new(Hive::CurrentUser, r"Software\AcmeCorp");
        system.put(
            &acme,
            vec![
                RawValue::string("Path", r"C:\Acme"),
                RawValue::string("acmeflag", "x"),
            ],
        );
        system.put(
            &RegPath::new(Hive::LocalMachine, r"SOFTWARE\Other"),
            vec![RawValue::string("x", "y")],
        );
        let report = search(
            &system,
            &request("acme"),
            &Options::default(),
            &Progress::default(),
        )
        .unwrap();
        let shown: Vec<(String, Option<String>)> = report
            .findings
            .iter()
            .map(|finding| (finding.key.display(), finding.value.clone()))
            .collect();
        assert_eq!(
            shown,
            vec![
                (r"HKCU\Software\AcmeCorp".to_owned(), None),
                (
                    r"HKCU\Software\AcmeCorp".to_owned(),
                    Some("acmeflag".into())
                ),
                (r"HKCU\Software\AcmeCorp".to_owned(), Some("Path".into())),
            ]
        );
        assert!(report.findings.iter().all(|finding| !finding.ticked));
    }

    #[test]
    fn case_and_whole_string_narrow_the_match() {
        let mut wanted = request("Acme");
        wanted.match_case = true;
        assert!(wanted.matches("AcmeCorp") && !wanted.matches("acmecorp"));
        wanted.whole = true;
        assert!(wanted.matches("Acme") && !wanted.matches("AcmeCorp"));
    }

    #[test]
    fn refuses_short_or_empty_searches() {
        assert!(request("ab").roots().is_err());
        let mut nothing = request("acme");
        (nothing.keys, nothing.values, nothing.data) = (false, false, false);
        assert!(nothing.roots().is_err());
        let mut within = request("acme");
        within.within = r"HKCR\.txt".into();
        assert!(within.roots().is_err());
        within.within = r"HKCU\Software".into();
        assert_eq!(
            within.roots().unwrap(),
            vec![RegPath::new(Hive::CurrentUser, "Software")]
        );
    }
}
