//! The registry cleaner: scan the chosen categories, then filter, de-duplicate
//! and number what was found. Nothing here changes the registry.

mod caches;
mod category;
mod classes;
mod machine;
mod programs;
mod roots;
#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

pub use category::{Category, Mode};
pub use roots::UserRoot;

use crate::check::PathCheck;
use crate::model::{Hive, RegPath};
use crate::system::System;

/// One entry the scan offers for removal: a whole key, or one value of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub id: u32,
    pub category: Category,
    pub key: RegPath,
    /// The value's name; `None` when the whole key is offered.
    pub value: Option<String>,
    /// Why it was offered, in plain words.
    pub detail: String,
    /// The other account whose registry it is in; `None` for this account
    /// and the machine.
    pub account: Option<String>,
    pub ticked: bool,
    /// Machine-wide and other accounts' entries need administrator rights.
    pub needs_admin: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub findings: Vec<Finding>,
    /// Things the person should know: accounts not read, keys not reachable.
    pub notes: Vec<String>,
    pub cancelled: bool,
}

/// What the person asked to scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub mode: Mode,
    /// The ticked categories, for [`Mode::Custom`].
    #[serde(default)]
    pub categories: Vec<Category>,
    /// Also scan every other account's registry.
    #[serde(default)]
    pub all_accounts: bool,
}

/// The cleaner settings a scan obeys.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    pub exclude_removable: bool,
    pub exclude_windows: bool,
    /// Keys never offered; `true` also covers everything beneath the key.
    pub excludes: Vec<(RegPath, bool)>,
    /// Keys whose contents the include-list category offers; `true` also
    /// offers their subkeys.
    pub includes: Vec<(RegPath, bool)>,
}

/// Shared with the page while a scan runs: what it is doing, and a way to
/// stop it.
#[derive(Debug, Default)]
pub struct Progress {
    cancel: AtomicBool,
    phase: Mutex<String>,
}

impl Progress {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn set_phase(&self, phase: &str) {
        *self
            .phase
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = phase.to_owned();
    }

    pub fn phase(&self) -> String {
        self.phase
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

/// Something a category found, before filtering and numbering.
#[derive(Debug, Clone)]
pub(crate) struct Found {
    pub category: Category,
    pub key: RegPath,
    pub value: Option<String>,
    pub detail: String,
    pub ticked: bool,
}

impl Found {
    pub fn key(category: Category, key: RegPath, detail: String) -> Self {
        Self {
            category,
            key,
            value: None,
            detail,
            ticked: true,
        }
    }

    pub fn value(category: Category, key: RegPath, name: &str, detail: String) -> Self {
        Self {
            category,
            key,
            value: Some(name.to_owned()),
            detail,
            ticked: true,
        }
    }

    pub fn unticked(mut self) -> Self {
        self.ticked = false;
        self
    }
}

/// What every category reads from.
pub(crate) struct Context<'a> {
    pub system: &'a dyn System,
    pub check: PathCheck<'a>,
    pub users: Vec<UserRoot>,
    pub options: &'a Options,
    pub progress: &'a Progress,
}

impl Context<'_> {
    /// `HKLM\SOFTWARE` and its 32-bit twin, then each account's `Software`,
    /// with whether the root is the 32-bit view.
    pub fn software_roots(&self) -> Vec<(RegPath, bool)> {
        let mut roots = vec![
            (RegPath::new(Hive::LocalMachine, "SOFTWARE"), false),
            (
                RegPath::new(Hive::LocalMachine, r"SOFTWARE\WOW6432Node"),
                true,
            ),
        ];
        roots.extend(self.users.iter().map(|user| (user.software.clone(), false)));
        roots
    }

    /// The machine's `Classes` key and each account's own.
    pub fn classes_roots(&self) -> Vec<RegPath> {
        let mut roots = vec![RegPath::new(Hive::LocalMachine, r"SOFTWARE\Classes")];
        roots.extend(self.users.iter().map(|user| user.classes.clone()));
        roots
    }
}

/// Scan the registry. Never changes it; other accounts' hives are closed
/// again before returning.
pub fn scan(
    system: &dyn System,
    request: &Request,
    options: &Options,
    progress: &Progress,
) -> Report {
    let mut notes = Vec::new();
    let users = roots::users(system, request.all_accounts, &mut notes);
    let context = Context {
        system,
        check: PathCheck::new(system, options.exclude_removable, options.exclude_windows),
        users,
        options,
        progress,
    };
    let mut found = Vec::new();
    let mut cancelled = false;
    for category in Category::for_mode(request.mode, &request.categories) {
        if progress.cancelled() {
            cancelled = true;
            break;
        }
        progress.set_phase(category.title());
        run(category, &context, &mut found);
    }
    if let Err(error) = system.release_account_hives() {
        notes.push(format!(
            "Another account's registry could not be closed: {error}"
        ));
    }
    Report {
        findings: finish(found, options, system.is_elevated(), true),
        notes,
        cancelled: cancelled || progress.cancelled(),
    }
}

fn run(category: Category, context: &Context, out: &mut Vec<Found>) {
    match category {
        Category::Startup => programs::startup(context, out),
        Category::AppPaths => programs::app_paths(context, out),
        Category::Uninstall => programs::uninstall(context, out),
        Category::Cache => caches::cache(context, out),
        Category::History => caches::history(context, out),
        Category::SystemSettings => caches::system_settings(context, out),
        Category::ComObjects => classes::com_objects(context, out),
        Category::ClassKeys => classes::class_keys(context, out),
        Category::ContextMenus => classes::context_menus(context, out),
        Category::Extensions => classes::extensions(context, out),
        Category::MissingSharedDlls => machine::shared_dlls(context, out, false),
        Category::UnusedSharedDlls => machine::shared_dlls(context, out, true),
        Category::MsiFolders => machine::msi_folders(context, out),
        Category::Firewall => machine::firewall(context, out),
        Category::Services => machine::services(context, out),
        Category::EmptyKeys => machine::empty_keys(context, out),
        Category::IncludeList => machine::include_list(context, out),
        Category::Search => {}
    }
}

/// Drop excluded entries and (with `drop_nested`) entries inside another
/// offered key, keep the first of duplicates, sort, number, and mark what
/// needs administrator rights. A search keeps nested matches so the person
/// can choose between a key and something inside it.
pub(crate) fn finish(
    found: Vec<Found>,
    options: &Options,
    elevated: bool,
    drop_nested: bool,
) -> Vec<Finding> {
    let excluded = |key: &RegPath| {
        options.excludes.iter().any(|(excluded, subkeys)| {
            key.folded() == excluded.folded() || (*subkeys && key.is_within(excluded))
        })
    };
    let whole_keys: Vec<RegPath> = found
        .iter()
        .filter(|item| item.value.is_none())
        .map(|item| item.key.clone())
        .collect();
    let inside_other = |item: &Found| {
        drop_nested
            && whole_keys.iter().any(|whole| {
                item.key.is_within(whole)
                    && (item.value.is_some() || item.key.folded() != whole.folded())
            })
    };
    let mut seen = HashSet::new();
    let mut kept: Vec<Found> = found
        .into_iter()
        .filter(|item| !excluded(&item.key) && !inside_other(item))
        .filter(|item| {
            let name = item.value.as_deref().map(str::to_lowercase);
            seen.insert((item.key.folded(), name))
        })
        .collect();
    kept.sort_by_cached_key(|item| {
        (
            item.category,
            item.key.folded(),
            item.value.as_deref().map(str::to_lowercase),
        )
    });
    kept.into_iter()
        .zip(1u32..)
        .map(|(item, id)| {
            let needs_admin = item.key.hive != Hive::CurrentUser;
            let account = item.key.account_part().map(|(account, _, _)| account);
            Finding {
                id,
                category: item.category,
                ticked: item.ticked && (elevated || !needs_admin),
                key: item.key,
                value: item.value,
                detail: item.detail,
                account,
                needs_admin,
            }
        })
        .collect()
}
