//! The app log: one plain line per event, in a file log-ship sends to the pile
//! (at-home-tools `docs/app-logs.md`). Each line starts with UTC time, so it reads
//! beside the player's `shows.log`.
//! the file, so a line is one append with the file closed after it.
//!
//! Nothing here reaches a terminal or a pipe: stdout and stderr belong to the
//! full-screen UI. A write that fails is
//! reported by the info panel instead.
use std::fmt::Display;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// Matches log-ship's 4 MiB first-sight backfill, so a PC's whole file reaches the pile.
const ROLL_AT: u64 = 4 * 1024 * 1024;
/// Long enough for a failure's whole message.
const MAX_VALUE: usize = 1024;
const REMOVED: &str = "[removed]";
const SECRET_NAMES: [&str; 13] = [
    "password",
    "passwd",
    "passphrase",
    "pwd",
    "secret",
    "token",
    "apikey",
    "api_key",
    "api-key",
    "privatekey",
    "private_key",
    "credential",
    "authorization",
];

struct Log {
    path: Option<PathBuf>,
    role: &'static str,
    failure: Mutex<Option<String>>,
}

static LOG: OnceLock<Log> = OnceLock::new();

fn location() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("SHOWS_REMOTE_LOG") {
        return Some(path.into());
    }
    if let Some(root) = std::env::var_os("LOCALAPPDATA") {
        return Some(
            PathBuf::from(root)
                .join("shows-remote")
                .join("shows-remote.log"),
        );
    }
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".local/state/shows-remote/shows-remote.log"))
}

/// Starts logging for this process. Until it is called, as in unit tests, events go
/// nowhere. A panic is logged too, because the screen it happens on is gone with it.
pub fn open(role: &'static str) {
    let path = location();
    if let Some(parent) = path.as_deref().and_then(Path::parent) {
        let _ = fs::create_dir_all(parent);
    }
    let failure = path
        .is_none()
        .then(|| "this PC has no LOCALAPPDATA or HOME".to_owned());
    let log = Log {
        path,
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

/// The info panel's line: where the log is, or why nothing is reaching it.
pub fn status() -> String {
    let Some(log) = LOG.get() else {
        return "logging did not start".into();
    };
    let path = log
        .path
        .as_deref()
        .map_or("no log file".into(), |path| path.display().to_string());
    match log.failure.lock().unwrap().as_deref() {
        Some(failure) => format!("{path} (not writing: {failure})"),
        None => path,
    }
}

pub fn info(event: &str, fields: &[(&str, &dyn Display)]) {
    write("INFO", event, fields);
}

pub fn warn(event: &str, fields: &[(&str, &dyn Display)]) {
    write("WARN", event, fields);
}

pub fn error(event: &str, fields: &[(&str, &dyn Display)]) {
    write("ERROR", event, fields);
}

fn write(level: &str, event: &str, fields: &[(&str, &dyn Display)]) {
    let Some(log) = LOG.get() else { return };
    let Some(path) = log.path.as_deref() else {
        return;
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as i64);
    let text = line(now, level, log.role, std::process::id(), event, fields);
    *log.failure.lock().unwrap() = append(path, ROLL_AT, &text).err().map(|e| e.to_string());
}

/// A roll renames rather than trims: log-ship then reads the short new file from its
/// top, which holds only new lines, and the listed name never matches the old file.
fn append(path: &Path, limit: u64, line: &str) -> io::Result<()> {
    if fs::metadata(path).is_ok_and(|file| file.len() >= limit) {
        let mut old = path.as_os_str().to_owned();
        old.push(".old");
        // Another process may be mid-write or have rolled first; the next line retries.
        let _ = fs::rename(path, old);
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(line.as_bytes())
}

fn line(
    millis: i64,
    level: &str,
    role: &str,
    pid: u32,
    event: &str,
    fields: &[(&str, &dyn Display)],
) -> String {
    let mut text = format!("{} {level:<5} {role} pid={pid} {event}", rfc3339(millis));
    for (name, value) in fields {
        let value = scrub(&value.to_string());
        if value.is_empty() || value.contains(' ') {
            text.push_str(&format!(" {name}=\"{value}\""));
        } else {
            text.push_str(&format!(" {name}={value}"));
        }
    }
    text.push('\n');
    text
}

fn rfc3339(millis: i64) -> String {
    let (year, month, day) = civil_date(millis.div_euclid(86_400_000));
    let of_day = millis.rem_euclid(86_400_000);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        of_day / 3_600_000,
        of_day / 60_000 % 60,
        of_day / 1000 % 60,
        of_day % 1000
    )
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

fn secret_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name == "sig" || SECRET_NAMES.iter().any(|secret| name.contains(secret))
}

/// A run this long with a digit in it is a key, token or digest, never a word or path.
fn opaque(word: &str) -> bool {
    word.split(|c: char| !(c.is_ascii_alphanumeric() || "+/_-".contains(c)))
        .any(|run| run.len() >= 40 && run.contains(|c: char| c.is_ascii_digit()))
}

fn without_url_secrets(word: &str) -> Option<String> {
    let (scheme, rest) = word.split_once("://")?;
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let host = authority.rsplit('@').next().unwrap_or(authority);
    Some(format!("{scheme}://{host}{path}"))
}

/// Every logged value is outside text at heart (a player's error, a show
/// name), so each becomes one line with anything shaped like a secret taken out.
fn scrub(text: &str) -> String {
    let (text, key_block) = match text.find("-----BEGIN") {
        Some(at) => (&text[..at], true),
        None => (text, false),
    };
    let mut words = Vec::new();
    let mut secret_next = false;
    for word in text
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|word| !word.is_empty())
    {
        let scheme = word.eq_ignore_ascii_case("bearer") || word.eq_ignore_ascii_case("basic");
        if secret_next {
            secret_next = scheme;
            words.push(REMOVED.to_owned());
        } else if let Some(url) = without_url_secrets(word) {
            words.push(url);
        } else if let Some((name, value)) = word
            .split_once(['=', ':'])
            .filter(|(name, _)| secret_name(name))
        {
            secret_next = value.is_empty();
            words.push(format!("{name}={REMOVED}"));
        } else if scheme {
            secret_next = true;
            words.push(word.to_owned());
        } else if opaque(word) {
            words.push(REMOVED.to_owned());
        } else {
            words.push(word.to_owned());
        }
    }
    if key_block {
        words.push(REMOVED.to_owned());
    }
    let joined = words.join(" ").replace('"', "'");
    match joined.char_indices().nth(MAX_VALUE) {
        Some((at, _)) => format!("{}…", &joined[..at]),
        None => joined,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOMENT: i64 = 1_793_661_240_123;

    #[test]
    fn a_line_starts_with_a_zoned_time_and_stays_one_line() {
        let text = line(
            MOMENT,
            "WARN",
            "client",
            7,
            "connection-lost",
            &[
                ("target", &"upstairspc"),
                ("message", &"closed:\r\nPermission denied\n"),
            ],
        );
        assert_eq!(
            text,
            "2026-11-02T23:14:00.123Z WARN  client pid=7 connection-lost target=upstairspc message=\"closed: Permission denied\"\n"
        );
    }

    #[test]
    fn terminal_escapes_and_quotes_cannot_forge_lines_or_fields() {
        let text = line(
            MOMENT,
            "INFO",
            "agent",
            7,
            "reply",
            &[(
                "error",
                &"bad\n2026-11-03T00:00:00Z ERROR \"forged\" \u{1b}[31m",
            )],
        );
        assert_eq!(text.lines().count(), 1);
        assert!(!text.contains('\u{1b}'));
        assert!(text.contains("'forged'"));
    }

    #[test]
    fn an_empty_value_still_reads_as_a_field() {
        let text = line(MOMENT, "INFO", "agent", 7, "e", &[("reason", &"")]);
        assert!(text.ends_with("e reason=\"\"\n"));
    }

    #[test]
    fn key_material_and_secrets_never_reach_a_line() {
        for (leak, secret) in [
            (
                "ssh: -----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAA\n-----END",
                "b3BlbnNz",
            ),
            (
                "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJxYzAbCdEfGhIjKlMnOpQrStUvWxYz0123456789 me",
                "AAAAC3Nza",
            ),
            ("token=hunter2 next", "hunter2"),
            ("Password: hunter2", "hunter2"),
            ("Authorization: Bearer hunter2", "hunter2"),
            ("Authorization:", "Authorization:"),
            ("api_key:hunter2", "hunter2"),
            ("https://user:hunter2@nas.example/share", "hunter2"),
            ("GET https://blob.example/a.mkv?sv=1&sig=hunter2", "hunter2"),
            ("sig=hunter2", "hunter2"),
        ] {
            let text = scrub(leak);
            assert!(!text.contains(secret), "{text}");
        }
    }

    #[test]
    fn what_a_failure_needs_survives_scrubbing() {
        for plain in [
            "Permission denied (publickey,password,keyboard-interactive).",
            r"C:\Users\Nelson\AppData\Local\shows-remote\channels\stable\shows-remote.exe",
            "Could not resolve hostname upstairspc: No such host is known.",
            "show_id: 3fa85f64-5717-4562-b3fc-2c963f66afa6",
            "Host key verification failed.",
            "pos=44 show=boys episode=e16da8d4-4f87-45f2-8cdc-be33a3bb13fa alerts=",
            "http://127.0.0.1:45831/status",
        ] {
            assert_eq!(scrub(plain), plain);
        }
    }

    #[test]
    fn a_value_is_capped_on_a_character_boundary() {
        let text = scrub(&"é ".repeat(MAX_VALUE));
        assert!(text.ends_with('…'));
        assert_eq!(text.chars().count(), MAX_VALUE + 1);
    }

    #[test]
    fn a_full_file_rolls_once_and_keeps_every_line() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("shows-remote.log");
        append(&path, 10, "first line\n").unwrap();
        append(&path, 10, "second\n").unwrap();
        append(&path, 10, "third\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "second\nthird\n");
        assert_eq!(
            fs::read_to_string(folder.path().join("shows-remote.log.old")).unwrap(),
            "first line\n"
        );
    }

    #[test]
    fn writers_sharing_the_file_never_split_a_line() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("shows-remote.log");
        std::thread::scope(|scope| {
            for writer in 0..4 {
                let path = &path;
                scope.spawn(move || {
                    for _ in 0..50 {
                        let text = format!("{}\n", writer.to_string().repeat(200));
                        append(path, ROLL_AT, &text).unwrap();
                    }
                });
            }
        });
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 200);
        assert!(text.lines().all(|line| {
            line.len() == 200 && line.bytes().all(|byte| byte == line.as_bytes()[0])
        }));
    }
}
