use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

struct ServerGuard(Child);

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn live_clock_alarm_observer_and_offline_replay_use_the_public_http_surface() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let campaign = root.join("tests/data/fast.json");
    let run_dir = unique_run_dir();
    fs::create_dir_all(&run_dir).expect("create test run directory");
    let audit = run_dir.join("audit.jsonl");
    let events = run_dir.join("events.jsonl");
    let recorder_inbox = run_dir.join("game-inbox.jsonl");
    fs::File::create(&recorder_inbox).expect("create recorder inbox");
    let port = unused_port();
    let address = format!("127.0.0.1:{port}");
    let child = Command::new(env!("CARGO_BIN_EXE_operator-server"))
        .env("OPERATOR_CAMPAIGN", &campaign)
        .env("OPERATOR_AUDIT", &audit)
        .env("OPERATOR_EVENTS", &events)
        .env("BENCHMARK_OBSERVER_INBOX", &recorder_inbox)
        .env("OPERATOR_LISTEN_ADDR", &address)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start operator server");
    let mut server = ServerGuard(child);
    wait_until_healthy(port, &mut server);

    let client = Command::new(env!("CARGO_BIN_EXE_operator"))
        .arg("status")
        .env("OPERATOR_URL", format!("http://127.0.0.1:{port}"))
        .output()
        .expect("run packaged client surface");
    assert!(
        client.status.success(),
        "client failed: {}",
        String::from_utf8_lossy(&client.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&client.stdout).expect("client JSON")["ok"],
        true
    );

    let (headers, rejection) = raw_request(
        port,
        "POST",
        "/v1/answer",
        Some(json!({"call": "medical-1", "at_ms": 1000})),
    );
    assert!(headers.starts_with("HTTP/1.1 400"));
    assert_eq!(rejection["error"]["code"], "invalid_request");

    assert_ok(request(port, "POST", "/v1/start", None));
    assert_ok(request(
        port,
        "POST",
        "/v1/answer",
        Some(json!({"call": "medical-1"})),
    ));
    assert_ok(request(
        port,
        "POST",
        "/v1/say",
        Some(json!({"call": "medical-1", "choice": "locate"})),
    ));
    assert_ok(request(
        port,
        "POST",
        "/v1/dispatch",
        Some(json!({"unit": "medic-1", "incident": "patient"})),
    ));
    let alarm = assert_ok(request(
        port,
        "POST",
        "/v1/alarm",
        Some(json!({"id": "check", "after_ms": 1000, "note": "observe again"})),
    ));
    assert_ne!(alarm["data"]["state"]["units"][0]["status"], "idle");
    assert_eq!(alarm["data"]["state"]["incidents"][0]["status"], "reported");

    let before_wait = Instant::now();
    let wake = assert_ok(request(port, "GET", "/v1/wake?wait_ms=2000", None));
    assert!(before_wait.elapsed() >= Duration::from_millis(850));
    assert_eq!(wake["data"]["alarms"][0]["id"], "check");
    assert_eq!(wake["data"]["state"]["incidents"][0]["status"], "resolved");
    assert_eq!(wake["data"]["state"]["campaign"]["score"], 120);

    let snapshot = request(port, "GET", "/v1/observe/snapshot", None);
    assert_eq!(snapshot["schema"], "benchmark-observer-snapshot-v1");
    assert_eq!(snapshot["state"]["campaign"]["score"], 120);
    let event_batch = request(port, "GET", "/v1/observe/events?after=0", None);
    assert_eq!(event_batch["schema"], "benchmark-observer-batch-v1");
    assert!(event_batch["events"].as_array().is_some_and(|events| {
        events
            .iter()
            .any(|event| event["action"]["command"] == "dispatch")
            && events.iter().any(|event| {
                event["action"]["command"] == "set_alarm" && event["type"] == "command"
            })
    }));
    let submit = assert_ok(request(port, "GET", "/v1/submit", None));
    assert_eq!(submit["data"]["score"], 120);

    drop(server);
    assert!(
        fs::read(&events)
            .expect("read legacy observer events")
            .is_empty()
    );
    assert!(
        !fs::read(&recorder_inbox)
            .expect("read recorder events")
            .is_empty()
    );
    let verified = Command::new(env!("CARGO_BIN_EXE_operator-verifier"))
        .arg(&campaign)
        .arg(&audit)
        .output()
        .expect("run verifier");
    assert_success(verified);

    let tampered_audit = run_dir.join("tampered-audit.jsonl");
    let mut lines = fs::read_to_string(&audit)
        .expect("read valid audit")
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut record: Value = serde_json::from_str(&lines[1]).expect("first audit record");
    record["response"]["ok"] = Value::Bool(false);
    lines[1] = serde_json::to_string(&record).expect("serialize tampered record");
    fs::write(&tampered_audit, lines.join("\n") + "\n").expect("write tampered audit");
    let rejected = Command::new(env!("CARGO_BIN_EXE_operator-verifier"))
        .arg(&campaign)
        .arg(&tampered_audit)
        .output()
        .expect("run verifier against tampered audit");
    assert!(!rejected.status.success(), "tampered audit was accepted");
    fs::remove_dir_all(run_dir).expect("remove test run directory");
}

fn unused_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind unused port")
        .local_addr()
        .expect("local address")
        .port()
}

fn unique_run_dir() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("operator-live-http-{}-{stamp}", std::process::id()))
}

fn wait_until_healthy(port: u16, server: &mut ServerGuard) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if server.0.try_wait().expect("inspect server").is_some() {
            let mut stderr = String::new();
            server
                .0
                .stderr
                .as_mut()
                .expect("server stderr")
                .read_to_string(&mut stderr)
                .expect("read server stderr");
            panic!("operator server stopped early: {stderr}");
        }
        if TcpStream::connect(("127.0.0.1", port)).is_ok()
            && request(port, "GET", "/health", None)["ok"] == true
        {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("operator server did not become healthy");
}

fn request(port: u16, method: &str, path: &str, payload: Option<Value>) -> Value {
    let (headers, response) = raw_request(port, method, path, payload);
    assert!(
        headers.starts_with("HTTP/1.1 200"),
        "unexpected response: {headers}"
    );
    response
}

fn raw_request(port: u16, method: &str, path: &str, payload: Option<Value>) -> (String, Value) {
    let body = payload.map_or_else(Vec::new, |value| {
        serde_json::to_vec(&value).expect("serialize request")
    });
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to operator server");
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .expect("write request headers");
    stream.write_all(&body).expect("write request body");
    stream.flush().expect("flush request");
    let mut response = Vec::new();
    stream.read_to_end(&mut response).expect("read response");
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("HTTP response headers");
    let headers = std::str::from_utf8(&response[..split])
        .expect("UTF-8 response headers")
        .to_owned();
    let body = serde_json::from_slice(&response[split + 4..]).expect("JSON response body");
    (headers, body)
}

fn assert_ok(response: Value) -> Value {
    assert_eq!(response["ok"], true, "failed response: {response}");
    response
}

fn assert_success(output: Output) {
    assert!(
        output.status.success(),
        "verifier failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 verifier output");
    assert!(
        stdout.contains("score: 120"),
        "unexpected verifier output: {stdout}"
    );
    assert!(stdout.contains("verified: 8 commands"));
}
