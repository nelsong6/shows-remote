//! The sign-in a player on another PC asks for: a five-minute token OpenBao signs for this
//! PC, saved by `romaine-auth home-token` and kept here until it's nearly spent. The TPM
//! key stays with romaine-auth; this app forbids unsafe code and never touches it.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use shows_control::TokenSource;

use crate::log;

/// A request and the player's check both have to land inside the token's life.
const RENEW_BEFORE_SECONDS: u64 = 30;

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
    std::env::temp_dir().join(format!("shows-remote-token-{}.json", std::process::id()))
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

fn current(held: &Mutex<Option<(String, u64)>>) -> Result<String, String> {
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

/// One token shared by every connection in this process.
pub fn source() -> TokenSource {
    static HELD: Mutex<Option<(String, u64)>> = Mutex::new(None);
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
}
