use std::collections::HashMap;
use std::fmt::Display;
use std::io::{BufReader, Write};
use std::process::{Child, ChildStdin, Command as ProcessCommand, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use shows_control::{Command, Frame, Request, read_line};

use crate::log;

pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(12);

pub fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.as_bytes()[0].is_ascii_alphanumeric()
        && host
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'.')
}

pub fn remote_command(channel: &str) -> String {
    assert!(matches!(channel, "stable" | "dev"));
    // The independently verified channel exposes the binary directly. Neither
    // JSON commands nor show names enter the remote command line.
    format!(r#""%LOCALAPPDATA%\shows-remote\channels\{channel}\shows-remote.exe" --agent"#)
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

pub struct Session {
    child: Child,
    input: ChildStdin,
    pub events: Receiver<Frame>,
    next_id: u64,
    target: String,
    waiting: Waiting,
    closing: Arc<AtomicBool>,
}

impl Session {
    pub fn connect(host: &str, channel: &str, local: bool) -> Result<Self, String> {
        if !valid_host(host) {
            log::warn("connect-refused", &[("target", &host)]);
            return Err("target must be a computer hostname or SSH host alias".into());
        }
        let mut command = if local {
            let mut cmd = ProcessCommand::new(std::env::current_exe().map_err(|e| e.to_string())?);
            cmd.arg("--agent");
            cmd
        } else {
            let mut cmd = ProcessCommand::new("ssh.exe");
            cmd.args([
                "-T",
                "-o",
                "BatchMode=yes",
                "-o",
                "StrictHostKeyChecking=yes",
                "-o",
                "ConnectTimeout=8",
                "-o",
                "ServerAliveInterval=5",
                "-o",
                "ServerAliveCountMax=2",
                host,
                &remote_command(channel),
            ]);
            cmd
        };
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                let message = format!(
                    "could not start {}: {e}",
                    if local {
                        "local agent"
                    } else {
                        "Windows OpenSSH"
                    }
                );
                log::error("connect-failed", &[("target", &host), ("error", &message)]);
                message
            })?;
        log::info(
            "connect",
            &[
                ("target", &host),
                ("via", &if local { "local-agent" } else { "ssh" }),
                ("channel", &channel),
                ("child", &child.id()),
            ],
        );
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let errors = child.stderr.take().unwrap();
        let (tx, events) = mpsc::sync_channel(32);
        let diagnostics = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let capture = diagnostics.clone();
        let waiting: Waiting = Arc::default();
        let closing = Arc::new(AtomicBool::new(false));
        let target = host.to_owned();
        let (answered, closed) = (waiting.clone(), closing.clone());
        std::thread::spawn(move || {
            let mut reader = BufReader::new(errors);
            while let Ok(Some(line)) = read_line(&mut reader) {
                let mut text = capture.lock().unwrap();
                if text.len() < 8192 {
                    text.push_str(&line);
                }
            }
        });
        std::thread::spawn(move || {
            let mut reader = BufReader::new(output);
            let mut replies = Replies::new(&target);
            let mut state = String::new();
            let result = loop {
                match read_line(&mut reader) {
                    Ok(Some(line)) => match serde_json::from_str(&line) {
                        Ok(frame) => {
                            match &frame {
                                Frame::Reply { id, result } => {
                                    let sent = answered.lock().unwrap().remove(id);
                                    match sent {
                                        Some((command, began)) => {
                                            replies.logged(*id, &command, began, result)
                                        }
                                        None => log::warn(
                                            "unexpected-reply",
                                            &[("target", &target), ("id", id)],
                                        ),
                                    }
                                }
                                Frame::Status { status } => {
                                    let now = playback_state(status);
                                    if now != state {
                                        log::info(
                                            "status",
                                            &[("target", &target), ("state", &now)],
                                        );
                                        state = now;
                                    }
                                }
                                Frame::Disconnected { message } => log::warn(
                                    "player-lost",
                                    &[("target", &target), ("message", message)],
                                ),
                                Frame::Heartbeat => (),
                            }
                            if tx.send(frame).is_err() {
                                return;
                            }
                        }
                        Err(e) => break format!("invalid remote protocol message: {e}"),
                    },
                    Ok(None) => break "remote connection closed".to_string(),
                    Err(e) => break e,
                }
            };
            // stderr may finish just after stdout; give its reader a bounded chance.
            std::thread::sleep(Duration::from_millis(50));
            let details = diagnostics.lock().unwrap().trim().to_string();
            let message = if details.is_empty() {
                result
            } else {
                format!("{result}: {details}")
            };
            if !closed.load(Ordering::Relaxed) {
                log::warn(
                    "connection-lost",
                    &[("target", &target), ("message", &message)],
                );
            }
            let _ = tx.send(Frame::Disconnected { message });
        });
        Ok(Self {
            child,
            input,
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
        let request = Request { id, command };
        self.write(&request).inspect_err(|error| {
            self.waiting.lock().unwrap().remove(&id);
            log::warn(
                "send-failed",
                &[
                    ("target", &self.target),
                    ("id", &id),
                    ("command", &label),
                    ("error", error),
                ],
            );
        })?;
        Ok(id)
    }

    fn write(&mut self, request: &Request) -> Result<(), String> {
        serde_json::to_writer(&mut self.input, request).map_err(|e| e.to_string())?;
        self.input
            .write_all(b"\n")
            .and_then(|_| self.input.flush())
            .map_err(|e| e.to_string())
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
        let _ = self.child.kill();
        let _ = self.child.wait();
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
