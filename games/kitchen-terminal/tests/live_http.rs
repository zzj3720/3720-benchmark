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
fn live_clock_single_action_alarm_observer_and_replay_use_the_public_surface() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let data = root.join("data/overcooked-1");
    let run_dir = unique_run_dir();
    fs::create_dir_all(&run_dir).expect("create test run directory");
    let audit = run_dir.join("audit.jsonl");
    let events = run_dir.join("events.jsonl");
    let recorder_inbox = run_dir.join("game-inbox.jsonl");
    fs::File::create(&recorder_inbox).expect("create recorder inbox");
    let port = unused_port();
    let address = format!("127.0.0.1:{port}");
    let mut server = spawn_server(&data, &audit, &events, &recorder_inbox, &address);
    wait_until_healthy(port, &mut server);

    let client = Command::new(env!("CARGO_BIN_EXE_kitchen"))
        .arg("status")
        .env("KITCHEN_URL", format!("http://127.0.0.1:{port}"))
        .output()
        .expect("run packaged client surface");
    assert_success_status(&client);

    let (headers, rejection) = raw_request(
        port,
        "POST",
        "/v1/go",
        Some(json!({"target": "object-1178", "at_ms": 1000})),
    );
    assert!(headers.starts_with("HTTP/1.1 400"));
    assert_eq!(rejection["error"]["code"], "invalid_request");

    let started = assert_ok(request(port, "POST", "/v1/start", None));
    assert!(started["data"]["map"].get("walkable").is_none());
    assert_eq!(started["data"]["map"]["objects_are_sparse"], true);
    assert!(
        serde_json::to_vec(&started)
            .expect("serialize planning response")
            .len()
            < 10_000,
        "model planning state should remain compact"
    );
    let destination = started["data"]["destinations"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|destination| destination["travel_ms"].as_u64().unwrap_or(0) > 0)
        .expect("non-local destination");
    let target = destination["target"].as_str().expect("destination target");
    let travel_ms = destination["travel_ms"].as_u64().expect("travel time");
    let before_travel = Instant::now();
    let moved = assert_ok(request(
        port,
        "POST",
        "/v1/go",
        Some(json!({"target": target})),
    ));
    assert_eq!(
        moved["data"]["state"]["schema"],
        "overcooked-dynamic-state-v3"
    );
    assert!(moved["data"]["state"]["chefs"][0]["travel"].is_object());
    assert_ok(request(port, "GET", "/v1/wake?wait_ms=60000", None));
    assert!(before_travel.elapsed() >= Duration::from_millis(travel_ms.saturating_sub(100)));
    assert!(moved["data"]["state"]["map"].get("walkable").is_none());
    assert!(
        serde_json::to_vec(&moved)
            .expect("serialize dynamic response")
            .len()
            < serde_json::to_vec(&started)
                .expect("serialize complete object response")
                .len()
    );
    assert_ok(request(
        port,
        "POST",
        "/v1/alarm",
        Some(json!({"id": "look", "after_ms": 1_100, "note": "inspect the kitchen"})),
    ));
    let before_wait = Instant::now();
    let wake = assert_ok(request(port, "GET", "/v1/wake?wait_ms=2000", None));
    assert!(before_wait.elapsed() >= Duration::from_millis(950));
    assert_eq!(wake["data"]["alarms"][0]["id"], "look");

    let state = assert_ok(request(port, "GET", "/v1/show", None))["data"].clone();
    let plate = state["map"]["objects"]
        .as_array()
        .expect("objects")
        .iter()
        .find(|object| object["item"]["kind"] == "plate")
        .and_then(|object| object["id"].as_str())
        .expect("authored plate")
        .to_owned();
    go_near(port, "object-1117");
    assert_ok(request(
        port,
        "POST",
        "/v1/interact",
        Some(json!({"target": "object-1117"})),
    ));
    go_near(port, "object-1178");
    assert_ok(request(
        port,
        "POST",
        "/v1/interact",
        Some(json!({"target": "object-1178"})),
    ));
    assert_ok(request(port, "POST", "/v1/switch", None));
    go_near(port, "object-681");
    assert_ok(request(
        port,
        "POST",
        "/v1/interact",
        Some(json!({"target": "object-681"})),
    ));
    go_near(port, "object-1239");
    assert_ok(request(
        port,
        "POST",
        "/v1/interact",
        Some(json!({"target": "object-1239"})),
    ));
    let second_work = assert_ok(request(
        port,
        "POST",
        "/v1/work/start",
        Some(json!({"target": "object-1239"})),
    ));
    assert!(second_work["data"]["expected_done_ms"].as_u64().is_some());
    assert_ok(request(port, "POST", "/v1/switch", None));
    let first_work = assert_ok(request(
        port,
        "POST",
        "/v1/work/start",
        Some(json!({"target": "object-1178"})),
    ));
    assert_eq!(
        first_work["data"]["state"]["works"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    assert_ok(request(
        port,
        "POST",
        "/v1/alarm",
        Some(json!({"id": "chop", "after_ms": 15_000, "note": "finish chop"})),
    ));
    let wake = assert_ok(request(port, "GET", "/v1/wake?wait_ms=16000", None));
    assert_eq!(wake["data"]["alarms"][0]["id"], "chop");
    assert!(
        wake["data"]["state"]["works"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );
    assert_ok(request(
        port,
        "POST",
        "/v1/interact",
        Some(json!({"target": "object-1178"})),
    ));
    go_near(port, &plate);
    assert_ok(request(
        port,
        "POST",
        "/v1/interact",
        Some(json!({"target": plate})),
    ));
    assert_ok(request(port, "POST", "/v1/switch", None));
    assert_ok(request(
        port,
        "POST",
        "/v1/interact",
        Some(json!({"target": "object-1239"})),
    ));
    go_near(port, &plate);
    assert_ok(request(
        port,
        "POST",
        "/v1/interact",
        Some(json!({"target": plate})),
    ));
    assert_ok(request(
        port,
        "POST",
        "/v1/interact",
        Some(json!({"target": plate})),
    ));
    go_near(port, "object-733");
    let delivered = state_after(assert_ok(request(
        port,
        "POST",
        "/v1/interact",
        Some(json!({"target": "object-733"})),
    )));
    assert!(
        delivered["campaign"]["score"]
            .as_i64()
            .is_some_and(|score| score >= 20)
    );

    let snapshot = request(port, "GET", "/v1/observe/snapshot", None);
    assert_eq!(snapshot["schema"], "benchmark-observer-snapshot-v1");
    assert_eq!(snapshot["state"]["campaign"]["level"], 1);
    assert_eq!(snapshot["state"]["chefs"].as_array().map(Vec::len), Some(2));
    assert!(snapshot["state"]["map"]["walkable"].is_array());
    let event_batch = request(port, "GET", "/v1/observe/events?after=0", None);
    assert_eq!(event_batch["schema"], "benchmark-observer-batch-v1");
    assert!(event_batch["events"].as_array().is_some_and(|events| {
        events
            .iter()
            .any(|event| event["action"]["command"] == "go")
            && events.iter().any(|event| event["type"] == "clock")
    }));
    let before_restart = snapshot["state"].clone();
    drop(server);
    let mut server = spawn_server(&data, &audit, &events, &recorder_inbox, &address);
    wait_until_healthy(port, &mut server);
    let restored = assert_ok(request(port, "GET", "/v1/show", None))["data"].clone();
    assert_eq!(
        restored["campaign"]["score"],
        before_restart["campaign"]["score"]
    );
    assert!(
        restored["shift"]["elapsed_ms"]
            .as_u64()
            .zip(before_restart["shift"]["elapsed_ms"].as_u64())
            .is_some_and(|(restored, before)| restored >= before && restored - before < 2_000)
    );
    let restored_events = request(port, "GET", "/v1/observe/events?after=0", None);
    assert!(restored_events["events"].as_array().is_some_and(|events| {
        events
            .iter()
            .any(|event| event["type"] == "sidecar_restored")
    }));
    assert_ok(request(port, "GET", "/v1/submit", None));
    drop(server);
    assert!(fs::read(&events).expect("legacy events").is_empty());
    assert!(
        !fs::read(&recorder_inbox)
            .expect("observer events")
            .is_empty()
    );
    let verified = Command::new(env!("CARGO_BIN_EXE_kitchen-verifier"))
        .arg(&data)
        .arg(&audit)
        .output()
        .expect("run verifier");
    assert_success(verified);

    let tampered = run_dir.join("tampered.jsonl");
    let mut lines = fs::read_to_string(&audit)
        .expect("read audit")
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut record: Value = serde_json::from_str(&lines[1]).expect("audit record");
    record["response"]["ok"] = Value::Bool(false);
    lines[1] = serde_json::to_string(&record).expect("tamper record");
    fs::write(&tampered, lines.join("\n") + "\n").expect("write tampered audit");
    let rejected = Command::new(env!("CARGO_BIN_EXE_kitchen-verifier"))
        .arg(&data)
        .arg(&tampered)
        .output()
        .expect("verify tampered audit");
    assert!(!rejected.status.success());
    fs::remove_dir_all(run_dir).expect("remove test run directory");
}

#[test]
fn cooking_fire_replays_at_the_same_boundary_as_the_live_clock() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source_data = root.join("data/overcooked-1");
    let run_dir = unique_run_dir();
    fs::create_dir_all(&run_dir).expect("create test run directory");
    let data = run_dir.join("data");
    write_fast_fire_fixture(&source_data, &data);
    let audit = run_dir.join("audit.jsonl");
    let events = run_dir.join("events.jsonl");
    let recorder_inbox = run_dir.join("game-inbox.jsonl");
    fs::File::create(&recorder_inbox).expect("create recorder inbox");
    let port = unused_port();
    let address = format!("127.0.0.1:{port}");
    let mut server = spawn_server_for_level(&data, &audit, &events, &recorder_inbox, &address, 2);
    wait_until_healthy(port, &mut server);
    assert_ok(request(port, "POST", "/v1/start", None));

    go_near(port, "object-3445");
    assert_ok(request(
        port,
        "POST",
        "/v1/interact",
        Some(json!({"target": "object-3445"})),
    ));
    go_near(port, "object-4134");
    assert_ok(request(
        port,
        "POST",
        "/v1/interact",
        Some(json!({"target": "object-4134"})),
    ));
    let work = assert_ok(request(
        port,
        "POST",
        "/v1/work/start",
        Some(json!({"target": "object-4134"})),
    ));
    let elapsed_ms = work["data"]["state"]["shift"]["elapsed_ms"]
        .as_u64()
        .expect("elapsed time");
    let done_ms = work["data"]["expected_done_ms"]
        .as_u64()
        .expect("chop completion");
    thread::sleep(Duration::from_millis(
        done_ms.saturating_sub(elapsed_ms).saturating_add(50),
    ));
    assert_ok(request(
        port,
        "POST",
        "/v1/interact",
        Some(json!({"target": "object-4134"})),
    ));
    go_near(port, "object-3526");
    assert_ok(request(
        port,
        "POST",
        "/v1/interact",
        Some(json!({"target": "object-3526"})),
    ));

    let cooking = assert_ok(request(port, "GET", "/v1/show", None))["data"]["map"]["objects"]
        .as_array()
        .expect("objects")
        .iter()
        .find(|object| object["id"] == "object-3526")
        .and_then(|object| object["item"]["cooking"].as_object())
        .expect("cooking pot")
        .clone();
    let duration_ms = cooking["duration_ms"].as_u64().expect("cooking duration");
    let progress_ms = cooking["progress_ms"].as_u64().expect("cooking progress");
    thread::sleep(Duration::from_millis(
        duration_ms
            .saturating_mul(2)
            .saturating_add(1)
            .saturating_sub(progress_ms)
            .saturating_add(1_100),
    ));
    let state = assert_ok(request(port, "GET", "/v1/show", None))["data"].clone();
    assert!(state["hazards"].as_array().is_some_and(|hazards| {
        hazards
            .iter()
            .any(|hazard| hazard["kind"] == "fire" && hazard["id"] == "object-3526")
    }));

    drop(server);
    let verified = Command::new(env!("CARGO_BIN_EXE_kitchen-verifier"))
        .arg(&data)
        .arg(&audit)
        .output()
        .expect("run verifier");
    assert_success(verified);
    fs::remove_dir_all(run_dir).expect("remove test run directory");
}

fn write_fast_fire_fixture(source: &Path, destination: &Path) {
    fs::create_dir_all(destination.join("levels")).expect("create fixture data directory");
    let mut campaign: Value =
        serde_json::from_slice(&fs::read(source.join("campaign.json")).expect("read campaign"))
            .expect("parse campaign");
    let onion_soup = campaign["orders"]
        .as_array_mut()
        .expect("orders")
        .iter_mut()
        .find(|order| order["id"] == "OnionSoup")
        .expect("onion soup");
    onion_soup["required"] = json!(["Onion"]);
    fs::write(
        destination.join("campaign.json"),
        serde_json::to_vec(&campaign).expect("serialize campaign"),
    )
    .expect("write campaign");

    let level_name = "4P_SoupKitchen.json";
    let mut level: Value = serde_json::from_slice(
        &fs::read(source.join("levels").join(level_name)).expect("read level"),
    )
    .expect("parse level");
    level["cooking_utensils"][0]["cooking_seconds"] = json!(0.5);
    fs::write(
        destination.join("levels").join(level_name),
        serde_json::to_vec(&level).expect("serialize level"),
    )
    .expect("write level");
}

fn state_after(response: Value) -> Value {
    response["data"]
        .get("state")
        .unwrap_or(&response["data"])
        .clone()
}

fn go_near(port: u16, target: &str) -> Value {
    let state = assert_ok(request(port, "GET", "/v1/show", None))["data"].clone();
    let destination = state["destinations"]
        .as_array()
        .expect("destinations")
        .iter()
        .find(|destination| destination["target"].as_str() == Some(target))
        .expect("target destination");
    if destination["travel_ms"].as_u64() == Some(0) {
        return state;
    }
    assert_ok(request(
        port,
        "POST",
        "/v1/go",
        Some(json!({"target": target})),
    ));
    state_after(assert_ok(request(
        port,
        "GET",
        "/v1/wake?wait_ms=60000",
        None,
    )))
}

fn spawn_server(
    data: &Path,
    audit: &Path,
    events: &Path,
    recorder_inbox: &Path,
    address: &str,
) -> ServerGuard {
    spawn_server_for_level(data, audit, events, recorder_inbox, address, 1)
}

fn spawn_server_for_level(
    data: &Path,
    audit: &Path,
    events: &Path,
    recorder_inbox: &Path,
    address: &str,
    level: u8,
) -> ServerGuard {
    ServerGuard(
        Command::new(env!("CARGO_BIN_EXE_kitchen-server"))
            .env("KITCHEN_DATA_ROOT", data)
            .env("KITCHEN_LEVEL", level.to_string())
            .env("KITCHEN_TIME_SCALE", "1")
            .env("KITCHEN_SEED", "7")
            .env("KITCHEN_AUDIT", audit)
            .env("KITCHEN_EVENTS", events)
            .env("BENCHMARK_OBSERVER_INBOX", recorder_inbox)
            .env("KITCHEN_LISTEN_ADDR", address)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start kitchen server"),
    )
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
    std::env::temp_dir().join(format!("kitchen-live-http-{}-{stamp}", std::process::id()))
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
            panic!("kitchen server stopped early: {stderr}");
        }
        if TcpStream::connect(("127.0.0.1", port)).is_ok()
            && request(port, "GET", "/health", None)["ok"] == true
        {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("kitchen server did not become healthy");
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
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to kitchen server");
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

fn assert_success_status(output: &Output) {
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

fn assert_success(output: Output) {
    assert!(
        output.status.success(),
        "verifier failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 verifier output");
    assert!(stdout.contains("verified:"), "{stdout}");
    assert!(stdout.contains("score:"), "{stdout}");
}
