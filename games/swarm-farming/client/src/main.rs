use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::thread;
use std::time::Duration;

fn main() {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let Some(command) = arguments.first().map(String::as_str) else {
        fail("usage: swarm <status|run|advance|submit>");
    };
    let (method, path, body) = match command {
        "status" if arguments.len() == 1 => ("GET", "/v1/status".into(), Vec::new()),
        "run" if arguments.len() == 2 => {
            let source = if arguments[1] == "-" {
                let mut source = Vec::new();
                io::stdin().read_to_end(&mut source).expect("stdin");
                source
            } else {
                fs::read(&arguments[1]).unwrap_or_else(|error| fail(&error.to_string()))
            };
            ("POST", "/v1/run".into(), source)
        }
        "advance" if arguments.len() == 2 => {
            let ticks = arguments[1]
                .parse::<u64>()
                .unwrap_or_else(|_| fail("ticks must be an integer"));
            ("POST", format!("/v1/advance/{ticks}"), Vec::new())
        }
        "submit" if arguments.len() == 1 => ("GET", "/v1/submit".into(), Vec::new()),
        _ => fail("invalid swarm command or arguments"),
    };
    let response = request(method, &path, &body);
    io::stdout().write_all(&response).expect("stdout");
}

fn request(method: &str, path: &str, body: &[u8]) -> Vec<u8> {
    let configured = env::var("SWARM_URL").ok();
    let urls = configured
        .as_deref()
        .map(|value| vec![value.trim_end_matches('/').to_owned()])
        .unwrap_or_else(|| vec!["http://127.0.0.1:3720".into(), "http://game:3720".into()]);
    let mut last_error = String::new();
    for attempt in 0..30 {
        for base in &urls {
            match http(method, base, path, body) {
                Ok(value) => return value,
                Err(error) => last_error = error,
            }
        }
        if attempt != 29 {
            thread::sleep(Duration::from_millis(200));
        }
    }
    fail(&format!("swarm sidecar unavailable: {last_error}"))
}

fn http(method: &str, base: &str, path: &str, body: &[u8]) -> Result<Vec<u8>, String> {
    let authority = base
        .strip_prefix("http://")
        .ok_or("only http:// sidecars are supported")?;
    let (host, port) = authority
        .split_once(':')
        .map(|(host, port)| (host, port.parse::<u16>().map_err(|error| error.to_string())))
        .unwrap_or((authority, Ok(80)));
    let address = (host, port?)
        .to_socket_addrs()
        .map_err(|error| error.to_string())?
        .next()
        .ok_or("sidecar address did not resolve")?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(1))
        .map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(7200)))
        .map_err(|error| error.to_string())?;
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .map_err(|error| error.to_string())?;
    stream.write_all(body).map_err(|error| error.to_string())?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|error| error.to_string())?;
    let header_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or("invalid HTTP response")?;
    let headers = &response[..header_end];
    let body = &response[header_end + 4..];
    if headers
        .split(|byte| *byte == b'\n')
        .any(|line| line.eq_ignore_ascii_case(b"transfer-encoding: chunked\r"))
    {
        decode_chunked(body)
    } else {
        Ok(body.to_vec())
    }
}

fn decode_chunked(mut encoded: &[u8]) -> Result<Vec<u8>, String> {
    let mut decoded = Vec::new();
    loop {
        let line_end = encoded
            .windows(2)
            .position(|window| window == b"\r\n")
            .ok_or("invalid chunk size")?;
        let size_text = std::str::from_utf8(&encoded[..line_end])
            .map_err(|error| error.to_string())?
            .split(';')
            .next()
            .ok_or("missing chunk size")?;
        let size =
            usize::from_str_radix(size_text.trim(), 16).map_err(|error| error.to_string())?;
        encoded = &encoded[line_end + 2..];
        if size == 0 {
            return Ok(decoded);
        }
        if encoded.len() < size + 2 || &encoded[size..size + 2] != b"\r\n" {
            return Err("truncated HTTP chunk".into());
        }
        decoded.extend_from_slice(&encoded[..size]);
        encoded = &encoded[size + 2..];
    }
}

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(2)
}

#[cfg(test)]
mod tests {
    use super::decode_chunked;

    #[test]
    fn decodes_chunked_json() {
        assert_eq!(
            decode_chunked(b"4\r\n{\"ok\r\n7\r\n\":true}\r\n0\r\n\r\n").unwrap(),
            br#"{"ok":true}"#
        );
    }

    #[test]
    fn rejects_truncated_chunk() {
        assert!(decode_chunked(b"4\r\nabc\r\n").is_err());
    }
}
