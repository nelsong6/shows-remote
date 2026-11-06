use std::collections::HashMap;
use std::fmt::Display;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use shows_control::{Client, Command, Frame};

use crate::{log, token};

pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(12);

pub fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.as_bytes()[0].is_ascii_alphanumeric()
        && host
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'.')
}

/// The playback fields worth a line when they change. The clock is left out: it moves
/// every status, and these lines are what show whether a command did anything.
fn playback_state(status: &Value) -> String {
    let text = |value: &Value| value.as_str().unwrap_or("-").to_owned();
    let pos = status["round_pos"].as_u64().unwrap_or(0) as usize;
    let entry = &status["round"][pos];
    let alerts = status["alerts"].as_array().map_or(String::new(), |alerts| {
        alerts
            .iter()
            .filter_map(|alert| alert["message"].as_str())
            .collect::<Vec<_>>()
            .join("; ")
    });
    format!(
        "phase={} paused={} pos={pos} show={} episode={} fullscreen={} alerts={alerts}",
        text(&status["phase"]),
        status["playback"]["paused"],
        text(&entry["show_name"]),
        text(&entry["episode_id"]),
        status["window_fullscreen"],
    )
}

/// Never the reply's body: a status holds the whole round, a preview a video frame.
fn reply_summary(value: &Value) -> String {
    if value.get("phase").is_some() {
        playback_state(value)
    } else if let Some(message) = value["message"].as_str() {
        message.to_owned()
    } else if let Some(entries) = value.as_array() {
        format!("{} entries", entries.len())
    } else {
        "accepted".to_owned()
    }
}

/// Logs what came back for each request. The video view reads up to four frames a
/// second, so a preview is logged only when its outcome changes.
pub struct Replies {
    target: String,
    preview: Option<Result<(), String>>,
}

impl Replies {
    pub fn new(target: &str) -> Self {
        Self {
            target: target.to_owned(),
            preview: None,
        }
    }

    fn preview_changed(&mut self, result: &Result<Value, String>) -> bool {
        let outcome = result.as_ref().map(|_| ()).map_err(Clone::clone);
        let changed = self.preview.as_ref() != Some(&outcome);
        self.preview = Some(outcome);
        changed
    }

    pub fn logged(
        &mut self,
        id: u64,
        command: &Command,
        began: Instant,
        result: &Result<Value, String>,
    ) {
        if *command == (Command::Preview {}) && !self.preview_changed(result) {
            return;
        }
        let label = format!("{command:?}");
        let millis = began.elapsed().as_millis();
        let line = |event, name, value: &dyn Display| {
            let fields: [(&str, &dyn Display); 5] = [
                ("target", &self.target),
                ("id", &id),
                ("command", &label),
                ("ms", &millis),
                (name, value),
            ];
            match event {
                "reply" => log::info(event, &fields),
                _ => log::warn(event, &fields),
            }
        };
        match result {
            Ok(value) => line("reply", "result", &reply_summary(value)),
            Err(error) => line("rejected", "error", error),
        }
    }
}

type Waiting = Arc<Mutex<HashMap<u64, (Command, Instant)>>>;

/// One connection to a player: a worker runs commands in order, and a second thread
/// follows the player's status stream once something asks to watch.
pub struct Session {
    requests: Sender<(u64, Command)>,
    pub events: Receiver<Frame>,
    next_id: u64,
    target: String,
    waiting: Waiting,
    closing: Arc<AtomicBool>,
}

fn opened(host: &str, local: bool) -> Result<(Client, String), String> {
    if local {
        let address = shows_control::control_addr()?;
        return Ok((Client::connect()?, address));
    }
    let address = format!("{host}:{}", shows_control::remote_port()?);
    Ok((Client::connect_remote(&address, token::source())?, address))
}

/// Follows the status stream until it ends or nobody is listening.
fn watch(
    client: Client,
    target: String,
    local: bool,
    events: SyncSender<Frame>,
    closing: Arc<AtomicBool>,
) {
    let mut state = String::new();
    let forwarded = events.clone();
    let ended = client.watch(|frame| {
        if !local {
            token::renew_ahead();
        }
        if let Frame::Status { status } = &frame {
            let now = playback_state(status);
            if now != state {
                log::info("status", &[("target", &target), ("state", &now)]);
                state = now;
            }
        }
        forwarded.send(frame).map_err(|e| e.to_string())
    });
    if let Err(message) = ended {
        if !closing.load(Ordering::Relaxed) {
            log::warn("connection-lost", &[("target", &target), ("message", &message)]);
            let _ = events.send(Frame::Disconnected { message });
        }
    }
}

impl Session {
    pub fn connect(host: &str, local: bool) -> Result<Self, String> {
        if !valid_host(host) {
            log::warn("connect-refused", &[("target", &host)]);
            return Err("target must be a computer hostname".into());
        }
        let (client, address) = opened(host, local).inspect_err(|error| {
            log::warn("connect-failed", &[("target", &host), ("error", error)]);
        })?;
        log::info(
            "connect",
            &[
                ("target", &host),
                ("via", &if local { "local" } else { "network" }),
                ("address", &address),
            ],
        );
        let (requests, queue) = mpsc::channel::<(u64, Command)>();
        let (tx, events) = mpsc::sync_channel(32);
        let waiting: Waiting = Arc::default();
        let closing = Arc::new(AtomicBool::new(false));
        let target = host.to_owned();
        let (answered, closed) = (waiting.clone(), closing.clone());
        std::thread::spawn(move || {
            let mut replies = Replies::new(&target);
            let mut watching = false;
            while let Ok((id, command)) = queue.recv() {
                let result = if command == (Command::Watch {}) {
                    if watching {
                        Err("already watching".to_string())
                    } else {
                        watching = true;
                        let (client, target, events, closing) =
                            (client.clone(), target.clone(), tx.clone(), closed.clone());
                        std::thread::spawn(move || watch(client, target, local, events, closing));
                        Ok(json!({"message": "subscribed to the target's live status"}))
                    }
                } else {
                    client.execute(&command)
                };
                match answered.lock().unwrap().remove(&id) {
                    Some((command, began)) => replies.logged(id, &command, began, &result),
                    None => log::warn("unexpected-reply", &[("target", &target), ("id", &id)]),
                }
                if tx.send(Frame::Reply { id, result }).is_err() {
                    return;
                }
            }
        });
        Ok(Self {
            requests,
            events,
            next_id: 0,
            target: host.to_owned(),
            waiting,
            closing,
        })
    }

    pub fn send(&mut self, command: Command) -> Result<u64, String> {
        command.validate()?;
        self.next_id += 1;
        let id = self.next_id;
        let label = format!("{command:?}");
        if command != (Command::Preview {}) {
            log::info(
                "send",
                &[("target", &self.target), ("id", &id), ("command", &label)],
            );
        }
        self.waiting
            .lock()
            .unwrap()
            .insert(id, (command.clone(), Instant::now()));
        self.requests.send((id, command)).map_err(|_| {
            self.waiting.lock().unwrap().remove(&id);
            let error = "the connection to the player has closed".to_string();
            log::warn(
                "send-failed",
                &[
                    ("target", &self.target),
                    ("id", &id),
                    ("command", &label),
                    ("error", &error),
                ],
            );
            error
        })?;
        Ok(id)
    }

    pub fn call(&mut self, command: Command) -> Result<Value, String> {
        let id = self.send(command)?;
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        loop {
            let event = self.events.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|_| {
                    log::warn("timeout", &[("target", &self.target), ("id", &id)]);
                    "Remote response timed out. The outcome may be unknown; check status before retrying.".to_string()
                })?;
            match event {
                Frame::Reply {
                    id: reply_id,
                    result,
                } if id == reply_id => return result,
                Frame::Disconnected { message } => return Err(message),
                _ => (),
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.closing.store(true, Ordering::Relaxed);
        let unanswered = self.waiting.lock().unwrap().len();
        log::info(
            "closed",
            &[("target", &self.target), ("unanswered", &unanswered)],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn repeated_preview_outcomes_are_one_change_each() {
        let mut replies = Replies::new("player");
        let frame = Ok(json!({"frame": "ascii"}));
        let covered = Err("window is covered".to_string());
        assert!(replies.preview_changed(&frame));
        assert!(!replies.preview_changed(&frame));
        assert!(replies.preview_changed(&covered));
        assert!(!replies.preview_changed(&covered));
        assert!(replies.preview_changed(&frame));
    }

    #[test]
    fn the_clock_moving_is_not_a_state_change() {
        let at = |time_pos: f64| {
            json!({"phase":"playing","round_pos":0,"round":[{"show_name":"A","episode_id":"e"}],
                   "playback":{"paused":false,"time_pos":time_pos},"alerts":[]})
        };
        assert_eq!(playback_state(&at(1.0)), playback_state(&at(2.0)));
        let mut paused = at(2.0);
        paused["playback"]["paused"] = json!(true);
        assert_ne!(playback_state(&at(2.0)), playback_state(&paused));
    }

    #[test]
    fn a_reply_summary_never_carries_its_body() {
        let status = json!({"phase":"playing","round":[{"show_name":"A"},{"show_name":"Hidden"}],"round_pos":0});
        assert!(!reply_summary(&status).contains("Hidden"));
        assert_eq!(reply_summary(&json!([{"show_name":"Hidden"}])), "1 entries");
        assert_eq!(reply_summary(&json!({"frame":"Hidden"})), "accepted");
        assert_eq!(reply_summary(&json!({"message":"Paused"})), "Paused");
    }
}
