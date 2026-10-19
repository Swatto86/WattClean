//! A watch log saved for a spreadsheet, or as plain text.

use std::fmt::Write;

use super::Event;
use crate::export::Table;

/// The log as a list, for saving as CSV or an Excel workbook.
pub fn table(events: &[Event]) -> Table {
    Table {
        headers: [
            "Time (ms since 1970)",
            "Time (UTC)",
            "Operation",
            "Key",
            "Value",
            "Type",
            "Old data",
            "New data",
            "Process",
            "PID",
        ]
        .map(str::to_owned)
        .to_vec(),
        rows: events
            .iter()
            .map(|event| {
                let (process, pid) = actor(event);
                vec![
                    event.time.to_string(),
                    utc(event.time),
                    format!("{:?}", event.change.op),
                    event.change.key.display(),
                    event.change.value.clone().unwrap_or_default(),
                    event.change.kind.clone().unwrap_or_default(),
                    event.change.old.clone().unwrap_or_default(),
                    event.change.new.clone().unwrap_or_default(),
                    process,
                    pid,
                ]
            })
            .collect(),
    }
}

/// The log as comma-separated text, for saving.
pub fn to_csv(events: &[Event]) -> String {
    table(events).csv()
}

/// The log as readable text, one change per line, times in UTC.
pub fn to_text(events: &[Event]) -> String {
    let mut text = String::new();
    for event in events {
        let change = &event.change;
        let (process, pid) = actor(event);
        let _ = write!(
            text,
            "{}  {:?}  {}",
            utc(event.time),
            change.op,
            change.key.display()
        );
        if let Some(value) = &change.value {
            let _ = write!(text, "  [{value}]");
        }
        match (&change.old, &change.new) {
            (Some(old), Some(new)) => {
                let _ = write!(text, "  {old} -> {new}");
            }
            (Some(old), None) => {
                let _ = write!(text, "  was {old}");
            }
            (None, Some(new)) => {
                let _ = write!(text, "  = {new}");
            }
            (None, None) => {}
        }
        if !process.is_empty() || !pid.is_empty() {
            let _ = write!(text, "  by {process} ({pid})");
        }
        text.push_str("\r\n");
    }
    text
}

fn actor(event: &Event) -> (String, String) {
    event.actor.as_ref().map_or_else(
        || (String::new(), String::new()),
        |actor| (actor.process.clone(), actor.pid.to_string()),
    )
}

/// `2026-10-16 13:51:10.664 UTC` for milliseconds since 1970.
fn utc(millis: u64) -> String {
    let seconds = millis / 1000;
    let days = i64::try_from(seconds / 86_400).unwrap_or(0);
    let rest = seconds % 86_400;
    // Days to a civil date (Howard Hinnant's algorithm).
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}.{:03} UTC",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60,
        millis % 1000
    )
}

#[cfg(test)]
#[cfg(windows)]
mod tests {
    use super::*;
    use crate::model::{Hive, RegPath};
    use crate::trace::Actor;
    use crate::watch::{Change, Op};

    fn event(actor: Option<Actor>) -> Event {
        Event {
            seq: 1,
            time: 1_792_156_317_123,
            change: Change {
                op: Op::ValueChanged,
                key: RegPath::new(Hive::CurrentUser, r"Software\Acme"),
                value: Some("Size".into()),
                kind: Some("REG_SZ".into()),
                old: Some("a".into()),
                new: Some("b".into()),
            },
            actor,
        }
    }

    #[test]
    fn times_read_as_utc_dates() {
        assert_eq!(utc(0), "1970-01-01 00:00:00.000 UTC");
        assert_eq!(utc(951_782_400_000), "2000-02-29 00:00:00.000 UTC");
        assert_eq!(utc(1_792_156_317_123), "2026-10-16 13:11:57.123 UTC");
    }

    #[test]
    fn text_lines_read_like_the_page() {
        let named = Actor {
            pid: 7,
            process: "reg.exe".into(),
        };
        assert_eq!(
            to_text(&[event(Some(named)), event(None)]),
            "2026-10-16 13:11:57.123 UTC  ValueChanged  HKCU\\Software\\Acme  [Size]  a -> b  by reg.exe (7)\r\n\
             2026-10-16 13:11:57.123 UTC  ValueChanged  HKCU\\Software\\Acme  [Size]  a -> b\r\n"
        );
    }
}
