//! The Task Scheduler task for a job, as the XML `schtasks /Create /XML`
//! reads.

use std::fmt::Write;
use std::path::Path;

use super::{Frequency, Job, parse_time};

const WEEKDAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// Every job's task lives in its own Task Scheduler folder.
pub fn task_name(id: &str) -> String {
    format!(r"\WattClean\Job {id}")
}

/// The task: start `exe --run-job <id>` on the job's schedule, as the
/// signed-in person, with the highest rights they have when `elevated` (so
/// machine-wide entries can be cleaned). A run missed while the PC was off
/// happens at the next chance; one run at a time; at most an hour.
pub fn task_xml(job: &Job, exe: &Path, elevated: bool, start_date: &str) -> String {
    let (hours, minutes) = parse_time(&job.schedule.time).unwrap_or((3, 0));
    let schedule = match job.schedule.frequency {
        Frequency::Day => {
            "<ScheduleByDay><DaysInterval>1</DaysInterval></ScheduleByDay>".to_owned()
        }
        Frequency::Week { weekday } => format!(
            "<ScheduleByWeek><DaysOfWeek><{0} /></DaysOfWeek><WeeksInterval>1</WeeksInterval></ScheduleByWeek>",
            WEEKDAYS[usize::from(weekday.min(6))]
        ),
        Frequency::Month { day } => {
            let mut months = String::new();
            for month in MONTHS {
                let _ = write!(months, "<{month} />");
            }
            format!(
                "<ScheduleByMonth><DaysOfMonth><Day>{day}</Day></DaysOfMonth><Months>{months}</Months></ScheduleByMonth>"
            )
        }
    };
    let run_level = if elevated {
        "HighestAvailable"
    } else {
        "LeastPrivilege"
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>WattClean: {name}</Description>
  </RegistrationInfo>
  <Triggers>
    <CalendarTrigger>
      <StartBoundary>{start_date}T{hours:02}:{minutes:02}:00</StartBoundary>
      <Enabled>true</Enabled>
      {schedule}
    </CalendarTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>{run_level}</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <StartWhenAvailable>true</StartWhenAvailable>
    <ExecutionTimeLimit>PT1H</ExecutionTimeLimit>
    <Enabled>true</Enabled>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{exe}</Command>
      <Arguments>--run-job {id}</Arguments>
    </Exec>
  </Actions>
</Task>
"#,
        name = escape(&job.name),
        exe = escape(&exe.to_string_lossy()),
        id = escape(&job.id),
    )
}

pub(crate) fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
