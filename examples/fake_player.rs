//! A stand-in Shows player for TESTING.md's isolated check: protocol 1 on loopback, an
//! in-memory round of three shows, controls applied to that state and pushed on the
//! status stream. Every control it receives is printed, so a check can see what the
//! remote sent. It touches no TV, database or media.
//!
//! cargo run --example fake_player -j 4 -- [port]
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

struct Player {
    status: Value,
    streams: Vec<TcpStream>,
}

type Shared = Arc<Mutex<Player>>;

fn initial() -> Value {
    let round: Vec<Value> = ["Fake Show One", "Fake Show Two", "Fake Show Three"]
        .iter()
        .enumerate()
        .map(|(i, name)| json!({"show_id": format!("fake-{i}"), "show_name": name, "episode_id": format!("episode-{i}")}))
        .collect();
    json!({
        "phase": "playing",
        "message": "Fake player for testing",
        "round_pos": 0,
        "round": round,
        "playback": {"paused": true, "time_pos": 120.0, "duration": 1320.0, "volume": 80.0},
        "alerts": [],
        "window_fullscreen": false,
    })
}

fn event(status: &Value) -> Vec<u8> {
    format!("event: status\ndata: {status}\n\n").into_bytes()
}

/// Sends to every open stream, dropping the ones whose remote has gone.
fn broadcast(player: &mut Player, bytes: &[u8]) {
    player
        .streams
        .retain_mut(|stream| stream.write_all(bytes).and_then(|_| stream.flush()).is_ok());
}

fn apply(status: &mut Value, command: &Value) -> Result<String, String> {
    let len = status["round"].as_array().map_or(0, Vec::len) as i64;
    let pos = status["round_pos"].as_i64().unwrap_or(0);
    let action = command["action"].as_str().ok_or("command has no action")?;
    let message = match action {
        "play" => {
            status["playback"]["paused"] = json!(false);
            "playing".to_owned()
        }
        "pause" => {
            status["playback"]["paused"] = json!(true);
            "paused".to_owned()
        }
        "skip" | "defer" => {
            status["round_pos"] = json!((pos + 1).rem_euclid(len));
            format!("{action}: next show")
        }
        "previous" => {
            status["round_pos"] = json!((pos - 1).rem_euclid(len));
            "previous show".to_owned()
        }
        "seek" => {
            let at = status["playback"]["time_pos"].as_f64().unwrap_or(0.0)
                + command["seconds"].as_f64().unwrap_or(0.0);
            status["playback"]["time_pos"] = json!(at.max(0.0));
            "seeked".to_owned()
        }
        "volume" => {
            status["playback"]["volume"] = command["level"].clone();
            "volume set".to_owned()
        }
        "fullscreen" => {
            let full = status["window_fullscreen"] == true;
            status["window_fullscreen"] = json!(!full);
            "fullscreen toggle requested".to_owned()
        }
        "play_show" => {
            let id = command["show_id"].as_str().unwrap_or_default();
            let found = status["round"]
                .as_array()
                .and_then(|round| round.iter().position(|show| show["show_id"] == id))
                .ok_or_else(|| format!("no show {id} in the round"))?;
            status["round_pos"] = json!(found);
            format!("navigated to {id}")
        }
        other => return Err(format!("unknown action {other}")),
    };
    Ok(message)
}

fn respond(stream: &mut TcpStream, code: &str, body: &Value) {
    let body = body.to_string();
    let _ = write!(
        stream,
        "HTTP/1.1 {code}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

fn serve(mut stream: TcpStream, player: Shared) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let mut length = 0;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 || header.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    match (method, path) {
        ("GET", "/remote/info") => respond(
            &mut stream,
            "200 OK",
            &json!({"application": "shows-desktop", "protocol": 1}),
        ),
        ("GET", "/status") => {
            let status = player.lock().unwrap().status.clone();
            respond(&mut stream, "200 OK", &status);
        }
        ("GET", "/status/stream") => {
            let mut player = player.lock().unwrap();
            stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )?;
            stream.write_all(&event(&player.status))?;
            player.streams.push(stream);
        }
        ("GET", "/remote/preview") => respond(
            &mut stream,
            "503 Service Unavailable",
            &json!({"ok": false, "message": "The fake player has no video"}),
        ),
        ("POST", "/remote/control") => {
            let command: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            println!("control {command}");
            let mut player = player.lock().unwrap();
            let result = apply(&mut player.status, &command);
            let status = player.status.clone();
            broadcast(&mut player, &event(&status));
            match result {
                Ok(message) => respond(
                    &mut stream,
                    "200 OK",
                    &json!({"ok": true, "message": message}),
                ),
                Err(message) => respond(
                    &mut stream,
                    "400 Bad Request",
                    &json!({"ok": false, "message": message}),
                ),
            }
        }
        _ => respond(
            &mut stream,
            "404 Not Found",
            &json!({"ok": false, "message": format!("no route {method} {path}")}),
        ),
    }
    Ok(())
}

fn main() {
    let port = std::env::args().nth(1).unwrap_or_else(|| "0".into());
    let listener = TcpListener::bind(format!("127.0.0.1:{port}")).expect("bind loopback");
    println!(
        "fake player on {}",
        listener.local_addr().expect("local address")
    );
    let player: Shared = Arc::new(Mutex::new(Player {
        status: initial(),
        streams: Vec::new(),
    }));
    let keepalive = player.clone();
    // The real player comments on its stream every 30 seconds; the remote drops a
    // stream silent for 65.
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(30));
            broadcast(&mut keepalive.lock().unwrap(), b": keepalive\n\n");
        }
    });
    for stream in listener.incoming().flatten() {
        let player = player.clone();
        std::thread::spawn(move || serve(stream, player));
    }
}
