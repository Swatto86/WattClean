//! Starting with Windows, in the notification area. Switched on as
//! administrator, the start is a Task Scheduler sign-in task with the
//! highest rights, so the app starts elevated without a prompt and the
//! Registry Watcher can name the program behind each change; switched on
//! without them, it is the HKCU `Run` value, which starts it with ordinary
//! rights.

use std::path::Path;

use serde::Serialize;

use crate::jobs::escape;
use crate::model::{Hive, RawValue, RegPath, find_value};
use crate::system::System;
use crate::{Error, Result};

pub const RUN_VALUE: &str = "WattClean";
/// In the schedules' folder, which the uninstaller empties.
pub const TASK: &str = r"\WattClean\Start at sign-in";
/// What a sign-in start passes, so the app starts in the tray.
pub const BACKGROUND_FLAG: &str = "--background";

pub fn run_key() -> RegPath {
    RegPath::new(
        Hive::CurrentUser,
        r"Software\Microsoft\Windows\CurrentVersion\Run",
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Autostart {
    /// Windows starts the app at sign-in.
    pub on: bool,
    /// ...with administrator rights (the sign-in task).
    pub elevated: bool,
}

pub fn state(system: &dyn System) -> Autostart {
    let elevated = system.task_exists(TASK).unwrap_or(false);
    let run = find_value(&system.values(&run_key()).unwrap_or_default(), RUN_VALUE).is_some();
    Autostart {
        on: elevated || run,
        elevated,
    }
}

/// Start `exe` at sign-in: elevated through the task when this copy runs
/// as administrator, with ordinary rights through the `Run` value otherwise.
pub fn enable(system: &dyn System, exe: &Path) -> Result<Autostart> {
    if system.is_elevated() {
        let user = account(system)?;
        system
            .schedule_task(TASK, &task_xml(exe, &user))
            .map_err(refused)?;
        remove_run_value(system)?;
    } else if !state(system).elevated {
        let command = format!("\"{}\" {BACKGROUND_FLAG}", exe.display());
        system
            .write_value(&run_key(), &RawValue::string(RUN_VALUE, &command))
            .map_err(refused)?;
    }
    Ok(state(system))
}

/// Stop starting at sign-in. The task can be removed only with the rights
/// that made it.
pub fn disable(system: &dyn System) -> Result<Autostart> {
    if state(system).elevated
        && let Err(error) = system.delete_task(TASK)
    {
        return Err(if system.is_elevated() {
            refused(error)
        } else {
            Error::Invalid(
                "It was switched on as administrator: restart as administrator to switch it off."
                    .into(),
            )
        });
    }
    remove_run_value(system)?;
    Ok(state(system))
}

fn remove_run_value(system: &dyn System) -> Result<()> {
    let values = system.values(&run_key()).unwrap_or_default();
    if find_value(&values, RUN_VALUE).is_some() {
        system
            .delete_value(&run_key(), RUN_VALUE)
            .map_err(refused)?;
    }
    Ok(())
}

fn refused(error: std::io::Error) -> Error {
    Error::Invalid(format!("Windows refused the change: {error}"))
}

/// The signed-in account, as `DOMAIN\name`: the task starts at its sign-in
/// only, not anyone else's.
fn account(system: &dyn System) -> Result<String> {
    match (system.env_var("USERDOMAIN"), system.env_var("USERNAME")) {
        (Some(domain), Some(name)) if !domain.is_empty() && !name.is_empty() => {
            Ok(format!("{domain}\\{name}"))
        }
        _ => Err(Error::Invalid(
            "Windows did not say which account is signed in.".into(),
        )),
    }
}

/// The sign-in task: `exe --background` when `user` signs in, with the
/// highest rights they have, for as long as the session lasts.
pub fn task_xml(exe: &Path, user: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>WattClean: start in the notification area, with administrator rights so the Registry Watcher can name programs</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
    <Enabled>true</Enabled>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{exe}</Command>
      <Arguments>{BACKGROUND_FLAG}</Arguments>
    </Exec>
  </Actions>
</Task>
"#,
        user = escape(user),
        exe = escape(&exe.to_string_lossy()),
    )
}

#[cfg(test)]
#[cfg(windows)]
mod tests {
    use super::*;
    use crate::fake::FakeSystem;

    fn machine() -> (tempfile::TempDir, FakeSystem) {
        let dir = tempfile::tempdir().unwrap();
        let system = FakeSystem::empty(dir.path());
        system.set_env("USERDOMAIN", "SWATPC");
        system.set_env("USERNAME", "Swatto");
        (dir, system)
    }

    const EXE: &str = r"C:\Program Files\WattClean\wattclean.exe";

    #[test]
    fn as_administrator_it_starts_elevated_through_a_sign_in_task() {
        let (_dir, system) = machine();
        // Switched on earlier without administrator rights.
        system.set_elevated(false);
        let first = enable(&system, Path::new(EXE)).unwrap();
        assert_eq!(
            first,
            Autostart {
                on: true,
                elevated: false
            }
        );
        system.set_elevated(true);
        let now = enable(&system, Path::new(EXE)).unwrap();
        assert_eq!(
            now,
            Autostart {
                on: true,
                elevated: true
            }
        );
        let xml = system.tasks().remove(TASK).unwrap();
        for wanted in [
            "<LogonTrigger>",
            r"<UserId>SWATPC\Swatto</UserId>",
            "<RunLevel>HighestAvailable</RunLevel>",
            "<Command>C:\\Program Files\\WattClean\\wattclean.exe</Command>",
            "<Arguments>--background</Arguments>",
            "<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>",
        ] {
            assert!(xml.contains(wanted), "{wanted} missing from {xml}");
        }
        assert!(
            find_value(&system.values_of(&run_key()), RUN_VALUE).is_none(),
            "the ordinary start is replaced, not doubled"
        );
    }

    #[test]
    fn switching_off_removes_both_kinds_but_the_task_needs_its_rights() {
        let (_dir, system) = machine();
        enable(&system, Path::new(EXE)).unwrap();
        system.set_elevated(false);
        let error = disable(&system).unwrap_err().to_string();
        assert!(error.contains("restart as administrator"), "{error}");
        system.set_elevated(true);
        assert_eq!(
            disable(&system).unwrap(),
            Autostart {
                on: false,
                elevated: false
            }
        );
        system.set_elevated(false);
        enable(&system, Path::new(EXE)).unwrap();
        assert!(!disable(&system).unwrap().on);
    }

    #[test]
    fn the_task_names_its_account_and_escapes_the_path() {
        let xml = task_xml(Path::new(r"C:\A&B\app.exe"), r"PC\Ann");
        assert!(xml.contains(r"<Command>C:\A&amp;B\app.exe</Command>"));
    }
}
