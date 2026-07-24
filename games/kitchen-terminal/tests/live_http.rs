use std::collections::{HashMap, HashSet, VecDeque};
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
    let child = Command::new(env!("CARGO_BIN_EXE_kitchen-server"))
        .env("KITCHEN_DATA_ROOT", &data)
        .env("KITCHEN_LEVEL", "1")
        .env("KITCHEN_TIME_SCALE", "1")
        .env("KITCHEN_SEED", "7")
        .env("KITCHEN_AUDIT", &audit)
        .env("KITCHEN_EVENTS", &events)
        .env("BENCHMARK_OBSERVER_INBOX", &recorder_inbox)
        .env("KITCHEN_LISTEN_ADDR", &address)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start kitchen server");
    let mut server = ServerGuard(child);
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
        "/v1/move",
        Some(json!({"direction": "north", "at_ms": 1000})),
    );
    assert!(headers.starts_with("HTTP/1.1 400"));
    assert_eq!(rejection["error"]["code"], "invalid_request");

    let started = assert_ok(request(port, "POST", "/v1/start", None));
    let direction = available_direction(&started["data"]);
    assert_ok(request(
        port,
        "POST",
        "/v1/move",
        Some(json!({"direction": direction})),
    ));
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

    let mut state = wake["data"]["state"].clone();
    let plate = state["map"]["objects"]
        .as_array()
        .expect("objects")
        .iter()
        .find(|object| object["item"]["kind"] == "plate")
        .and_then(|object| object["id"].as_str())
        .expect("authored plate")
        .to_owned();
    for (index, ingredient) in ["object-1117", "object-681"].into_iter().enumerate() {
        walk_near(port, &state, ingredient);
        state = state_after(assert_ok(request(
            port,
            "POST",
            "/v1/interact",
            Some(json!({"target": ingredient})),
        )));
        walk_near(port, &state, "object-1178");
        assert_ok(request(
            port,
            "POST",
            "/v1/interact",
            Some(json!({"target": "object-1178"})),
        ));
        let work = assert_ok(request(
            port,
            "POST",
            "/v1/work/start",
            Some(json!({"target": "object-1178"})),
        ));
        assert!(work["data"]["expected_done_ms"].as_u64().is_some());
        let alarm = format!("chop-{index}");
        assert_ok(request(
            port,
            "POST",
            "/v1/alarm",
            Some(json!({"id": alarm, "after_ms": 15_000, "note": "finish chop"})),
        ));
        let wake = assert_ok(request(port, "GET", "/v1/wake?wait_ms=16000", None));
        assert_eq!(wake["data"]["alarms"][0]["id"], alarm);
        state = state_after(assert_ok(request(
            port,
            "POST",
            "/v1/interact",
            Some(json!({"target": "object-1178"})),
        )));
        walk_near(port, &state, &plate);
        state = state_after(assert_ok(request(
            port,
            "POST",
            "/v1/interact",
            Some(json!({"target": plate})),
        )));
    }
    state = state_after(assert_ok(request(
        port,
        "POST",
        "/v1/interact",
        Some(json!({"target": plate})),
    )));
    walk_near(port, &state, "object-733");
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
    let event_batch = request(port, "GET", "/v1/observe/events?after=0", None);
    assert_eq!(event_batch["schema"], "benchmark-observer-batch-v1");
    assert!(event_batch["events"].as_array().is_some_and(|events| {
        events
            .iter()
            .any(|event| event["action"]["command"] == "move")
            && events.iter().any(|event| event["type"] == "clock")
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

fn state_after(response: Value) -> Value {
    response["data"]
        .get("state")
        .unwrap_or(&response["data"])
        .clone()
}

fn walk_near(port: u16, state: &Value, target: &str) -> Value {
    let objects = state["map"]["objects"].as_array().expect("objects");
    let target = objects
        .iter()
        .find(|object| object["id"].as_str() == Some(target))
        .expect("target object");
    let target_world = world(target);
    let active = state["active_chef"].as_u64().expect("active chef");
    let chefs = state["chefs"].as_array().expect("chefs");
    let chef = chefs
        .iter()
        .find(|chef| chef["id"].as_u64() == Some(active))
        .expect("active chef state");
    let start = coordinate(chef);
    let blocked = chefs
        .iter()
        .find(|chef| chef["id"].as_u64() != Some(active))
        .map(coordinate)
        .expect("other chef");
    let cells = state["map"]["walkable"].as_array().expect("walkable");
    let world_by_cell = cells
        .iter()
        .map(|cell| (coordinate(cell), world(cell)))
        .collect::<HashMap<_, _>>();
    let mut queue = VecDeque::from([start.clone()]);
    let mut visited = HashSet::from([start.clone()]);
    let mut previous = HashMap::<String, (String, &'static str)>::new();
    let destination = loop {
        let current = queue.pop_front().expect("target is reachable");
        if distance(world_by_cell[&current], target_world) <= 2.05 {
            break current;
        }
        let (manager, x, y, z) = split_coordinate(&current);
        for (direction, dx, dz) in [
            ("north", 0, 1),
            ("south", 0, -1),
            ("east", 1, 0),
            ("west", -1, 0),
        ] {
            let next = format!("{manager}:{}:{y}:{}", x + dx, z + dz);
            if next != blocked && world_by_cell.contains_key(&next) && visited.insert(next.clone())
            {
                previous.insert(next.clone(), (current.clone(), direction));
                queue.push_back(next);
            }
        }
    };
    let mut path = Vec::new();
    let mut cursor = destination;
    while cursor != start {
        let (prior, direction) = previous[&cursor].clone();
        path.push(direction);
        cursor = prior;
    }
    let mut current = state.clone();
    for direction in path.into_iter().rev() {
        current = state_after(assert_ok(request(
            port,
            "POST",
            "/v1/move",
            Some(json!({"direction": direction})),
        )));
    }
    current
}

fn coordinate(value: &Value) -> String {
    format!(
        "{}:{}:{}:{}",
        value["grid_manager"].as_str().expect("grid manager"),
        value["position"]["x"].as_i64().expect("x"),
        value["position"]["y"].as_i64().expect("y"),
        value["position"]["z"].as_i64().expect("z"),
    )
}

fn split_coordinate(value: &str) -> (&str, i64, i64, i64) {
    let mut parts = value.split(':');
    (
        parts.next().expect("manager"),
        parts.next().expect("x").parse().expect("numeric x"),
        parts.next().expect("y").parse().expect("numeric y"),
        parts.next().expect("z").parse().expect("numeric z"),
    )
}

fn world(value: &Value) -> [f64; 3] {
    [
        value["world"]["x"].as_f64().expect("world x"),
        value["world"]["y"].as_f64().expect("world y"),
        value["world"]["z"].as_f64().expect("world z"),
    ]
}

fn distance(left: [f64; 3], right: [f64; 3]) -> f64 {
    left.into_iter()
        .zip(right)
        .map(|(left, right)| (left - right).powi(2))
        .sum::<f64>()
        .sqrt()
}

fn available_direction(state: &Value) -> &'static str {
    let active = state["active_chef"].as_u64().expect("active chef");
    let chef = state["chefs"]
        .as_array()
        .expect("chefs")
        .iter()
        .find(|chef| chef["id"].as_u64() == Some(active))
        .expect("active chef state");
    let manager = chef["grid_manager"].as_str().expect("chef manager");
    let x = chef["position"]["x"].as_i64().expect("chef x");
    let y = chef["position"]["y"].as_i64().expect("chef y");
    let z = chef["position"]["z"].as_i64().expect("chef z");
    for (direction, dx, dz) in [
        ("north", 0, 1),
        ("south", 0, -1),
        ("east", 1, 0),
        ("west", -1, 0),
    ] {
        if state["map"]["walkable"]
            .as_array()
            .expect("walkable")
            .iter()
            .any(|cell| {
                cell["grid_manager"].as_str() == Some(manager)
                    && cell["position"]["x"].as_i64() == Some(x + dx)
                    && cell["position"]["y"].as_i64() == Some(y)
                    && cell["position"]["z"].as_i64() == Some(z + dz)
            })
        {
            return direction;
        }
    }
    panic!("chef has no direct movement neighbor")
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
