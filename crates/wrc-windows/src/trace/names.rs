//! Key names as Windows' events give them (`\REGISTRY\MACHINE\…`, or
//! relative to a key opened earlier), and the keys the app shows for them.

use wrc_core::model::{Hive, RegPath};

/// What is known of a key object's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Name {
    Full(String),
    /// Beneath key object `base`, opened before tracing started: only
    /// Windows' handle table can name it (see `handles`).
    Under {
        base: u64,
        relative: String,
    },
}

impl Name {
    /// The name of a key opened relative to this one.
    pub fn join(&self, relative: &str) -> Self {
        match self {
            Self::Full(name) => Self::Full(join(name, relative)),
            Self::Under {
                base,
                relative: inner,
            } => Self::Under {
                base: *base,
                relative: join(inner, relative),
            },
        }
    }
}

pub fn join(parent: &str, relative: &str) -> String {
    if relative.is_empty() {
        parent.to_owned()
    } else if parent.is_empty() {
        relative.to_owned()
    } else {
        format!("{}\\{relative}", parent.trim_end_matches('\\'))
    }
}

/// The hives the app addresses, by their kernel names.
pub struct Hives {
    /// The signed-in account's SID, whose hive is HKCU.
    user_sid: Option<String>,
}

impl Hives {
    pub fn new(user_sid: Option<String>) -> Self {
        Self { user_sid }
    }

    /// A kernel key name as the app addresses it; `None` outside HKLM and
    /// the signed-in account's hives.
    pub fn key(&self, kernel: &str) -> Option<RegPath> {
        if let Some(rest) = strip_prefix_ci(kernel, r"\REGISTRY\MACHINE") {
            return Some(RegPath::new(Hive::LocalMachine, rest));
        }
        let sid = self.user_sid.as_deref()?;
        if let Some(rest) = strip_prefix_ci(kernel, &format!(r"\REGISTRY\USER\{sid}_Classes")) {
            return Some(RegPath::new(
                Hive::CurrentUser,
                &format!(r"Software\Classes{rest}"),
            ));
        }
        strip_prefix_ci(kernel, &format!(r"\REGISTRY\USER\{sid}"))
            .map(|rest| RegPath::new(Hive::CurrentUser, rest))
    }
}

fn strip_prefix_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    let rest = &text[prefix.len()..];
    (head.eq_ignore_ascii_case(prefix) && (rest.is_empty() || rest.starts_with('\\')))
        .then_some(rest)
}

/// `reg.exe` from `\Device\HarddiskVolume3\Windows\System32\reg.exe`.
pub fn file_name(image: &str) -> String {
    image.rsplit('\\').next().unwrap_or(image).to_owned()
}
