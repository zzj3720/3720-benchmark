use std::env;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

fn main() {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let Some(command) = arguments.first().map(String::as_str) else {
        fail("usage: operator <command>");
    };
    let (method, path, body) = match command {
        "show" | "status" | "submit" if arguments.len() == 1 => {
            ("GET", format!("/v1/{command}"), None)
        }
        "start" if arguments.len() == 1 => ("POST", "/v1/start".into(), None),
        "answer" if arguments.len() == 2 => (
            "POST",
            "/v1/answer".into(),
            Some(json!({"call": arguments[1]})),
        ),
        "say" if arguments.len() == 3 => (
            "POST",
            "/v1/say".into(),
            Some(json!({"call": arguments[1], "choice": arguments[2]})),
        ),
        "dispatch" if arguments.len() == 3 => (
            "POST",
            "/v1/dispatch".into(),
            Some(json!({"unit": arguments[1], "incident": arguments[2]})),
        ),
        "recall" if arguments.len() == 2 => (
            "POST",
            "/v1/recall".into(),
            Some(json!({"unit": arguments[1]})),
        ),
        "alarm" if (3..=4).contains(&arguments.len()) => {
            let seconds = arguments[2]
                .parse::<u64>()
                .unwrap_or_else(|_| fail("after_seconds must be an integer"));
            if seconds == 0 {
                fail("after_seconds must be at least 1");
            }
            (
                "POST",
                "/v1/alarm".into(),
                Some(json!({
                    "id": arguments[1],
                    "after_ms": seconds * 1000,
                    "note": arguments.get(3).map(String::as_str).unwrap_or(""),
                })),
            )
        }
        "cancel" if arguments.len() == 2 => (
            "POST",
            "/v1/alarm/cancel".into(),
            Some(json!({"id": arguments[1]})),
        ),
        "wait" if arguments.len() == 1 => ("GET", "/v1/wake".into(), None),
        _ => fail("invalid operator command or arguments"),
    };
    let result = request(method, &path, body.as_ref(), Duration::from_secs(1900));
    let ok = result.get("ok").and_then(Value::as_bool).unwrap_or(false);
    println!("{}", serde_json::to_string(&result).expect("response JSON"));
    std::process::exit(if ok { 0 } else { 2 });
}

fn request(method: &str, path: &str, body: Option<&Value>, timeout: Duration) -> Value {
    let configured = env::var("OPERATOR_URL").ok();
    let urls = configured
        .as_deref()
        .map(|value| vec![value.trim_end_matches('/').to_owned()])
        .unwrap_or_else(|| vec!["http://127.0.0.1:3720".into(), "http://game:3720".into()]);
    let mut last_error = String::new();
    for attempt in 0..40 {
        for base in &urls {
            match http(method, base, path, body, timeout) {
                Ok(value) => return value,
                Err(error) => last_error = error,
            }
        }
        if attempt != 39 {
            thread::sleep(Duration::from_millis(50));
        }
    }
    fail(&format!("operator sidecar unavailable: {last_error}"))
}

fn http(
    method: &str,
    base: &str,
    path: &str,
    body: Option<&Value>,
    timeout: Duration,
) -> Result<Value, String> {
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
        .set_read_timeout(Some(timeout))
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
