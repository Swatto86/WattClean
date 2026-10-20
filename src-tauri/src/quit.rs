//! Quitting while a task runs. Closing the window (when it is not kept
//! running), the menu-bar icon's Quit and macOS's Cmd+Q stop a running disk
//! job, wait for a removal on a worker thread, then exit; AppKit's own quit
//! (the Dock's menu, logging out) waits in the Exit event on the main
//! thread instead, for a removal at most `QUIT_LIMIT`. Meanwhile the window
//! is hidden and no new disk job or removal starts.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use crate::disk;
#[cfg(windows)]
use crate::registry;

/// A registry clean, or an uninstall's removal or restore, is under way:
/// cut short, its backup would hold entries that were never removed.
#[cfg(windows)]
pub fn removing(app: &AppHandle) -> bool {
    app.try_state::<Arc<registry::state::AppState>>()
        .is_some_and(|state| state.is_cleaning())
        || app
            .try_state::<Arc<crate::uninstall::state::AppState>>()
            .is_some_and(|state| state.is_removing())
}

/// A package manager is removing a program: cut short, it could leave the
/// package database half-changed.
#[cfg(not(windows))]
pub fn removing(app: &AppHandle) -> bool {
    app.try_state::<Arc<crate::packages::AppState>>()
        .is_some_and(|state| state.is_removing())
}

/// How long a quit waits for a stopped disk job to wind down, and the
/// longest macOS's own Quit waits for a removal (`wait_until_idle`).
pub const QUIT_LIMIT: Duration = Duration::from_secs(60);

/// Quits once the app is idle, at once when it already is. Otherwise the
/// window goes at once (so closing never seems to do nothing), and a worker
/// thread waits (`wait_until_idle`), for a removal as long as it takes,
/// then exits. The menu-bar icon can show the window again meanwhile, but
/// no new disk job or removal starts.
pub fn quit_when_idle(app: &AppHandle) {
    stop_new_work(app);
    if !disk_busy(app) && !removing(app) {
        app.exit(0);
        return;
    }
    crate::tray::hide_main(app);
    let app = app.clone();
    std::thread::spawn(move || {
        wait_until_idle(&app, None);
        app.exit(0);
    });
}

/// Stops the running disk job (a wipe gives its space back when stopped)
/// and waits for it, at most `QUIT_LIMIT`, and for any registry clean or
/// uninstall removal to finish: at most `removal_limit`, or as long as it
/// takes. No new disk job or removal starts meanwhile, and the window is
/// hidden. macOS's own Quit (the Dock's menu, logging out) ends the app
/// from the main thread without asking, so the Exit event waits here too,
/// with a limit since the app cannot draw meanwhile, unless a quit already
/// waited.
pub fn wait_until_idle(app: &AppHandle, removal_limit: Option<Duration>) {
    static WAITED: AtomicBool = AtomicBool::new(false);
    if WAITED.load(Ordering::SeqCst) {
        return;
    }
    stop_new_work(app);
    if disk_busy(app) || removing(app) {
        crate::tray::hide_main(app);
    }
    if disk_busy(app)
        && let Some(disk) = app.try_state::<Arc<disk::state::AppState>>()
    {
        disk.cancel();
    }
    let now = Instant::now();
    wait_while(
        || disk_busy(app),
        || removing(app),
        now + QUIT_LIMIT,
        removal_limit.map(|limit| now + limit),
    );
    WAITED.store(true, Ordering::SeqCst);
}

/// Waits while the disk job runs, until `disk_until`, and while a removal
/// runs, until `removal_until` (as long as it takes when `None`).
fn wait_while(
    disk_busy: impl Fn() -> bool,
    removing: impl Fn() -> bool,
    disk_until: Instant,
    removal_until: Option<Instant>,
) {
    loop {
        let now = Instant::now();
        let disk = now < disk_until && disk_busy();
        let removal = removal_until.is_none_or(|until| now < until) && removing();
        if !disk && !removal {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// From now on no disk job or removal starts: the app is quitting, and one
/// started now could be cut off when it exits.
fn stop_new_work(app: &AppHandle) {
    if let Some(disk) = app.try_state::<Arc<disk::state::AppState>>() {
        disk.stop_new_jobs();
    }
    #[cfg(not(windows))]
    if let Some(packages) = app.try_state::<Arc<crate::packages::AppState>>() {
        packages.stop_new_removals();
    }
}

fn disk_busy(app: &AppHandle) -> bool {
    app.try_state::<Arc<disk::state::AppState>>()
        .is_some_and(|disk| disk.is_busy())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs `wait_while` on a worker; whether it returned within `limit`.
    fn returns_within(
        limit: Duration,
        disk_busy: impl Fn() -> bool + Send + 'static,
        removing: impl Fn() -> bool + Send + 'static,
        disk_for: Duration,
        removal_for: Option<Duration>,
    ) -> bool {
        let (done, finished) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let now = Instant::now();
            wait_while(
                disk_busy,
                removing,
                now + disk_for,
                removal_for.map(|d| now + d),
            );
            let _ = done.send(());
        });
        finished.recv_timeout(limit).is_ok()
    }

    #[test]
    fn a_removal_that_never_ends_holds_the_exit_only_until_its_deadline() {
        assert!(returns_within(
            Duration::from_secs(3),
            || false,
            || true,
            Duration::from_secs(60),
            Some(Duration::from_millis(300)),
        ));
    }

    #[test]
    fn a_stopped_disk_job_is_waited_for_at_most_until_its_deadline() {
        assert!(returns_within(
            Duration::from_secs(3),
            || true,
            || false,
            Duration::from_millis(300),
            None,
        ));
    }

    #[test]
    fn a_removal_without_a_deadline_is_waited_for_until_it_ends() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let ends_on_fifth_look = move || seen.fetch_add(1, Ordering::SeqCst) < 4;
        assert!(returns_within(
            Duration::from_secs(5),
            || false,
            ends_on_fifth_look,
            Duration::ZERO,
            None,
        ));
        assert!(
            calls.load(Ordering::SeqCst) >= 5,
            "it stopped waiting early"
        );
    }
}
