use std::process::Command;
use std::time::Duration;

use serde_json::{Value, json};
use tiny_http::{Response, Server};

/// Every run logs to its own temporary file, never the user's real log.
fn remote(log: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_shows-remote"));
    command.env("SHOWS_REMOTE_LOG", log);
    command
}

fn session(args: &[&str], replies: Vec<(&'static str, Value)>) -> (std::process::Output, String) {
    let folder = tempfile::tempdir().unwrap();
    let log = folder.path().join("shows-remote.log");
    let server = Server::http("127.0.0.1:0").unwrap();
    let port = server.server_addr().to_ip().unwrap().port();
    let worker = std::thread::spawn(move || {
        for (path, value) in replies {
            let request = server
                .recv_timeout(Duration::from_secs(4))
                .unwrap()
                .expect("request");
            assert_eq!(request.url(), path);
            request
                .respond(Response::from_string(value.to_string()))
                .unwrap();
        }
    });
    let result = remote(&log)
        .env("SHOWS_CONTROL_PORT", port.to_string())
        .arg("--local")
        .arg("thispc")
        .args(args)
        .output()
        .unwrap();
    worker.join().unwrap();
    (result, std::fs::read_to_string(log).unwrap_or_default())
}

fn run(args: &[&str], replies: Vec<(&'static str, Value)>) -> std::process::Output {
    session(args, replies).0
}

fn identity() -> (&'static str, Value) {
    (
        "/remote/info",
        json!({"application":"shows-desktop","protocol":1}),
    )
}

#[test]
fn local_round_trip_prints_json_without_starting_a_player() {
    let output = run(
        &["status", "--json"],
        vec![
            identity(),
            ("/status", json!({"phase":"playing","message":"Télévision"})),
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let status: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(status["message"], "Télévision");
}

#[test]
fn rejected_control_exits_nonzero_and_surfaces_the_target_error() {
    let output = run(
        &["play"],
        vec![
            identity(),
            (
                "/remote/control",
                json!({"ok":false,"message":"player unavailable"}),
            ),
        ],
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("player unavailable"));
}

#[test]
fn fullscreen_uses_the_native_control_route() {
    let output = run(
        &["fullscreen"],
        vec![
            identity(),
            (
                "/remote/control",
                json!({"ok":true,"message":"fullscreen toggle requested"}),
            ),
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("fullscreen toggle requested"));
}

#[test]
fn ambiguous_show_selection_sends_no_play_request() {
    let output = run(
        &["play", "the"],
        vec![
            identity(),
            (
                "/status",
                json!({"round":[
        {"show_id":"a","show_name":"The Simpsons"}, {"show_id":"b","show_name":"The Daily Show"}]}),
            ),
        ],
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("several"));
}

#[test]
fn scripts_get_a_clear_error_instead_of_entering_raw_terminal_mode() {
    let folder = tempfile::tempdir().unwrap();
    let output = remote(&folder.path().join("shows-remote.log"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("needs a terminal"));
}

#[test]
fn one_shot_commands_never_pick_an_implicit_machine() {
    let folder = tempfile::tempdir().unwrap();
    let output = remote(&folder.path().join("shows-remote.log"))
        .args(["status", "--json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Commands require a target"));
}

#[test]
fn a_session_logs_the_target_what_was_sent_and_what_came_back() {
    let (output, log) = session(
        &["pause"],
        vec![
            identity(),
            (
                "/remote/control",
                json!({"ok":true,"message":"Paused","round":["Hidden body"]}),
            ),
        ],
    );
    assert!(output.status.success());
    for line in log.lines() {
        let (time, _) = line.split_once(' ').unwrap();
        assert!(time.ends_with('Z') && time.as_bytes()[10] == b'T', "{line}");
    }
    let positions: Vec<_> = [
        "client pid=",
        "connect target=thispc via=local",
        "send target=thispc id=1 command=Pause",
        "reply target=thispc id=1 command=Pause",
    ]
    .iter()
    .map(|wanted| {
        log.find(wanted)
            .unwrap_or_else(|| panic!("no {wanted} in {log}"))
    })
    .collect();
    assert!(positions[1] < positions[2], "{log}");
    assert!(log.contains("result=Paused"), "{log}");
    assert!(log.contains("version=\"development build"), "{log}");
    assert!(log.contains("address=127.0.0.1:"), "{log}");
    assert!(!log.contains("connection-lost"), "a clean exit: {log}");
    assert!(!log.contains("Hidden body"), "{log}");
}

#[test]
fn a_status_reply_is_logged_without_its_round() {
    let (output, log) = session(
        &["status", "--json"],
        vec![
            identity(),
            (
                "/status",
                json!({"phase":"playing","round_pos":0,"round":[
                    {"show_name":"Current","episode_id":"e1"},{"show_name":"Hidden entry"}],
                    "playback":{"paused":false}}),
            ),
        ],
    );
    assert!(output.status.success());
    assert!(log.contains("show=Current"), "{log}");
    assert!(!log.contains("Hidden entry"), "{log}");
}

#[test]
fn a_rejected_command_keeps_its_reason_and_loses_its_secrets() {
    let (output, log) = session(
        &["play"],
        vec![
            identity(),
            (
                "/remote/control",
                json!({"ok":false,"message":"player unavailable token=hunter2"}),
            ),
        ],
    );
    assert!(!output.status.success());
    assert!(log.contains("rejected target=thispc id=1"), "{log}");
    assert!(log.contains("player unavailable"), "{log}");
    assert!(!log.contains("hunter2"), "{log}");
    assert!(log.contains("ERROR client pid="), "{log}");
}

#[test]
fn no_player_says_which_instance_was_tried_and_what_came_back() {
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = closed.local_addr().unwrap().port();
    drop(closed);
    let folder = tempfile::tempdir().unwrap();
    let log = folder.path().join("shows-remote.log");
    let output = remote(&log)
        .env("SHOWS_CONTROL_PORT", port.to_string())
        .args(["--local", "thispc", "status"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let log = std::fs::read_to_string(log).unwrap();
    assert!(log.contains("connect-failed target=thispc"), "{log}");
}

#[test]
fn a_target_that_is_not_a_hostname_is_logged_and_never_started() {
    let folder = tempfile::tempdir().unwrap();
    let log = folder.path().join("shows-remote.log");
    let output = remote(&log)
        .args(["not a host", "status"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let log = std::fs::read_to_string(log).unwrap();
    assert!(log.contains("connect-refused"), "{log}");
    assert!(!log.contains(" connect target"), "{log}");
}

#[test]
fn a_log_that_cannot_be_written_leaves_the_session_and_its_pipes_alone() {
    let folder = tempfile::tempdir().unwrap();
    let server = Server::http("127.0.0.1:0").unwrap();
    let port = server.server_addr().to_ip().unwrap().port();
    let worker = std::thread::spawn(move || {
        for value in [identity().1, json!({"phase":"playing"})] {
            server
                .recv_timeout(Duration::from_secs(4))
                .unwrap()
                .expect("request")
                .respond(Response::from_string(value.to_string()))
                .unwrap();
        }
    });
    let output = remote(folder.path())
        .env("SHOWS_CONTROL_PORT", port.to_string())
        .args(["--local", "thispc", "status", "--json"])
        .output()
        .unwrap();
    worker.join().unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
}

#[test]
fn a_target_on_another_pc_needs_a_sign_in_before_anything_is_sent() {
    let folder = tempfile::tempdir().unwrap();
    let log = folder.path().join("shows-remote.log");
    let output = remote(&log)
        .env("PATH", folder.path())
        .env("SHOWS_REMOTE_PORT", "1")
        .args(["127.0.0.1", "status"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("romaine-auth"), "{stderr}");
    let log = std::fs::read_to_string(log).unwrap();
    assert!(log.contains("connect-failed target=127.0.0.1"), "{log}");
}
