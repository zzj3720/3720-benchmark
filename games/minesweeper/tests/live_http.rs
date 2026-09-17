use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use minesweeper_benchmark::{Board, Campaign, Cell};
use serde_json::{Value, json};

struct ServerGuard(Child);

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn public_client_solves_resumes_observes_and_replays_a_campaign_path() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let campaign = root.join("data/campaign/minesweeper.json");
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

    let client = Command::new(env!("CARGO_BIN_EXE_minesweeper"))
        .arg("show")
        .env("MINESWEEPER_URL", format!("http://127.0.0.1:{port}"))
        .output()
        .expect("run packaged client");
    assert_success(client, "client");

    let (headers, rejection) = raw_request(
        port,
        "POST",
        "/v1/select",
        Some(json!({"level": "cadet-01", "future": true})),
    );
    assert!(headers.starts_with("HTTP/1.1 400"));
    assert_eq!(rejection["error"]["code"], "invalid_request");

    let frozen = Campaign::load(&campaign).expect("campaign");
    let level = frozen.find_level("cadet-01").expect("level").2.clone();
    let first = Cell { row: 2, column: 2 };
    let mut reference = Board::new(&level);
    reference.reveal(&[first]).expect("reference first reveal");
    let mine = reference
        .generated_mines()
        .expect("generated")
        .iter()
        .position(|value| *value)
        .expect("mine");
    assert_eq!(
        request(
            port,
            "POST",
            "/v1/select",
            Some(json!({"level": "cadet-01"}))
        )["ok"],
        true
    );
    assert_eq!(
        request(port, "POST", "/v1/reveal", Some(json!({"cells": [first]})))["ok"],
        true
    );
    let loss = request(
        port,
        "POST",
        "/v1/reveal",
        Some(json!({"cells": [{
            "row": mine / level.width,
            "column": mine % level.width
        }]})),
    );
    assert_eq!(loss["data"]["state"]["board"]["status"], "lost");
    assert_eq!(loss["data"]["state"]["level"]["failed"], true);
    let retry = request(
        port,
        "POST",
        "/v1/select",
        Some(json!({"level": "cadet-01"})),
    );
    assert_eq!(retry["ok"], false);
    assert_eq!(retry["state"]["level"]["failed"], true);
    let (headers, reset) = raw_request(port, "POST", "/v1/reset", None);
    assert!(headers.starts_with("HTTP/1.1 404"));
    assert_eq!(reset["error"]["code"], "not_found");

    let client_path = Path::new(env!("CARGO_BIN_EXE_minesweeper"));
    let existing_path = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![client_path.parent().expect("client parent").to_path_buf()];
    paths.extend(std::env::split_paths(&existing_path));
    let executable_path = std::env::join_paths(paths).expect("join PATH");
    let oracle = Command::new("bash")
        .arg(root.join("../../tasks/minesweeper/solution/solve.sh"))
        .env("MINESWEEPER_URL", format!("http://127.0.0.1:{port}"))
        .env("PATH", executable_path)
        .output()
        .expect("run solution through public client");
    assert_success(oracle, "oracle");

    let snapshot = request(port, "GET", "/v1/observe/snapshot", None);
    assert_eq!(snapshot["schema"], "benchmark-observer-snapshot-v1");
    assert_eq!(snapshot["state"]["schema"], "minesweeper-state-v4");
    assert_eq!(
        snapshot["state"]["guarantee"]["difficulty_metric"],
        "proof-profile-v1"
    );
    assert_eq!(
        snapshot["state"]["guarantee"]["distribution_metric"],
        "tier-opening-distribution-v1"
    );
    assert_eq!(snapshot["state"]["campaign"]["score"], 5);
    assert_eq!(snapshot["state"]["campaign"]["complete"], true);
    let proof = &snapshot["state"]["board"]["proof"];
    assert!(
        proof["opening_revealed"].as_u64().expect("opening") * 100
            >= proof["safe_cells"].as_u64().expect("safe cells") * 3
    );
    assert!(
        proof["opening_revealed"].as_u64().expect("opening") * 100
            <= proof["safe_cells"].as_u64().expect("safe cells") * 25
    );
    assert!(proof["proof_rounds"].as_u64().expect("proof rounds") >= 16);
    assert!(proof["subset_rounds"].as_u64().expect("subset rounds") >= 13);
    assert!(proof["max_frontier"].as_u64().expect("frontier") >= 30);
    assert!(
        snapshot["state"]["tiers"]
            .as_array()
            .is_some_and(|tiers| tiers.iter().all(|tier| tier["status"] == "passed"))
    );
    let batch = request(port, "GET", "/v1/observe/events?after=0", None);
    assert!(batch["events"].as_array().is_some_and(|events| {
        events
            .iter()
            .any(|event| event["score_delta"] == 1 && event["state"]["board"]["status"] == "won")
    }));
    let sequence = batch["latest_sequence"].as_u64().expect("sequence");
    drop(server);

    let mut server = start_server(&campaign, &audit, &events, &recorder_inbox, &address);
    wait_until_healthy(port, &mut server);
    let restored = request(port, "GET", "/v1/observe/snapshot", None);
    assert_eq!(restored["state"]["campaign"]["score"], 5);
    let resumed = request(
        port,
        "GET",
        &format!("/v1/observe/events?after={sequence}"),
        None,
    );
    assert_eq!(resumed["events"][0]["type"], "sidecar_resumed");
    drop(server);

    let verified = Command::new(env!("CARGO_BIN_EXE_minesweeper-verifier"))
        .arg(&campaign)
        .arg(&audit)
        .output()
        .expect("run verifier");
    assert_success(verified, "verifier");

    let tampered = run_dir.join("tampered.jsonl");
    let mut lines = fs::read_to_string(&audit)
        .expect("read audit")
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut record: Value = serde_json::from_str(&lines[1]).expect("audit record");
    record["response"]["ok"] = Value::Bool(false);
    lines[1] = serde_json::to_string(&record).expect("serialize tamper");
    fs::write(&tampered, lines.join("\n") + "\n").expect("write tampered audit");
    let rejected = Command::new(env!("CARGO_BIN_EXE_minesweeper-verifier"))
        .arg(&campaign)
        .arg(&tampered)
        .output()
        .expect("run verifier against tamper");
    assert!(!rejected.status.success(), "tampered audit was accepted");
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
        Command::new(env!("CARGO_BIN_EXE_minesweeper-server"))
            .env("MINESWEEPER_CAMPAIGN", campaign)
            .env("MINESWEEPER_AUDIT", audit)
            .env("MINESWEEPER_EVENTS", events)
            .env("BENCHMARK_OBSERVER_INBOX", recorder_inbox)
            .env("MINESWEEPER_LISTEN_ADDR", address)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start server"),
    )
}

fn wait_until_healthy(port: u16, server: &mut ServerGuard) {
    for _ in 0..100 {
        if server.0.try_wait().expect("poll server").is_some() {
            let mut stderr = String::new();
            server
                .0
                .stderr
                .as_mut()
                .expect("stderr")
                .read_to_string(&mut stderr)
                .expect("read stderr");
            panic!("server exited: {stderr}");
        }
        if raw_request(port, "GET", "/health", None).1["ok"] == true {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("server did not become healthy");
}

fn request(port: u16, method: &str, path: &str, body: Option<Value>) -> Value {
    raw_request(port, method, path, body).1
}

fn raw_request(port: u16, method: &str, path: &str, body: Option<Value>) -> (String, Value) {
    let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) else {
        return (String::new(), Value::Null);
    };
    let encoded = body
        .map(|value| serde_json::to_vec(&value).expect("JSON"))
        .unwrap_or_default();
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        encoded.len()
    )
    .expect("write request");
    stream.write_all(&encoded).expect("write body");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read response");
    let (headers, body) = response.split_once("\r\n\r\n").expect("HTTP response");
    (
        headers.to_owned(),
        serde_json::from_str(body).expect("response JSON"),
    )
}

fn assert_success(output: Output, label: &str) {
    assert!(
        output.status.success(),
        "{label} failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn unused_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind port")
        .local_addr()
        .expect("port")
        .port()
}

fn unique_run_dir() -> PathBuf {
    std::env::temp_dir().join(format!(
        "minesweeper-e2e-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ))
}
