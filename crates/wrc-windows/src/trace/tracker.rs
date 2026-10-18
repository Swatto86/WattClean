//! What the trace has seen, applied in time order. Windows delivers events
//! per processor, so each waits a moment before it is applied; set and
//! delete events name their key only by a kernel key object, so the name
//! of every key object opened or created while tracing is kept until it is
//! closed. A key reached through a handle opened before tracing started is
//! kept as a write waiting for its name, which `handles` finds when the
//! watcher asks. Event times come from the processor's counter, which
//! drifts from the (time-synchronised) system clock over hours; the
//! shortest delivery delay seen in each window of time measures that drift.

use std::collections::{HashMap, HashSet, VecDeque};

use wrc_core::model::RegPath;
use wrc_core::trace::{Actor, Captured, Write, WriteOp};

use super::event::Raw;
use super::names::{Hives, Name, file_name, join};

/// Event times are FILETIMEs: 100 ns units since 1601.
const MS: i64 = 10_000;
const UNIX_EPOCH: i64 = 116_444_736_000_000_000;
/// How long an event waits for earlier ones from other processors.
pub const REORDER: i64 = 300 * MS;
/// How often waiting events are sorted and applied.
const APPLY_EVERY: i64 = 50 * MS;
/// Writes, and processes that have ended, are kept this long.
const KEEP: i64 = 60_000 * MS;
const MAX_WRITES: usize = 4_096;
/// More open keys than this means close events were lost: start afresh.
const MAX_OBJECTS: usize = 500_000;
const SYSTEM_PID: u32 = 4;
/// How often the event clock is measured against the system clock.
const DRIFT_WINDOW: i64 = 10_000 * MS;

pub fn to_millis(time: i64) -> u64 {
    u64::try_from((time - UNIX_EPOCH) / MS).unwrap_or(0)
}

pub fn from_millis(millis: u64) -> i64 {
    i64::try_from(millis)
        .ok()
        .and_then(|millis| millis.checked_mul(MS))
        .map_or(i64::MAX, |time| time.saturating_add(UNIX_EPOCH))
}

struct Process {
    name: String,
    ended: Option<i64>,
}

/// A write through a key beneath `base`, a handle opened before tracing.
struct Pending {
    /// Milliseconds since the Unix epoch, by the system clock.
    time: u64,
    actor: Actor,
    base: u64,
    relative: String,
    what: What,
}

/// What a write did, apart from where.
struct What {
    op: WriteOp,
    value: Option<String>,
    new: Option<Captured>,
    old: Option<Captured>,
}

pub struct Tracker {
    hives: Hives,
    followed: Vec<RegPath>,
    waiting: Vec<(i64, Raw)>,
    /// The newest event time delivered.
    latest: i64,
    /// Every event up to this time has been applied.
    applied: i64,
    last_apply: i64,
    /// Add to an event time for the system clock's time.
    offset: i64,
    /// The shortest delay, by the two clocks, in the current window.
    shortest: i64,
    window_start: i64,
    objects: HashMap<u64, Name>,
    processes: HashMap<u32, Process>,
    writes: VecDeque<Write>,
    pending: VecDeque<Pending>,
    /// Base objects the handle table could not name (a protected process,
    /// or already closed): writes beneath them are not kept.
    lost: HashSet<u64>,
}

impl Tracker {
    pub fn new(user_sid: Option<String>) -> Self {
        Self {
            hives: Hives::new(user_sid),
            followed: Vec::new(),
            waiting: Vec::new(),
            latest: 0,
            applied: 0,
            last_apply: 0,
            offset: 0,
            shortest: i64::MAX,
            window_start: 0,
            objects: HashMap::new(),
            processes: HashMap::new(),
            writes: VecDeque::new(),
            pending: VecDeque::new(),
            lost: HashSet::new(),
        }
    }

    pub fn follow(&mut self, keys: &[RegPath]) {
        self.followed = keys.to_vec();
    }

    /// Take one event, made at `time` by the event clock and delivered at
    /// `received` by the system clock; `image_of` names a process that
    /// started before tracing did.
    pub fn push(
        &mut self,
        time: i64,
        received: i64,
        raw: Raw,
        image_of: &dyn Fn(u32) -> Option<String>,
    ) {
        if received - self.window_start >= DRIFT_WINDOW {
            if self.shortest != i64::MAX {
                self.offset = self.shortest;
            }
            self.shortest = i64::MAX;
            self.window_start = received;
        }
        self.shortest = self.shortest.min(received - time);
        self.latest = self.latest.max(time);
        self.waiting.push((time, raw));
        if self.latest - self.last_apply >= APPLY_EVERY {
            self.last_apply = self.latest;
            self.apply_until(self.latest - REORDER, image_of);
        }
    }

    /// Apply every waiting event up to `until`, oldest first.
    pub fn apply_until(&mut self, until: i64, image_of: &dyn Fn(u32) -> Option<String>) {
        // Nearly sorted already, which the stable sort handles quickly.
        self.waiting.sort_by_key(|(time, _)| *time);
        let ready = self.waiting.partition_point(|(time, _)| *time <= until);
        let events: Vec<(i64, Raw)> = self.waiting.drain(..ready).collect();
        for (time, raw) in events {
            self.apply(time, raw, image_of);
        }
        self.applied = self.applied.max(until.min(self.latest));
        let horizon = to_millis(self.applied - KEEP + self.offset);
        while self
            .writes
            .front()
            .is_some_and(|write| write.time < horizon)
        {
            self.writes.pop_front();
        }
        while self
            .pending
            .front()
            .is_some_and(|write| write.time < horizon)
        {
            self.pending.pop_front();
        }
        let ended = self.applied - KEEP;
        self.processes
            .retain(|_, process| process.ended.is_none_or(|time| time >= ended));
    }

    /// Apply everything delivered so far.
    pub fn apply_all(&mut self, image_of: &dyn Fn(u32) -> Option<String>) {
        self.apply_until(i64::MAX, image_of);
    }

    /// Whether every event made until `to` (system clock, ms since 1970)
    /// has been applied.
    pub fn settled(&self, to: u64) -> bool {
        self.applied >= from_millis(to).saturating_sub(self.offset)
    }

    pub fn writes(&self, from: u64, to: u64) -> Vec<Write> {
        self.writes
            .iter()
            .filter(|write| (from..=to).contains(&write.time))
            .cloned()
            .collect()
    }

    /// The handles opened before tracing that writes from `from` to `to`
    /// went through, for `handles::names`.
    pub fn unnamed(&self, from: u64, to: u64) -> HashSet<u64> {
        self.pending
            .iter()
            .filter(|write| (from..=to).contains(&write.time))
            .map(|write| write.base)
            .collect()
    }

    /// Take the names the handle table gave for the `asked` handles. Writes
    /// beneath a handle it could not name are dropped, and later ones too.
    pub fn named(&mut self, asked: &HashSet<u64>, found: &HashMap<u64, String>) {
        for name in self.objects.values_mut() {
            if let Name::Under { base, relative } = name
                && let Some(full) = found.get(base)
            {
                *name = Name::Full(join(full, relative));
            }
        }
        for (base, full) in found {
            self.objects.insert(*base, Name::Full(full.clone()));
        }
        let (ready, waiting): (Vec<Pending>, Vec<Pending>) = self
            .pending
            .drain(..)
            .partition(|write| asked.contains(&write.base));
        self.pending = waiting.into();
        for write in ready {
            let Some(full) = found.get(&write.base) else {
                continue;
            };
            let kernel = join(full, &write.relative);
            self.keep(write.time, write.actor, &kernel, write.what);
        }
        self.writes
            .make_contiguous()
            .sort_by_key(|write| write.time);
        self.lost
            .extend(asked.iter().filter(|base| !found.contains_key(base)));
    }

    fn apply(&mut self, time: i64, raw: Raw, image_of: &dyn Fn(u32) -> Option<String>) {
        match raw {
            Raw::ProcessStart { pid, image } => {
                let name = file_name(&image);
                self.processes.insert(pid, Process { name, ended: None });
            }
            Raw::ProcessStop { pid } => {
                if let Some(process) = self.processes.get_mut(&pid) {
                    process.ended = Some(time);
                }
            }
            Raw::KeyOpened {
                pid,
                object,
                base,
                base_name,
                relative,
                created,
            } => {
                self.lost.remove(&object);
                let name = if relative.starts_with('\\') {
                    Name::Full(relative)
                } else if let Some(parent) = self.objects.get(&base) {
                    parent.join(&relative)
                } else if !base_name.is_empty() {
                    Name::Full(join(&base_name, &relative))
                } else if base != 0 {
                    Name::Under { base, relative }
                } else {
                    // A reused object must not keep an older key's name.
                    self.objects.remove(&object);
                    return;
                };
                if created {
                    let what = What {
                        op: WriteOp::KeyCreated,
                        value: None,
                        new: None,
                        old: None,
                    };
                    self.wrote(time, pid, &name, what, image_of);
                }
                if self.objects.len() >= MAX_OBJECTS {
                    self.objects.clear();
                }
                self.objects.insert(object, name);
            }
            Raw::KeyClosed { object } => {
                self.objects.remove(&object);
                self.lost.remove(&object);
                // Its address may name another key next: never guess.
                self.pending.retain(|write| write.base != object);
            }
            Raw::Registry {
                pid,
                op,
                object,
                key,
                value,
                new,
                old,
            } => {
                let name = if !key.is_empty() {
                    Name::Full(key)
                } else if let Some(name) = self.objects.get(&object) {
                    name.clone()
                } else {
                    Name::Under {
                        base: object,
                        relative: String::new(),
                    }
                };
                let what = What {
                    op,
                    value,
                    new,
                    old,
                };
                self.wrote(time, pid, &name, what, image_of);
            }
        }
    }

    fn wrote(
        &mut self,
        time: i64,
        pid: u32,
        name: &Name,
        what: What,
        image_of: &dyn Fn(u32) -> Option<String>,
    ) {
        let time = to_millis(time + self.offset);
        match name {
            Name::Full(kernel) => {
                if self.followed_key(kernel).is_some() {
                    let actor = self.actor(pid, image_of);
                    self.keep(time, actor, kernel, what);
                }
            }
            Name::Under { base, relative } => {
                if self.lost.contains(base) || self.followed.is_empty() {
                    return;
                }
                let actor = self.actor(pid, image_of);
                self.pending.push_back(Pending {
                    time,
                    actor,
                    base: *base,
                    relative: relative.clone(),
                    what,
                });
                if self.pending.len() > MAX_WRITES {
                    self.pending.pop_front();
                }
            }
        }
    }

    /// The key a kernel name stands for, when it is beneath a followed key.
    fn followed_key(&self, kernel: &str) -> Option<RegPath> {
        self.hives
            .key(kernel)
            .filter(|key| self.followed.iter().any(|followed| key.is_within(followed)))
    }

    fn keep(&mut self, time: u64, actor: Actor, kernel: &str, what: What) {
        let Some(key) = self.followed_key(kernel) else {
            return;
        };
        self.writes.push_back(Write {
            time,
            actor,
            op: what.op,
            key,
            value: what.value,
            new: what.new,
            old: what.old,
        });
        if self.writes.len() > MAX_WRITES {
            self.writes.pop_front();
        }
    }

    fn actor(&mut self, pid: u32, image_of: &dyn Fn(u32) -> Option<String>) -> Actor {
        let process = if pid == SYSTEM_PID {
            "System".into()
        } else if let Some(process) = self.processes.get(&pid) {
            process.name.clone()
        } else if let Some(image) = image_of(pid) {
            let name = file_name(&image);
            self.processes.insert(
                pid,
                Process {
                    name: name.clone(),
                    ended: None,
                },
            );
            name
        } else {
            String::new()
        };
        Actor { pid, process }
    }
}
