//! The tracker names keys from earlier opens, applies events in time order,
//! keeps only writes beneath followed keys and names their processes.

use wrc_core::model::{Hive, RegPath};
use wrc_core::trace::WriteOp;

use std::collections::{HashMap, HashSet};

use super::event::Raw;
use super::tracker::{REORDER, Tracker, from_millis, to_millis};

const SID: &str = "S-1-5-21-1-2-3-1001";
const START: u64 = 1_700_000_000_000;

fn at(ms: u64) -> i64 {
    from_millis(START + ms)
}

fn nobody(_: u32) -> Option<String> {
    None
}

fn tracker(followed: &[RegPath]) -> Tracker {
    let mut tracker = Tracker::new(Some(SID.into()));
    tracker.follow(followed);
    tracker
}

fn open(pid: u32, object: u64, base: u64, base_name: &str, relative: &str) -> Raw {
    Raw::KeyOpened {
        pid,
        object,
        base,
        base_name: base_name.into(),
        relative: relative.into(),
        created: false,
    }
}

fn set(pid: u32, object: u64, value: &str) -> Raw {
    Raw::Registry {
        pid,
        op: WriteOp::ValueSet,
        object,
        key: String::new(),
        value: Some(value.into()),
        new: None,
        old: None,
    }
}

fn acme() -> RegPath {
    RegPath::new(Hive::CurrentUser, r"Software\Acme")
}

#[test]
fn times_convert_both_ways() {
    assert_eq!(to_millis(from_millis(START)), START);
}

#[test]
fn a_write_is_named_through_the_key_it_was_opened_relative_to() {
    let mut tracker = tracker(&[acme()]);
    let user = format!(r"\REGISTRY\USER\{SID}");
    tracker.push(at(0), at(0), open(9, 1, 0, "", &user), &nobody);
    tracker.push(
        at(1),
        at(1),
        open(9, 2, 1, "", r"software\acme\sub"),
        &nobody,
    );
    tracker.push(at(2), at(2), set(9, 2, "Size"), &nobody);
    tracker.push(
        at(3),
        at(3),
        open(9, 3, 0, &user, r"Software\Other"),
        &nobody,
    );
    tracker.push(at(4), at(4), set(9, 3, "Size"), &nobody);
    tracker.apply_all(&nobody);
    let writes = tracker.writes(START, START + 10);
    assert_eq!(writes.len(), 1, "only beneath the followed key: {writes:?}");
    assert_eq!(writes[0].key.folded(), r"HKCU\software\acme\sub");
    assert_eq!(writes[0].value.as_deref(), Some("Size"));
    assert_eq!(writes[0].time, START + 2);
}

#[test]
fn events_out_of_order_are_applied_by_time_once_settled() {
    let machine = RegPath::new(Hive::LocalMachine, r"SOFTWARE\Acme");
    let mut tracker = tracker(&[machine]);
    tracker.push(
        at(0),
        at(0),
        Raw::ProcessStart {
            pid: 7,
            image: r"\Device\HarddiskVolume3\Windows\System32\reg.exe".into(),
        },
        &nobody,
    );
    // The set arrives before the open it depends on (another processor).
    tracker.push(at(20), at(20), set(7, 5, ""), &nobody);
    tracker.push(
        at(10),
        at(10),
        open(7, 5, 0, "", r"\REGISTRY\MACHINE\SOFTWARE\Acme"),
        &nobody,
    );
    tracker.push(at(30), at(30), Raw::KeyClosed { object: 5 }, &nobody);
    tracker.push(at(31), at(31), Raw::ProcessStop { pid: 7 }, &nobody);
    assert!(
        tracker.writes(START, START + 100).is_empty(),
        "still waiting"
    );
    let later = u64::try_from(REORDER / 10_000).unwrap() + 100;
    tracker.push(at(later), at(later), Raw::KeyClosed { object: 99 }, &nobody);
    let writes = tracker.writes(START, START + 100);
    assert_eq!(writes.len(), 1, "{writes:?}");
    assert_eq!(writes[0].actor.process, "reg.exe");
    assert_eq!(writes[0].actor.pid, 7);
    assert!(tracker.settled(START + 30));
    assert!(
        !tracker.settled(START + later),
        "the newest has not settled"
    );
}

#[test]
fn drift_between_the_event_clock_and_the_system_clock_is_corrected() {
    let mut tracker = tracker(&[acme()]);
    let user = format!(r"\REGISTRY\USER\{SID}");
    // Every event reaches the system clock two seconds after its own time.
    let late = |ms: u64| at(ms + 2_000);
    for second in 0..12 {
        let ms = second * 1_000;
        tracker.push(at(ms), late(ms), Raw::KeyClosed { object: 0 }, &nobody);
    }
    tracker.push(
        at(12_000),
        late(12_000),
        open(3, 9, 0, &user, r"Software\Acme"),
        &nobody,
    );
    tracker.push(at(12_001), late(12_001), set(3, 9, "Size"), &nobody);
    tracker.apply_all(&nobody);
    let writes = tracker.writes(START, START + 20_000);
    assert_eq!(writes.len(), 1, "{writes:?}");
    assert_eq!(writes[0].time, START + 14_001, "shown by the system clock");
}

#[test]
fn creations_classes_and_processes_started_before_tracing() {
    let classes = RegPath::new(Hive::CurrentUser, r"Software\Classes\.acme");
    let mut tracker = tracker(&[classes]);
    let earlier = |pid: u32| (pid == 42).then(|| r"C:\Tools\setup.exe".to_owned());
    tracker.push(
        at(0),
        at(0),
        Raw::KeyOpened {
            pid: 42,
            object: 8,
            base: 0,
            base_name: format!(r"\REGISTRY\USER\{SID}_Classes"),
            relative: r".acme\shell".into(),
            created: true,
        },
        &earlier,
    );
    tracker.push(
        at(1),
        at(1),
        Raw::Registry {
            pid: 4,
            op: WriteOp::KeyDeleted,
            object: 0,
            key: format!(r"\REGISTRY\USER\{SID}_Classes\.acme\old"),
            value: None,
            new: None,
            old: None,
        },
        &earlier,
    );
    tracker.apply_all(&earlier);
    let writes = tracker.writes(START, START + 10);
    let summary: Vec<(WriteOp, String, String)> = writes
        .iter()
        .map(|write| (write.op, write.key.display(), write.actor.process.clone()))
        .collect();
    assert_eq!(
        summary,
        vec![
            (
                WriteOp::KeyCreated,
                r"HKCU\Software\Classes\.acme\shell".into(),
                "setup.exe".into()
            ),
            (
                WriteOp::KeyDeleted,
                r"HKCU\Software\Classes\.acme\old".into(),
                "System".into()
            ),
        ]
    );
}

#[test]
fn a_key_beneath_a_handle_from_before_tracing_is_named_from_the_handle_table() {
    let mut tracker = tracker(&[acme()]);
    // Opened relative to handle 77, which predates tracing.
    tracker.push(at(0), at(0), open(1, 6, 77, "", r"Acme\Sub"), &nobody);
    tracker.push(at(1), at(1), set(1, 6, "Size"), &nobody);
    tracker.apply_all(&nobody);
    assert!(
        tracker.writes(START, START + 10).is_empty(),
        "not yet named"
    );
    let asked = tracker.unnamed(START, START + 10);
    assert_eq!(asked, HashSet::from([77]));
    let found = HashMap::from([(77, format!(r"\REGISTRY\USER\{SID}\Software"))]);
    tracker.named(&asked, &found);
    let writes = tracker.writes(START, START + 10);
    assert_eq!(writes.len(), 1, "{writes:?}");
    assert_eq!(writes[0].key.display(), r"HKCU\Software\Acme\Sub");
    // The name is kept: the next write needs no lookup.
    tracker.push(at(2), at(2), set(1, 6, "Other"), &nobody);
    tracker.apply_all(&nobody);
    assert!(tracker.unnamed(START, START + 10).is_empty());
    assert_eq!(tracker.writes(START, START + 10).len(), 2);
}

#[test]
fn a_handle_the_table_cannot_name_is_given_up_and_a_closed_one_dropped() {
    let mut tracker = tracker(&[acme()]);
    tracker.push(at(0), at(0), set(2, 50, "Size"), &nobody);
    tracker.apply_all(&nobody);
    let asked = tracker.unnamed(START, START + 10);
    assert_eq!(asked, HashSet::from([50]));
    tracker.named(&asked, &HashMap::new());
    tracker.push(at(1), at(1), set(2, 50, "Size"), &nobody);
    tracker.apply_all(&nobody);
    assert!(
        tracker.unnamed(START, START + 10).is_empty(),
        "not asked again"
    );
    // A handle closed before it was looked up: its address may be reused.
    tracker.push(at(2), at(2), set(2, 60, "Size"), &nobody);
    tracker.push(at(3), at(3), Raw::KeyClosed { object: 60 }, &nobody);
    tracker.apply_all(&nobody);
    assert!(tracker.unnamed(START, START + 10).is_empty());
}

#[test]
fn a_reused_object_forgets_its_old_name() {
    let mut tracker = tracker(&[acme()]);
    let user = format!(r"\REGISTRY\USER\{SID}");
    tracker.push(
        at(0),
        at(0),
        open(1, 6, 0, &user, r"Software\Acme"),
        &nobody,
    );
    // Reopened relative to a handle from before tracing: no name.
    tracker.push(at(1), at(1), open(1, 6, 77, "", r"Elsewhere"), &nobody);
    tracker.push(at(2), at(2), set(1, 6, "Size"), &nobody);
    tracker.apply_all(&nobody);
    assert!(tracker.writes(START, START + 10).is_empty());
}

/// A SetValueKey event's data, laid out as Windows' manifest declares it.
fn set_value_event(
    data: &mut Vec<u8>,
) -> windows_sys::Win32::System::Diagnostics::Etw::EVENT_RECORD {
    let utf16 = |text: &str| -> Vec<u8> {
        text.encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect()
    };
    data.extend(0x1234_u64.to_le_bytes()); // KeyObject
    data.extend(0_u32.to_le_bytes()); // Status
    data.extend(4_u32.to_le_bytes()); // Type: REG_DWORD
    data.extend(4_u32.to_le_bytes()); // DataSize
    data.extend(utf16("")); // KeyName
    data.extend(utf16("Size")); // ValueName
    data.extend(4_u16.to_le_bytes()); // CapturedDataSize
    data.extend(2_u32.to_le_bytes()); // CapturedData
    data.extend(4_u32.to_le_bytes()); // PreviousDataType
    data.extend(4_u32.to_le_bytes()); // PreviousDataSize
    data.extend(4_u16.to_le_bytes()); // PreviousDataCapturedSize
    data.extend(1_u32.to_le_bytes()); // PreviousData
    // SAFETY: an all-zero EVENT_RECORD is a valid empty record.
    let mut record: windows_sys::Win32::System::Diagnostics::Etw::EVENT_RECORD =
        unsafe { std::mem::zeroed() };
    record.EventHeader.EventDescriptor.Id = 5;
    record.EventHeader.ProcessId = 42;
    record.UserData = data.as_mut_ptr().cast();
    record.UserDataLength = u16::try_from(data.len()).unwrap();
    record
}

#[test]
fn a_value_sets_data_and_what_it_replaced_are_read_from_the_event() {
    let mut data = Vec::new();
    let record = set_value_event(&mut data);
    let Some(Raw::Registry {
        pid,
        op,
        object,
        value,
        new,
        old,
        ..
    }) = super::event::registry(&record)
    else {
        panic!("not decoded");
    };
    assert_eq!((pid, op, object), (42, WriteOp::ValueSet, 0x1234));
    assert_eq!(value.as_deref(), Some("Size"));
    assert_eq!(new.unwrap().bytes, 2_u32.to_le_bytes());
    assert_eq!(old.unwrap().bytes, 1_u32.to_le_bytes());
}
