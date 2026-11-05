use std::io::{BufReader, Write};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::json;
use shows_control::{Client, Command, Frame, Request, control_addr, read_line};
use shows_remote::log;
use shows_remote::transport::Replies;

fn emit(output: &Mutex<std::io::Stdout>, frame: Frame) -> Result<(), String> {
    let mut output = output.lock().unwrap();
    serde_json::to_writer(&mut *output, &frame).map_err(|e| e.to_string())?;
    output
        .write_all(b"\n")
        .and_then(|_| output.flush())
        .map_err(|e| e.to_string())
}

pub fn run() -> Result<(), String> {
    let address = control_addr()?;
    let client = Client::connect().inspect_err(|error| {
        log::error(
            "player-unavailable",
            &[("address", &address), ("error", error)],
        );
    })?;
    log::info("player-found", &[("address", &address)]);
    let output = Arc::new(Mutex::new(std::io::stdout()));
    let mut input = BufReader::new(std::io::stdin());
    let mut watching = false;
    let mut replies = Replies::new("player");
    while let Some(line) = read_line(&mut input)? {
        let request: Request =
            serde_json::from_str(&line).map_err(|e| format!("invalid request: {e}"))?;
        let began = Instant::now();
        if request.command != (Command::Preview {}) {
            let command = format!("{:?}", request.command);
            log::info("request", &[("id", &request.id), ("command", &command)]);
        }
        let result = if request.command == (Command::Watch {}) {
            if watching {
                Err("already watching".into())
            } else {
                watching = true;
                let (client, output) = (client.clone(), output.clone());
                std::thread::spawn(move || {
                    let result = client.watch(|frame| emit(&output, frame));
                    if let Err(message) = result {
                        log::warn("watch-ended", &[("message", &message)]);
                        let _ = emit(&output, Frame::Disconnected { message });
                    }
                });
                Ok(json!({"message": "subscribed to the target's live status"}))
            }
        } else {
            client.execute(&request.command)
        };
        replies.logged(request.id, &request.command, began, &result);
        emit(
            &output,
            Frame::Reply {
                id: request.id,
                result,
            },
        )?;
    }
    log::info("client-closed", &[]);
    Ok(())
}
