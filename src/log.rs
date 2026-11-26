//! The app log, `%LOCALAPPDATA%\shows-remote\shows-remote.log` (`SHOWS_REMOTE_LOG` names
//! another file, as the tests do), which log-ship sends to the pile (at-home-tools
//! `docs/app-logs.md`). Each line is the house's one log line, written by logline
//! (AHT-204): the time with milliseconds and its zone, the pid, `info`, `warn` or `error`,
//! the event as the message (`connection-lost`, ...), then the event's fields and `role=`.
//! logline opens and closes the file for each line and moves it to `shows-remote.log.old`
//! past 1 MiB.
//!
//! Nothing here reaches a terminal or a pipe: stdout and stderr belong to the
//! full-screen UI. A log that can't be opened is reported by the info panel instead.
use std::fmt::Display;
use std::fs::{self, OpenOptions};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use logline::{Level, Logger};

/// Long enough for a failure's whole message.
const MAX_VALUE: usize = 1024;

struct Log {
    logger: Logger,
    role: &'static str,
    failure: Mutex<Option<String>>,
}

static LOG: OnceLock<Log> = OnceLock::new();

/// Starts logging for this process. Until it is called, as in unit tests, events go
/// nowhere. A panic is logged too, because the screen it happens on is gone with it.
pub fn open(role: &'static str) {
    let logger = Logger::open("shows-remote", "SHOWS_REMOTE_LOG");
    let failure = match logger.path() {
        Some(path) => writable(path).err().map(|e| e.to_string()),
        None => Some("this PC has no LOCALAPPDATA or HOME".to_owned()),
    };
    let log = Log {
        logger,
        role,
        failure: Mutex::new(failure),
    };
    if LOG.set(log).is_ok() {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic| {
            error("panic", &[("detail", panic)]);
            previous(panic);
        }));
    }
}

/// Whether the log can be added to, for the info panel; logline itself never fails.
fn writable(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map(drop)
}

/// The info panel's line: where the log is, or why nothing is reaching it.
pub fn status() -> String {
    let Some(log) = LOG.get() else {
        return "logging did not start".into();
    };
    let path = log
        .logger
        .path()
        .map_or("no log file".into(), |path| path.display().to_string());
    match log.failure.lock().unwrap().as_deref() {
        Some(failure) => format!("{path} (not writing: {failure})"),
        None => path,
    }
}

pub fn info(event: &str, fields: &[(&str, &dyn Display)]) {
    write(Level::Info, event, fields);
}

pub fn warn(event: &str, fields: &[(&str, &dyn Display)]) {
    write(Level::Warn, event, fields);
}

pub fn error(event: &str, fields: &[(&str, &dyn Display)]) {
    write(Level::Error, event, fields);
}

fn write(level: Level, event: &str, fields: &[(&str, &dyn Display)]) {
    if let Some(log) = LOG.get() {
        write_to(&log.logger, log.role, level, event, fields);
    }
}

/// Every logged value is outside text at heart (a player's error, a show name), so each
/// goes through logline's `redact`, the estate's one list of credential shapes, and is
/// capped.
fn write_to(
    logger: &Logger,
    role: &str,
    level: Level,
    event: &str,
    fields: &[(&str, &dyn Display)],
) {
    let values: Vec<String> = fields
        .iter()
        .map(|(_, value)| capped(logline::redact(&value.to_string())))
        .collect();
    let mut line: Vec<(&str, &dyn Display)> = fields
        .iter()
        .zip(&values)
        .map(|((name, _), value)| (*name, value as &dyn Display))
        .collect();
    line.push(("role", &role));
    logger.log(level, event, &line);
}

fn capped(text: String) -> String {
    match text.char_indices().nth(MAX_VALUE) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text,
    }
}

/// (year, month, day) for a count of days since 1970-01-01.
pub fn civil_date(days: i64) -> (i64, i64, i64) {
    let days = days + 719468;
    let era = days.div_euclid(146097);
    let day_of_era = days.rem_euclid(146097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let month = (shifted_month + 2) % 12 + 1;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day_of_year - (153 * shifted_month + 2) / 5 + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn logged(level: Level, event: &str, fields: &[(&str, &dyn Display)]) -> Vec<String> {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("shows-remote.log");
        write_to(&Logger::open_file(&path), "client", level, event, fields);
        fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn a_line_is_the_standard_one_with_its_role_and_fields() {
        let lines = logged(
            Level::Warn,
            "connection-lost",
            &[
                ("target", &"upstairspc"),
                ("message", &"closed:\r\nPermission denied\n"),
            ],
        );
        assert_eq!(lines.len(), 1, "{lines:?}");
        let entry = logline::parse(&lines[0]).unwrap();
        assert_eq!(entry.level, Level::Warn);
        assert_eq!(entry.pid, std::process::id());
        assert_eq!(entry.message, "connection-lost");
        assert_eq!(
            entry.fields,
            [
                ("target".into(), "upstairspc".into()),
                ("message".into(), "closed:\r\nPermission denied\n".into()),
                ("role".into(), "client".into()),
            ]
        );
    }

    #[test]
    fn terminal_escapes_and_quotes_cannot_forge_lines_or_fields() {
        let forged = "bad\n2026-11-03T00:00:00.000Z pid=1 error \"forged\" \u{1b}[31m";
        let lines = logged(Level::Info, "reply", &[("error", &forged)]);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(!lines[0].contains('\u{1b}'));
        let entry = logline::parse(&lines[0]).unwrap();
        assert_eq!(entry.fields[0], ("error".into(), forged.into()));
    }

    #[test]
    fn an_empty_value_still_reads_as_a_field() {
        let lines = logged(Level::Info, "e", &[("reason", &"")]);
        assert!(
            lines[0].ends_with(" e reason=\"\" role=client"),
            "{lines:?}"
        );
    }

    #[test]
    fn secrets_never_reach_a_line() {
        for (leak, secret) in [
            ("token=hunter2 next", "hunter2"),
            ("Authorization: Bearer hunter2", "hunter2"),
            ("https://user:hunter2@nas.example/share", "hunter2"),
            ("GET https://blob.example/a.mkv?sv=1&sig=hunter2", "hunter2"),
        ] {
            let lines = logged(Level::Error, "failed", &[("error", &leak)]);
            assert!(!lines[0].contains(secret), "{lines:?}");
        }
    }

    #[test]
    fn what_a_failure_needs_survives_masking() {
        for plain in [
            "Permission denied (publickey,password,keyboard-interactive).",
            r"C:\Program Files\at-home\shows-remote\current\shows-remote.exe",
            "Could not resolve hostname upstairspc: No such host is known.",
            "show_id: 3fa85f64-5717-4562-b3fc-2c963f66afa6",
            "http://127.0.0.1:45831/status",
        ] {
            let lines = logged(Level::Error, "failed", &[("error", &plain)]);
            let entry = logline::parse(&lines[0]).unwrap();
            assert_eq!(entry.fields[0].1, plain);
        }
    }

    #[test]
    fn a_value_is_capped_on_a_character_boundary() {
        let text = capped("é ".repeat(MAX_VALUE));
        assert!(text.ends_with('…'));
        assert_eq!(text.chars().count(), MAX_VALUE + 1);
    }

    #[test]
    fn a_log_it_cant_open_is_said() {
        let folder = tempfile::tempdir().unwrap();
        let blocker = folder.path().join("file");
        fs::write(&blocker, "").unwrap();
        assert!(writable(&blocker.join("shows-remote.log")).is_err());
        assert!(writable(&folder.path().join("logs").join("shows-remote.log")).is_ok());
    }
}
