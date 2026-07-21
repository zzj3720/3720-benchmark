use std::env;
use std::io::{BufReader, Read, Write};
use std::net::TcpStream;
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

const API_VERSION: &str = "parabox-api-v3";
const DEFAULT_ADDRESSES: [&str; 2] = ["127.0.0.1:3720", "game:3720"];

fn connect() -> Result<TcpStream, String> {
    let configured = env::var("PARABOX_API_ADDR").ok();
    let addresses: Vec<&str> = configured
        .as_deref()
        .map(|address| vec![address])
        .unwrap_or_else(|| DEFAULT_ADDRESSES.to_vec());
    let mut last_error = None;
    for _ in 0..40 {
        for address in &addresses {
            match TcpStream::connect(address) {
                Ok(stream) => return Ok(stream),
                Err(error) => last_error = Some(format!("{address}: {error}")),
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err(format!(
        "game API is unavailable ({})",
        last_error.unwrap_or_else(|| "no address configured".to_string())
    ))
}

fn run() -> Result<i32, String> {
    let arguments: Vec<_> = env::args().skip(1).collect();
    if arguments
        .iter()
        .any(|argument| argument.chars().any(|ch| ch.is_whitespace()))
    {
        return Err("arguments cannot contain whitespace".to_string());
    }

    let mut stream = connect()?;
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(|error| format!("failed to configure game API: {error}"))?;
    writeln!(stream, "{}", arguments.join(" "))
        .map_err(|error| format!("failed to call game API: {error}"))?;

    let mut reader = BufReader::new(stream);
    let mut output = String::new();
    reader
        .by_ref()
        .take(1_000_001)
        .read_to_string(&mut output)
        .map_err(|error| format!("failed to read game API response: {error}"))?;
    if output.len() > 1_000_000 {
        return Err("game API response exceeds the client limit".to_string());
    }
    let response: Value = serde_json::from_str(&output)
        .map_err(|error| format!("game API returned malformed JSON: {error}"))?;
    let ok = response
        .get("ok")
        .and_then(Value::as_bool)
        .ok_or_else(|| "game API JSON does not contain a boolean ok field".to_string())?;
    if ok {
        std::io::stdout()
            .write_all(output.as_bytes())
            .map_err(|error| format!("failed to print game output: {error}"))?;
    } else {
        std::io::stderr()
            .write_all(output.as_bytes())
            .map_err(|error| format!("failed to print game error: {error}"))?;
    }
    Ok(if ok { 0 } else { 2 })
}

fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!(
                "{}",
                json!({
                    "api_version": API_VERSION,
                    "ok": false,
                    "command": "client",
                    "error": {
                        "code": "client_error",
                        "message": error,
                    },
                })
            );
            std::process::exit(2);
        }
    }
}
