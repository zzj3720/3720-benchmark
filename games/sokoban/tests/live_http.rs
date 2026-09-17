use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc;
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
fn public_http_client_observer_resume_and_offline_replay_cover_a_solved_level() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let campaign = root.join("data/campaign/sokoban.json");
    let run_dir = unique_run_dir();
    fs::create_dir_all(&run_dir).expect("create test run directory");
    let audit = run_dir.join("audit.jsonl");
    let events = run_dir.join("events.jsonl");
    let recorder_inbox = run_dir.join("game-inbox.jsonl");
    fs::File::create(&recorder_inbox).expect("create recorder inbox");
    let port = unused_port();
    let address = format!("127.0.0.1:{port}");
    let mut server = start_server(&campaign, &audit, &events, &recorder_inbox, &address);
    wait_until_healthy(port, &mut server);

    let client = Command::new(env!("CARGO_BIN_EXE_sokoban"))
        .arg("show")
        .env("SOKOBAN_URL", format!("http://127.0.0.1:{port}"))
        .output()
        .expect("run packaged client surface");
    assert_successful_client(client);

    let (headers, rejection) = raw_request(
        port,
        "POST",
        "/v1/select",
        Some(json!({"level": "novoban-001", "future": true})),
    );
    assert!(headers.starts_with("HTTP/1.1 400"));
    assert_eq!(rejection["error"]["code"], "invalid_request");

    let locked = request(
        port,
        "POST",
        "/v1/select",
        Some(json!({"level": "sasquatch-001"})),
    );
    assert_eq!(locked["ok"], false);
    assert_eq!(locked["state"]["tiers"][2]["status"], "locked");

    let client_path = Path::new(env!("CARGO_BIN_EXE_sokoban"));
    let existing_path = std::env::var_os("PATH").unwrap_or_default();
    let mut executable_paths = vec![client_path.parent().expect("client parent").to_path_buf()];
    executable_paths.extend(std::env::split_paths(&existing_path));
    let executable_path = std::env::join_paths(executable_paths).expect("join executable path");
    let oracle = Command::new("bash")
        .arg(root.join("../../tasks/sokoban/solution/solve.sh"))
        .env("SOKOBAN_URL", format!("http://127.0.0.1:{port}"))
        .env("SOKOBAN_ORACLE_LIMIT", "25")
        .env("PATH", executable_path)
        .output()
        .expect("run reference solution through the packaged client");
    assert_successful_oracle(oracle);

    let snapshot = request(port, "GET", "/v1/observe/snapshot", None);
    assert_eq!(snapshot["schema"], "benchmark-observer-snapshot-v1");
    assert_eq!(snapshot["state"]["campaign"]["score"], 25);
    assert_eq!(snapshot["state"]["tiers"][1]["status"], "unlocked");
    let batch = request(port, "GET", "/v1/observe/events?after=0", None);
    assert_eq!(batch["schema"], "benchmark-observer-batch-v1");
    assert!(batch["events"].as_array().is_some_and(|events| {
        events.iter().any(|event| {
            event["action"]["command"] == "move"
                && event["score_delta"] == 1
                && event["state"]["board"]["solved"] == true
                && event["instruction_trace"]["encoding"] == "gzip+base64"
                && event["instruction_trace"]["count"]
                    .as_u64()
                    .is_some_and(|count| count > 1)
        })
    }));
    let sequence_before_restart = batch["latest_sequence"]
        .as_u64()
        .expect("latest event sequence");
    drop(server);

    let mut server = start_server(&campaign, &audit, &events, &recorder_inbox, &address);
    wait_until_healthy(port, &mut server);
    let restored = request(port, "GET", "/v1/observe/snapshot", None);
    assert_eq!(restored["state"]["campaign"]["score"], 25);
    assert_eq!(restored["state"]["level"]["id"], "novoban-025");
    let resumed = request(
        port,
        "GET",
        &format!("/v1/observe/events?after={sequence_before_restart}"),
        None,
    );
    assert_eq!(resumed["events"][0]["type"], "sidecar_resumed");
    assert_eq!(resumed["events"][0]["score"], 25);
    let submit = assert_ok(request(port, "GET", "/v1/submit", None));
    assert_eq!(submit["data"]["score"], 25);
    assert_eq!(submit["data"]["max_score"], 305);

    drop(server);
    assert!(!fs::read(&events).expect("read event history").is_empty());
    assert!(
        !fs::read(&recorder_inbox)
            .expect("read recorder events")
            .is_empty()
    );
    let verified = Command::new(env!("CARGO_BIN_EXE_sokoban-verifier"))
        .arg(&campaign)
        .arg(&audit)
        .output()
        .expect("run verifier");
    assert_successful_verifier(verified);

    let tampered = run_dir.join("tampered.jsonl");
    let mut lines = fs::read_to_string(&audit)
        .expect("read valid audit")
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut record: Value = serde_json::from_str(&lines[2]).expect("locked audit record");
    record["response"]["ok"] = Value::Bool(true);
    lines[2] = serde_json::to_string(&record).expect("serialize tampered record");
    fs::write(&tampered, lines.join("\n") + "\n").expect("write tampered audit");
    let rejected = Command::new(env!("CARGO_BIN_EXE_sokoban-verifier"))
        .arg(&campaign)
        .arg(&tampered)
        .output()
        .expect("run verifier against tampered audit");
    assert!(!rejected.status.success(), "tampered audit was accepted");
    fs::remove_dir_all(run_dir).expect("remove test run directory");
}

#[test]
fn client_does_not_retry_a_mutation_after_losing_its_response() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let campaign = root.join("data/campaign/sokoban.json");
    let run_dir = unique_run_dir();
    fs::create_dir_all(&run_dir).expect("create test run directory");
    let audit = run_dir.join("audit.jsonl");
    let events = run_dir.join("events.jsonl");
    let recorder_inbox = run_dir.join("game-inbox.jsonl");
    fs::File::create(&recorder_inbox).expect("create recorder inbox");
    let game_port = unused_port();
    let game_address = format!("127.0.0.1:{game_port}");
    let mut server = start_server(&campaign, &audit, &events, &recorder_inbox, &game_address);
    wait_until_healthy(game_port, &mut server);
    assert_ok(request(
        game_port,
        "POST",
        "/v1/select",
        Some(json!({"level": "novoban-001"})),
    ));

    let proxy = TcpListener::bind("127.0.0.1:0").expect("bind response-loss proxy");
    let proxy_port = proxy.local_addr().expect("proxy address").port();
    proxy
        .set_nonblocking(true)
        .expect("nonblocking response-loss proxy");
    let (count_sender, count_receiver) = mpsc::channel();
    let proxy_thread = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut connections = 0;
        let mut second_connection_at = None;
        while Instant::now() < deadline {
            match proxy.accept() {
                Ok((mut client, _)) => {
                    connections += 1;
                    let request = read_http_request(&mut client);
                    if connections <= 2 {
                        let mut game =
                            TcpStream::connect(("127.0.0.1", game_port)).expect("connect game");
                        game.write_all(&request).expect("forward request");
                        game.flush().expect("flush forwarded request");
                        let mut response = Vec::new();
                        game.read_to_end(&mut response).expect("read game response");
                        if connections == 1 {
                            client.write_all(&response).expect("return health response");
                        } else {
                            second_connection_at = Some(Instant::now());
                        }
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if second_connection_at
                        .is_some_and(|at| at.elapsed() >= Duration::from_millis(300))
                    {
                        break;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept response-loss proxy connection: {error}"),
            }
        }
        count_sender
            .send(connections)
            .expect("send connection count");
    });

    let client = Command::new(env!("CARGO_BIN_EXE_sokoban"))
        .args(["move", "down"])
        .env("SOKOBAN_URL", format!("http://127.0.0.1:{proxy_port}"))
        .output()
        .expect("run client through response-loss proxy");
    assert!(
        !client.status.success(),
        "lost response was reported as success"
    );
    assert!(
        String::from_utf8_lossy(&client.stderr).contains("command was not retried"),
        "unexpected client error: {}",
        String::from_utf8_lossy(&client.stderr)
    );
    proxy_thread.join().expect("join response-loss proxy");
    assert_eq!(
        count_receiver.recv().expect("receive connection count"),
        2,
        "client connected more than once for the mutation"
    );
    let snapshot = assert_ok(request(game_port, "GET", "/v1/show", None));
    assert_eq!(snapshot["data"]["board"]["moves"], 1);
    drop(server);
    fs::remove_dir_all(run_dir).expect("remove test run directory");
}

fn start_server(
    campaign: &Path,
    audit: &Path,
    events: &Path,
    recorder_inbox: &Path,
    address: &str,
) -> ServerGuard {
    ServerGuard(
        Command::new(env!("CARGO_BIN_EXE_sokoban-server"))
            .env("SOKOBAN_CAMPAIGN", campaign)
            .env("SOKOBAN_AUDIT", audit)
            .env("SOKOBAN_EVENTS", events)
            .env("BENCHMARK_OBSERVER_INBOX", recorder_inbox)
            .env("SOKOBAN_LISTEN_ADDR", address)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start sokoban server"),
    )
}

fn read_http_request(stream: &mut TcpStream) -> Vec<u8> {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("configure proxy read timeout");
    let mut request = Vec::new();
    let mut buffer = [0; 4096];
    let mut expected = None;
    loop {
        let read = stream.read(&mut buffer).expect("read proxied request");
        assert!(read > 0, "client closed before sending a complete request");
        request.extend_from_slice(&buffer[..read]);
        if expected.is_none()
            && let Some(split) = request.windows(4).position(|window| window == b"\r\n\r\n")
        {
            let headers = std::str::from_utf8(&request[..split]).expect("UTF-8 request headers");
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length: ")
                        .and_then(|value| value.parse::<usize>().ok())
                })
                .unwrap_or(0);
            expected = Some(split + 4 + content_length);
        }
        if expected.is_some_and(|expected| request.len() >= expected) {
            return request;
        }
    }
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
    std::env::temp_dir().join(format!("sokoban-live-http-{}-{stamp}", std::process::id()))
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
            panic!("sokoban server stopped early: {stderr}");
        }
        if TcpStream::connect(("127.0.0.1", port)).is_ok()
            && request(port, "GET", "/health", None)["ok"] == true
        {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("sokoban server did not become healthy");
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
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to sokoban server");
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

fn assert_successful_client(output: Output) {
    assert!(
        output.status.success(),
        "client failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).expect("client JSON")["ok"],
        true
    );
}

fn assert_successful_verifier(output: Output) {
    assert!(
        output.status.success(),
        "verifier failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 verifier output");
    assert!(stdout.contains("score: 25"), "unexpected output: {stdout}");
    assert!(stdout.contains("max_score: 305"));
    assert!(stdout.contains("verified:"));
}

fn assert_successful_oracle(output: Output) {
    assert!(
        output.status.success(),
        "oracle failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let last = output
        .stdout
        .split(|byte| *byte == b'\n')
        .rfind(|line| !line.is_empty())
        .expect("oracle output");
    let submit: Value = serde_json::from_slice(last).expect("oracle submit JSON");
    assert_eq!(submit["data"]["score"], 25);
    assert_eq!(submit["data"]["state"]["tiers"][1]["status"], "unlocked");
}
