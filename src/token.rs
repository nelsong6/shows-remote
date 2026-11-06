//! The sign-in a player on another PC asks for: a five-minute token OpenBao signs for this
//! PC, saved by `romaine-auth home-token` and kept here until it's nearly spent. The TPM
//! key stays with romaine-auth; this app forbids unsafe code and never touches it.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use shows_control::TokenSource;

use crate::log;

/// A request and the player's check both have to land inside the token's life.
const RENEW_BEFORE_SECONDS: u64 = 30;
/// Sign-in takes seconds, so an open stream replaces the token before a request has to
/// wait for it. Two of the player's 30-second heartbeats land inside this margin.
const RENEW_AHEAD_SECONDS: u64 = 90;

type Held = Mutex<Option<(String, u64)>>;

static HELD: Held = Mutex::new(None);
static RENEWING: AtomicBool = AtomicBool::new(false);

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn parsed(saved: &str) -> Result<(String, u64), String> {
    let value: Value =
        serde_json::from_str(saved).map_err(|e| format!("romaine-auth saved no token: {e}"))?;
    match (value["token"].as_str(), value["expires_at"].as_u64()) {
        (Some(token), Some(expires)) if !token.is_empty() => Ok((token.to_owned(), expires)),
        _ => Err("romaine-auth saved no token".into()),
    }
}

fn out_file() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let name = format!(
        "shows-remote-token-{}-{}.json",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    std::env::temp_dir().join(name)
}

fn requested() -> Result<(String, u64), String> {
    let file = out_file();
    let run = Command::new("romaine-auth")
        .args(["home-token", "--role", "shows", "--out"])
        .arg(&file)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("could not run romaine-auth: {e}"))?;
    let saved = std::fs::read_to_string(&file);
    let _ = std::fs::remove_file(&file);
    if !run.status.success() {
        let reason = String::from_utf8_lossy(&run.stderr).trim().to_owned();
        return Err(format!("could not sign in to OpenBao: {reason}"));
    }
    parsed(&saved.map_err(|e| format!("romaine-auth saved no token: {e}"))?)
}

fn current(held: &Held) -> Result<String, String> {
    let mut held = held.lock().unwrap();
    if let Some((token, expires)) = held.as_ref() {
        if now() + RENEW_BEFORE_SECONDS < *expires {
            return Ok(token.clone());
        }
    }
    let (token, expires) = requested().inspect_err(|error| {
        log::warn("sign-in-failed", &[("error", error)]);
    })?;
    log::info("signed-in", &[("expires", &expires)]);
    *held = Some((token.clone(), expires));
    Ok(token)
}

/// Requests keep using the held token while its replacement is fetched. A failure leaves
/// it in place for the next heartbeat to retry, and `current` reports it if it runs out.
fn renewed_ahead(
    held: &'static Held,
    renewing: &'static AtomicBool,
    sign_in: impl FnOnce() -> Result<(String, u64), String> + Send + 'static,
) -> Option<JoinHandle<()>> {
    let (_, expires) = held.lock().unwrap().clone()?;
    if now() + RENEW_AHEAD_SECONDS < expires || renewing.swap(true, Ordering::AcqRel) {
        return None;
    }
    Some(thread::spawn(move || {
        match sign_in() {
            Ok((token, expires)) => {
                log::info("signed-in", &[("expires", &expires)]);
                *held.lock().unwrap() = Some((token, expires));
            }
            Err(error) => log::warn("sign-in-failed", &[("error", &error)]),
        }
        renewing.store(false, Ordering::Release);
    }))
}

/// Called on each status frame, so an open remote always has a token ready.
pub fn renew_ahead() {
    renewed_ahead(&HELD, &RENEWING, requested);
}

/// One token shared by every connection in this process.
pub fn source() -> TokenSource {
    Arc::new(|| current(&HELD))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_token_is_read_and_a_bad_one_is_refused() {
        assert_eq!(
            parsed(r#"{"token":"signed","expires_at":99}"#).unwrap(),
            ("signed".to_owned(), 99)
        );
        for bad in ["", "{}", r#"{"token":"","expires_at":99}"#, r#"{"token":"signed"}"#] {
            assert!(parsed(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_token_with_time_left_is_kept_without_asking_again() {
        let held = Mutex::new(Some(("kept".to_owned(), now() + 300)));
        assert_eq!(current(&held).unwrap(), "kept");
    }

    fn held_for(seconds: u64) -> (&'static Held, &'static AtomicBool) {
        (
            Box::leak(Box::new(Mutex::new(Some(("old".to_owned(), now() + seconds))))),
            Box::leak(Box::new(AtomicBool::new(false))),
        )
    }

    #[test]
    fn requests_keep_the_old_token_until_its_replacement_arrives() {
        let (held, renewing) = held_for(60);
        let (release, gate) = std::sync::mpsc::channel::<()>();
        let expires = now() + 300;
        let renewal = renewed_ahead(held, renewing, move || {
            gate.recv().unwrap();
            Ok(("new".to_owned(), expires))
        })
        .unwrap();
        assert_eq!(current(held).unwrap(), "old");
        assert!(renewed_ahead(held, renewing, || unreachable!()).is_none());
        release.send(()).unwrap();
        renewal.join().unwrap();
        assert_eq!(current(held).unwrap(), "new");
    }

    #[test]
    fn a_failed_renewal_keeps_the_token_and_the_next_heartbeat_retries() {
        let (held, renewing) = held_for(60);
        renewed_ahead(held, renewing, || Err("OpenBao is down".into()))
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(current(held).unwrap(), "old");
        renewed_ahead(held, renewing, || Ok(("new".to_owned(), now() + 300)))
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(current(held).unwrap(), "new");
    }

    #[test]
    fn nothing_renews_with_time_in_hand_or_with_no_token() {
        let (held, renewing) = held_for(300);
        assert!(renewed_ahead(held, renewing, || unreachable!()).is_none());
        let empty: &'static Held = Box::leak(Box::new(Mutex::new(None)));
        assert!(renewed_ahead(empty, renewing, || unreachable!()).is_none());
    }
}
