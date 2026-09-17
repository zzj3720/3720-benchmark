use std::env;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};
use sokoban_benchmark::Direction;

fn main() {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let Some(command) = arguments.first().map(String::as_str) else {
        fail("usage: sokoban <show|levels|select|move|undo|reset|submit>");
    };
    let (method, path, body) = match command {
        "show" | "status" | "submit" if arguments.len() == 1 => {
            ("GET", format!("/v1/{command}"), None)
        }
        "levels" if arguments.len() <= 2 => (
            "GET",
            arguments.get(1).map_or_else(
                || "/v1/levels".to_owned(),
                |tier| format!("/v1/levels?tier={tier}"),
            ),
            None,
        ),
        "select" if arguments.len() == 2 => (
            "POST",
            "/v1/select".into(),
            Some(json!({"level": arguments[1]})),
        ),
        "move" if arguments.len() >= 2 => (
            "POST",
            "/v1/move".into(),
            Some(json!({"directions": parse_directions(&arguments[1..])})),
        ),
        "undo" if arguments.len() <= 2 => {
            let steps = arguments
                .get(1)
                .map(|value| {
                    value
                        .parse::<usize>()
                        .unwrap_or_else(|_| fail("undo steps must be an integer"))
                })
                .unwrap_or(1);
            ("POST", "/v1/undo".into(), Some(json!({"steps": steps})))
        }
        "reset" if arguments.len() == 1 => ("POST", "/v1/reset".into(), None),
        _ => fail("invalid sokoban command or arguments"),
    };
    let result = request(method, &path, body.as_ref());
    let ok = result.get("ok").and_then(Value::as_bool).unwrap_or(false);
    println!("{}", serde_json::to_string(&result).expect("response JSON"));
    std::process::exit(if ok { 0 } else { 2 });
}

fn parse_directions(arguments: &[String]) -> Vec<&'static str> {
    let mut parsed = Vec::new();
    for argument in arguments {
        if let Ok(direction) = Direction::parse(argument) {
            parsed.push(direction.as_str());
            continue;
        }
        for value in argument.chars() {
            let direction =
                Direction::parse(&value.to_string()).unwrap_or_else(|error| fail(&error));
            parsed.push(direction.as_str());
        }
    }
    if parsed.len() > 64 {
        fail("a move command accepts at most 64 directions");
    }
    parsed
}

fn request(method: &str, path: &str, body: Option<&Value>) -> Value {
    let configured = env::var("SOKOBAN_URL").ok();
    let urls = configured
        .as_deref()
        .map(|value| vec![value.trim_end_matches('/').to_owned()])
        .unwrap_or_else(|| vec!["http://127.0.0.1:3720".into(), "http://game:3720".into()]);
    let base = wait_until_ready(&urls);
    http(method, &base, path, body).unwrap_or_else(|error| {
        fail(&format!(
            "sokoban request outcome is unknown; the command was not retried: {error}"
        ))
    })
}

fn wait_until_ready(urls: &[String]) -> String {
    let mut last_error = String::new();
    for attempt in 0..40 {
        for base in urls {
            match http("GET", base, "/health", None) {
                Ok(value) if value["ok"] == true => return base.clone(),
                Ok(_) => last_error = format!("{base} returned an unhealthy response"),
                Err(error) => last_error = format!("{base}: {error}"),
            }
        }
        if attempt != 39 {
            thread::sleep(Duration::from_millis(50));
        }
    }
    fail(&format!("sokoban sidecar unavailable: {last_error}"))
}

fn http(method: &str, base: &str, path: &str, body: Option<&Value>) -> Result<Value, String> {
    let authority = base
        .strip_prefix("http://")
        .ok_or("only http:// sidecars are supported")?;
    let (host, port) = authority
        .split_once(':')
        .map(|(host, port)| (host, port.parse::<u16>().map_err(|error| error.to_string())))
        .unwrap_or((authority, Ok(80)));
    let port = port?;
    let address = (host, port)
        .to_socket_addrs()
        .map_err(|error| error.to_string())?
        .next()
        .ok_or("sidecar address did not resolve")?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(1))
        .map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(|error| error.to_string())?;
    let encoded = body
        .map(serde_json::to_vec)
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or_default();
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        encoded.len()
    )
    .map_err(|error| error.to_string())?;
    stream
        .write_all(&encoded)
        .map_err(|error| error.to_string())?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|error| error.to_string())?;
    let body = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| &response[index + 4..])
        .ok_or("invalid HTTP response")?;
    serde_json::from_slice(body).map_err(|error| error.to_string())
}

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(2)
}
