//! Tweaks switch on, and off again to exactly what was there before.

use super::*;
use crate::fake::FakeSystem;

fn machine() -> (tempfile::TempDir, FakeSystem) {
    let dir = tempfile::tempdir().unwrap();
    let system = FakeSystem::seeded(&dir.path().join("machine")).unwrap();
    (dir, system)
}

fn advanced() -> RegPath {
    RegPath::new(
        Hive::CurrentUser,
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced",
    )
}

#[test]
fn every_tweak_parses_and_has_an_unique_id() {
    let mut ids: Vec<&str> = TWEAKS.iter().map(|tweak| tweak.id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), TWEAKS.len());
    for tweak in &TWEAKS {
        assert!(
            !tweak.on.is_empty() && !tweak.off.is_empty(),
            "{}",
            tweak.id
        );
        for change in tweak.on.iter().chain(tweak.off) {
            let key = key_of(change).unwrap();
            assert!(!key.path.is_empty(), "{}", tweak.id);
        }
    }
}

#[test]
fn off_puts_back_the_value_that_was_there() {
    let (dir, system) = machine();
    let shown = |id| {
        list(&system, dir.path())
            .unwrap()
            .into_iter()
            .find(|t| t.id == id)
            .unwrap()
    };
    assert!(
        !shown("showExtensions").on,
        "the test machine hides extensions"
    );
    let on = set(&system, dir.path(), "showExtensions", true).unwrap();
    assert!(on.on && on.changed_by_app);
    assert_eq!(
        find_value(&system.values_of(&advanced()), "HideFileExt")
            .unwrap()
            .as_u32(),
        Some(0)
    );
    let off = set(&system, dir.path(), "showExtensions", false).unwrap();
    assert!(!off.on && !off.changed_by_app);
    assert_eq!(
        find_value(&system.values_of(&advanced()), "HideFileExt")
            .unwrap()
            .as_u32(),
        Some(1)
    );
}

#[test]
fn off_removes_a_value_and_keys_the_app_created() {
    let (dir, system) = machine();
    let clsid = RegPath::new(
        Hive::CurrentUser,
        r"Software\Classes\CLSID\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}",
    );
    set(&system, dir.path(), "classicContextMenu", true).unwrap();
    assert!(system.has_key(&clsid.child("InprocServer32")));
    assert!(is_on(&system, find("classicContextMenu").unwrap()).unwrap());
    set(&system, dir.path(), "classicContextMenu", false).unwrap();
    assert!(!system.has_key(&clsid), "the key the tweak created is gone");
    assert!(system.has_key(&RegPath::new(Hive::CurrentUser, r"Software\Classes")));
}

#[test]
fn a_tweak_already_on_is_switched_off_to_windows_default() {
    let (dir, system) = machine();
    system.put(
        &advanced(),
        vec![RawValue::dword("ShowSecondsInSystemClock", 1)],
    );
    let view = list(&system, dir.path())
        .unwrap()
        .into_iter()
        .find(|t| t.id == "secondsInClock")
        .unwrap();
    assert!(view.on && !view.changed_by_app);
    set(&system, dir.path(), "secondsInClock", false).unwrap();
    assert!(find_value(&system.values_of(&advanced()), "ShowSecondsInSystemClock").is_none());
}

#[test]
fn machine_tweaks_need_administrator_rights() {
    let (dir, system) = machine();
    system.set_elevated(false);
    let error = set(&system, dir.path(), "verboseStopScreen", true).unwrap_err();
    assert!(error.to_string().contains("administrator"), "{error}");
    assert!(
        set(&system, dir.path(), "secondsInClock", true).is_ok(),
        "per-account tweaks still work"
    );
}

#[test]
fn windows_10_cannot_switch_windows_11_tweaks() {
    let (dir, system) = machine();
    system.put(
        &RegPath::new(
            Hive::LocalMachine,
            r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
        ),
        vec![RawValue::string("CurrentBuildNumber", "19045")],
    );
    assert_eq!(windows_build(&system), Some(19_045));
    assert!(set(&system, dir.path(), "taskbarLeft", true).is_err());
    assert!(set(&system, dir.path(), "classicVolume", true).is_ok());
    assert!(set(&system, dir.path(), "nonsense", true).is_err());
}

#[test]
fn reset_all_reverts_every_change_the_app_made() {
    let (dir, system) = machine();
    set(&system, dir.path(), "showHidden", true).unwrap();
    set(&system, dir.path(), "fasterMenus", true).unwrap();
    let (reset, failed) = reset_all(&system, dir.path()).unwrap();
    assert_eq!((reset, failed.len()), (2, 0));
    assert!(Record::load(dir.path()).unwrap().changed.is_empty());
    assert!(find_value(&system.values_of(&advanced()), "Hidden").is_none());
}

#[test]
fn every_tweak_switches_on_and_back_to_exactly_what_was_there() {
    let (dir, system) = machine();
    // Every key a tweak touches, with whether it exists and its values.
    let state = |tweak: &Tweak| {
        tweak
            .on
            .iter()
            .chain(tweak.off)
            .map(|change| {
                let key = key_of(change).unwrap();
                (system.has_key(&key), system.values_of(&key))
            })
            .collect::<Vec<_>>()
    };
    let version = RegPath::new(
        Hive::LocalMachine,
        r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
    );
    for tweak in &TWEAKS {
        // Each on the Windows that has it.
        let build = if tweak.windows10 { "19045" } else { "26100" };
        system.put(
            &version,
            vec![RawValue::string("CurrentBuildNumber", build)],
        );
        let shown = |on: bool| {
            list(&system, dir.path())
                .unwrap()
                .into_iter()
                .find(|view| view.id == tweak.id)
                .is_some_and(|view| view.on == on)
        };
        if !shown(false) {
            continue;
        }
        let before = state(tweak);
        set(&system, dir.path(), tweak.id, true).unwrap();
        assert!(shown(true), "{} did not switch on", tweak.id);
        set(&system, dir.path(), tweak.id, false).unwrap();
        assert!(shown(false), "{} did not switch off", tweak.id);
        assert_eq!(state(tweak), before, "{} left something behind", tweak.id);
    }
}

#[test]
fn windows_11_cannot_switch_windows_10_tweaks() {
    let (dir, system) = machine();
    system.put(
        &RegPath::new(
            Hive::LocalMachine,
            r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
        ),
        vec![RawValue::string("CurrentBuildNumber", "26100")],
    );
    let error = set(&system, dir.path(), "classicBattery", true).unwrap_err();
    assert!(error.to_string().contains("Only Windows 10"), "{error}");
    assert!(set(&system, dir.path(), "taskbarLeft", true).is_ok());
}
