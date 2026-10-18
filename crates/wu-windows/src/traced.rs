//! A traced install on the real machine: the ETW session from `trace`, the
//! installer it watches, and the conversion of kernel events into changes
//! named the way people name them.

use std::collections::HashMap;
use std::io;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use wu_core::model::{Hive, RegPath};
use wu_core::trace::{Change, Trace};
use wu_core::{Launch, Tracing};

use crate::trace::{Session, volumes};
use crate::trace_event::{FileOp, Raw, RegOp};

pub fn start(launch: &Launch) -> io::Result<Box<dyn Tracing>> {
    let (session, events) = Session::start(true)?;
    // Windows Installer may already be running as a service; its work for
    // this install happens there, not under the installer.
    for pid in crate::process::running("msiexec.exe") {
        session.track(pid);
    }
    let mut command = Command::new(&launch.program);
    if !launch.args.is_empty() {
        command.raw_arg(&launch.args);
    }
    if let Some(dir) = launch.program.parent().filter(|dir| dir.is_dir()) {
        command.current_dir(dir);
    }
    let child = command.spawn()?;
    session.track(child.id());
    Ok(Box::new(WindowsTracing {
        session: Some(session),
        events,
        child,
        raws: Vec::new(),
    }))
}

struct WindowsTracing {
    session: Option<Session>,
    events: Receiver<Raw>,
    child: Child,
    raws: Vec<Raw>,
}

impl Tracing for WindowsTracing {
    fn finished(&mut self) -> io::Result<bool> {
        self.raws.extend(self.events.try_iter());
        let root_done = self.child.try_wait()?.is_some();
        Ok(root_done
            && self
                .session
                .as_ref()
                .is_none_or(|session| session.live() == 0))
    }

    fn finish(mut self: Box<Self>) -> io::Result<Trace> {
        // Real-time buffers flush every second; let the last ones arrive.
        std::thread::sleep(Duration::from_millis(2500));
        let stats = match self.session.take() {
            Some(session) => session.stop()?,
            None => return Err(io::Error::other("the trace was already stopped")),
        };
        self.raws.extend(self.events.try_iter());
        let names = Names::new();
        let mut trace = convert(&mut self.raws, &names);
        trace.events_lost = stats.events_lost + stats.buffers_lost;
        Ok(trace)
    }
}

/// Kernel names to normal ones.
struct Names {
    volumes: Vec<(String, String)>,
    user_sid: Option<String>,
}

impl Names {
    fn new() -> Self {
        Self {
            volumes: volumes(),
            user_sid: crate::privilege::user_sid(),
        }
    }

    fn file(&self, path: &str) -> PathBuf {
        for (device, drive) in &self.volumes {
            if let Some(rest) = strip_prefix_ci(path, device) {
                return PathBuf::from(format!("{drive}{rest}"));
            }
        }
        PathBuf::from(path)
    }

    /// `None` for keys outside HKLM and the current user's hive.
    fn key(&self, key: &str) -> Option<RegPath> {
        if let Some(rest) = strip_prefix_ci(key, r"\REGISTRY\MACHINE") {
            return Some(RegPath::new(Hive::LocalMachine, rest));
        }
        let sid = self.user_sid.as_deref()?;
        if let Some(rest) = strip_prefix_ci(key, &format!(r"\REGISTRY\USER\{sid}_Classes")) {
            return Some(RegPath::new(
                Hive::CurrentUser,
                &format!(r"Software\Classes{rest}"),
            ));
        }
        strip_prefix_ci(key, &format!(r"\REGISTRY\USER\{sid}"))
            .map(|rest| RegPath::new(Hive::CurrentUser, rest))
    }
}

fn strip_prefix_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    let rest = &text[prefix.len()..];
    (head.eq_ignore_ascii_case(prefix) && (rest.is_empty() || rest.starts_with('\\')))
        .then_some(rest)
}

fn time_of(raw: &Raw) -> i64 {
    match raw {
        Raw::File { time, .. } | Raw::KeyOpened { time, .. } | Raw::Registry { time, .. } => *time,
        Raw::ProcessStart { .. } | Raw::ProcessStop { .. } => 0,
    }
}

/// Events in time order (they arrive per CPU), as changes. Set and delete
/// registry events name their key only by kernel object, so key paths are
/// rebuilt from the open and create events before them.
fn convert(raws: &mut [Raw], names: &Names) -> Trace {
    raws.sort_by_key(time_of);
    let mut trace = Trace::default();
    let mut keys: HashMap<u64, String> = HashMap::new();
    for raw in raws.iter() {
        match raw {
            Raw::ProcessStart { image, .. } => trace
                .processes
                .push(names.file(image).display().to_string()),
            Raw::ProcessStop { .. } => {}
            Raw::File {
                op,
                path,
                directory,
                ..
            } => {
                let path = names.file(path);
                trace.changes.push(match (op, directory) {
                    (FileOp::Created, true) => Change::FolderCreated { path },
                    (FileOp::Created, false) => Change::FileCreated { path },
                    (FileOp::Overwritten, _) => Change::FileReplaced { path },
                    (FileOp::Deleted, _) => Change::FileDeleted { path },
                    (FileOp::Renamed, _) => Change::FileRenamed { path },
                });
            }
            Raw::KeyOpened {
                object,
                base,
                base_name,
                relative,
                created,
                ..
            } => {
                let parent = keys
                    .get(base)
                    .cloned()
                    .or_else(|| (!base_name.is_empty()).then(|| base_name.clone()));
                let full = if relative.starts_with('\\') {
                    Some(relative.clone())
                } else {
                    parent.map(|parent| {
                        if relative.is_empty() {
                            parent
                        } else {
                            format!("{}\\{relative}", parent.trim_end_matches('\\'))
                        }
                    })
                };
                match full {
                    Some(full) => {
                        if *created {
                            match names.key(&full) {
                                Some(key) => trace.changes.push(Change::KeyCreated { key }),
                                None => trace.unnamed += 1,
                            }
                        }
                        keys.insert(*object, full);
                    }
                    None if *created => trace.unnamed += 1,
                    None => {}
                }
            }
            Raw::Registry {
                op,
                object,
                key,
                value,
                existed,
                ..
            } => {
                let full = if key.is_empty() {
                    keys.get(object).cloned()
                } else {
                    Some(key.clone())
                };
                let Some(key) = full.as_deref().and_then(|full| names.key(full)) else {
                    trace.unnamed += 1;
                    continue;
                };
                let name = value.clone().unwrap_or_default();
                trace.changes.push(match op {
                    RegOp::KeyDeleted => Change::KeyDeleted { key },
                    RegOp::ValueSet => Change::ValueSet {
                        key,
                        name,
                        existed: *existed,
                    },
                    RegOp::ValueDeleted => Change::ValueDeleted { key, name },
                });
            }
        }
    }
    trace
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Names {
        Names {
            volumes: vec![(r"\Device\HarddiskVolume3".into(), "C:".into())],
            user_sid: Some("S-1-5-21-1".into()),
        }
    }

    #[test]
    fn kernel_names_become_normal_ones() {
        let names = names();
        assert_eq!(
            names.file(r"\Device\HarddiskVolume3\Program Files\A"),
            PathBuf::from(r"C:\Program Files\A")
        );
        assert_eq!(
            names.file(r"\Device\HarddiskVolume30\A"),
            PathBuf::from(r"\Device\HarddiskVolume30\A")
        );
        let key = |text: &str| names.key(text).map(|key| key.display());
        assert_eq!(
            key(r"\REGISTRY\MACHINE\SOFTWARE\A").as_deref(),
            Some(r"HKLM\SOFTWARE\A")
        );
        assert_eq!(
            key(r"\REGISTRY\USER\S-1-5-21-1\Software\A").as_deref(),
            Some(r"HKCU\Software\A")
        );
        assert_eq!(
            key(r"\REGISTRY\USER\S-1-5-21-1_Classes\.x").as_deref(),
            Some(r"HKCU\Software\Classes\.x")
        );
        assert_eq!(
            key(r"\REGISTRY\USER\S-1-5-18\Software\A"),
            None,
            "other users are not named"
        );
    }

    #[test]
    fn key_paths_are_rebuilt_from_earlier_opens_in_time_order() {
        let mut raws = vec![
            Raw::Registry {
                pid: 1,
                time: 30,
                op: RegOp::ValueSet,
                object: 7,
                key: String::new(),
                value: Some("V".into()),
                existed: false,
            },
            Raw::KeyOpened {
                pid: 1,
                time: 10,
                object: 5,
                base: 0,
                base_name: String::new(),
                relative: r"\REGISTRY\MACHINE\SOFTWARE".into(),
                created: false,
            },
            Raw::KeyOpened {
                pid: 1,
                time: 20,
                object: 7,
                base: 5,
                base_name: String::new(),
                relative: r"Acme\Widget".into(),
                created: true,
            },
            Raw::Registry {
                pid: 1,
                time: 40,
                op: RegOp::ValueSet,
                object: 99,
                key: String::new(),
                value: Some("W".into()),
                existed: true,
            },
            Raw::File {
                pid: 1,
                time: 25,
                op: FileOp::Created,
                path: r"\Device\HarddiskVolume3\A".into(),
                directory: true,
            },
        ];
        let trace = convert(&mut raws, &names());
        let acme = RegPath::new(Hive::LocalMachine, r"SOFTWARE\Acme\Widget");
        assert_eq!(
            trace.changes,
            vec![
                Change::KeyCreated { key: acme.clone() },
                Change::FolderCreated {
                    path: PathBuf::from(r"C:\A")
                },
                Change::ValueSet {
                    key: acme,
                    name: "V".into(),
                    existed: false
                },
            ]
        );
        assert_eq!(
            trace.unnamed, 1,
            "a key opened before the trace has no name"
        );
    }
}
