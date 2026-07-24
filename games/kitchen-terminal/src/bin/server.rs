use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use kitchen_terminal::{
    API_VERSION, Command, Direction, GameData, Session, SessionConfig, execute,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

const AUDIT_SCHEMA: &str = "overcooked-audit-v1";
const EVENT_SCHEMA: &str = "benchmark-observer-event-v1";
const BATCH_SCHEMA: &str = "benchmark-observer-batch-v1";
const MAX_BODY_BYTES: usize = 32_768;
const MAX_WAKE_WAIT_MS: u64 = 30 * 60 * 1_000;

struct Inner {
    session: Session<'static>,
    started_at: Option<Instant>,
    sequence: u64,
    events: Vec<Value>,
    last_observer_elapsed_ms: Option<u64>,
}

struct App {
    inner: Mutex<Inner>,
    changed: Condvar,
    audit_path: PathBuf,
    event_path: PathBuf,
    recorder_inbox_path: PathBuf,
    content_hash: String,
    level: u8,
    scene: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectionBody {
    direction: Direction,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetBody {
    target: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdBody {
    id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AlarmBody {
    id: String,
    after_ms: u64,
    #[serde(default)]
    note: String,
}

fn main() {
    if let Err(error) = serve() {
        eprintln!("kitchen-server: {error}");
        std::process::exit(2);
    }
}

fn serve() -> Result<(), String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let data_root = env_path("KITCHEN_DATA_ROOT", root.join("data/overcooked-1"));
    let level = env_u64("KITCHEN_LEVEL", 1)?;
    let level =
        u8::try_from(level).map_err(|_| "KITCHEN_LEVEL must be between 1 and 30".to_owned())?;
    let config = SessionConfig {
        time_scale: u32::try_from(env_u64("KITCHEN_TIME_SCALE", 5)?)
            .map_err(|_| "KITCHEN_TIME_SCALE is too large".to_owned())?,
        seed: env_u64("KITCHEN_SEED", 448_510)?,
    };
    let audit_path = env_path(
        "KITCHEN_AUDIT",
        PathBuf::from("/var/lib/kitchen/audit.jsonl"),
    );
    let event_path = env_path(
        "KITCHEN_EVENTS",
        PathBuf::from("/var/lib/kitchen/events.jsonl"),
    );
    let recorder_inbox_path = env_path(
        "BENCHMARK_OBSERVER_INBOX",
        PathBuf::from("/logs/artifacts/observer/game-inbox.jsonl"),
    );
    ensure_parent(&audit_path)?;
    ensure_parent(&event_path)?;
    let data = Box::leak(Box::new(GameData::load(&data_root, level)?));
    let content_hash = content_hash(&data_root, data, config)?;
    initialize_files(&audit_path, &event_path, data, config, &content_hash)?;
    let app = Arc::new(App {
        inner: Mutex::new(Inner {
            session: Session::new(data, config)?,
            started_at: None,
            sequence: 0,
            events: Vec::new(),
            last_observer_elapsed_ms: None,
        }),
        changed: Condvar::new(),
        audit_path,
        event_path,
        recorder_inbox_path,
        content_hash,
        level,
        scene: data.layout.scene.clone(),
    });
    record_lifecycle(&app, "sidecar_started")?;
    start_clock_publisher(Arc::clone(&app));

    let address = env::var("KITCHEN_LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:3720".to_owned());
    let server = Server::http(&address)
        .map_err(|error| format!("could not listen on {address}: {error}"))?;
    println!("kitchen-server listening on {address}");
    for request in server.incoming_requests() {
        let app = Arc::clone(&app);
        thread::spawn(move || {
            if let Err(error) = handle(request, &app) {
                eprintln!("kitchen-server: {error}");
            }
        });
    }
    Ok(())
}

fn handle(mut request: Request, app: &Arc<App>) -> Result<(), String> {
    let url = request.url().to_owned();
    let (path, query) = split_url(&url);
    if request.method() == &Method::Options {
        return respond(request, StatusCode(204), Value::Null);
    }
    if request.method() == &Method::Get && path == "/health" {
        return respond(request, StatusCode(200), json!({"ok": true}));
    }
    if request.method() == &Method::Get && path == "/v1/observe/snapshot" {
        let mut inner = app.inner.lock().map_err(|_| "state lock poisoned")?;
        sync_clock(&mut inner)?;
        let body = json!({
            "schema": "benchmark-observer-snapshot-v1",
            "task": task_identity(app),
            "latest_sequence": inner.sequence,
            "state": inner.session.snapshot(),
        });
        drop(inner);
        return respond(request, StatusCode(200), body);
    }
    if request.method() == &Method::Get && path == "/v1/observe/events" {
        return observe_events(request, app, query);
    }

    let routed = (|| -> Result<Option<Command>, String> {
        let command = match (request.method(), path) {
            (&Method::Get, "/v1/show" | "/v1/status") => Command::Show,
            (&Method::Post, "/v1/start") => {
                require_empty_body(&mut request)?;
                Command::Start
            }
            (&Method::Post, "/v1/move") => {
                let body: DirectionBody = read_json_body(&mut request)?;
                Command::Move {
                    direction: body.direction,
                }
            }
            (&Method::Post, "/v1/dash") => {
                let body: DirectionBody = read_json_body(&mut request)?;
                Command::Dash {
                    direction: body.direction,
                }
            }
            (&Method::Post, "/v1/switch") => {
                require_empty_body(&mut request)?;
                Command::Switch
            }
            (&Method::Post, "/v1/interact") => {
                let body: TargetBody = read_json_body(&mut request)?;
                Command::Interact {
                    target: body.target,
                }
            }
            (&Method::Post, "/v1/work/start") => {
                let body: TargetBody = read_json_body(&mut request)?;
                Command::StartWork {
                    target: body.target,
                }
            }
            (&Method::Post, "/v1/work/stop") => {
                require_empty_body(&mut request)?;
                Command::StopWork
            }
            (&Method::Post, "/v1/alarm") => {
                let body: AlarmBody = read_json_body(&mut request)?;
                Command::SetAlarm {
                    id: body.id,
                    after_ms: body.after_ms,
                    note: body.note,
                }
            }
            (&Method::Post, "/v1/alarm/cancel") => {
                let body: IdBody = read_json_body(&mut request)?;
                Command::CancelAlarm { id: body.id }
            }
            (&Method::Get, "/v1/wake") => {
                let parameters = query_parameters(query);
                let wait_ms =
                    parameter_u64(&parameters, "wait_ms", MAX_WAKE_WAIT_MS)?.min(MAX_WAKE_WAIT_MS);
                wait_for_alarm(app, wait_ms)?;
                Command::Wake
            }
            (&Method::Get, "/v1/submit") => Command::Submit,
            _ => return Ok(None),
        };
        Ok(Some(command))
    })();
    let command = match routed {
        Ok(Some(command)) => command,
        Ok(None) => {
            return respond(
                request,
                StatusCode(404),
                json!({"ok": false, "error": {"code": "not_found", "message": "unknown endpoint"}}),
            );
        }
        Err(message) => {
            return respond(
                request,
                StatusCode(400),
                json!({"ok": false, "error": {"code": "invalid_request", "message": message}}),
            );
        }
    };

    let mut inner = app.inner.lock().map_err(|_| "state lock poisoned")?;
    sync_clock(&mut inner)?;
    let elapsed_ms = inner.session.elapsed_ms();
    let starting = matches!(command, Command::Start) && !inner.session.started();
    let response = execute(&mut inner.session, &command);
    if starting && response["ok"] == true {
        inner.started_at = Some(Instant::now());
    }
    record_command(app, &mut inner, elapsed_ms, &command, &response)?;
    let response_for_client = response.clone();
    drop(inner);
    app.changed.notify_all();
    respond(request, StatusCode(200), response_for_client)
}

fn observe_events(request: Request, app: &Arc<App>, query: &str) -> Result<(), String> {
    let parameters = query_parameters(query);
    let after = parameter_u64(&parameters, "after", 0)?;
    let limit = parameter_u64(&parameters, "limit", 256)?.clamp(1, 1_000) as usize;
    let wait_ms = parameter_u64(&parameters, "wait_ms", 0)?.min(30_000);
    let mut inner = app.inner.lock().map_err(|_| "state lock poisoned")?;
    sync_clock(&mut inner)?;
    if inner.sequence <= after && wait_ms > 0 {
        let (next, _) = app
            .changed
            .wait_timeout_while(inner, Duration::from_millis(wait_ms), |state| {
                state.sequence <= after
            })
            .map_err(|_| "state lock poisoned")?;
        inner = next;
        sync_clock(&mut inner)?;
    }
    let events = inner
        .events
        .iter()
        .filter(|event| {
            event
                .get("sequence")
                .and_then(Value::as_u64)
                .is_some_and(|sequence| sequence > after)
        })
        .take(limit)
        .cloned()
        .collect::<Vec<_>>();
    let body = json!({
        "schema": BATCH_SCHEMA,
        "task": task_identity(app),
        "after": after,
        "latest_sequence": inner.sequence,
        "events": events,
    });
    drop(inner);
    respond(request, StatusCode(200), body)
}

fn wait_for_alarm(app: &Arc<App>, wait_ms: u64) -> Result<(), String> {
    if wait_ms == 0 {
        return Ok(());
    }
    let sleep_ms = {
        let mut inner = app.inner.lock().map_err(|_| "state lock poisoned")?;
        sync_clock(&mut inner)?;
        if !inner.session.started() {
            return Ok(());
        }
        let elapsed = inner.session.elapsed_ms();
        match inner.session.next_pending_alarm_ms() {
            Some(due) if due <= elapsed => 0,
            Some(due) => wait_ms.min(due - elapsed),
            None => 0,
        }
        .min(inner.session.duration_ms().saturating_sub(elapsed))
    };
    if sleep_ms > 0 {
        thread::sleep(Duration::from_millis(sleep_ms));
    }
    Ok(())
}

fn sync_clock(inner: &mut Inner) -> Result<(), String> {
    let Some(started_at) = inner.started_at else {
        return Ok(());
    };
    let elapsed_ms = u64::try_from(started_at.elapsed().as_millis())
        .unwrap_or(u64::MAX)
        .min(inner.session.duration_ms());
    inner.session.advance_to(elapsed_ms)
}

fn record_command(
    app: &App,
    inner: &mut Inner,
    elapsed_ms: u64,
    command: &Command,
    response: &Value,
) -> Result<(), String> {
    inner.sequence += 1;
    append_json_line(
        &app.audit_path,
        &json!({
            "sequence": inner.sequence,
            "elapsed_ms": elapsed_ms,
            "command": command,
            "response": response,
        }),
    )?;
    let score = inner.session.snapshot().campaign.score;
    let previous_score = last_observer_score(inner);
    let event = json!({
        "schema": EVENT_SCHEMA,
        "sequence": inner.sequence,
        "timestamp_ms": timestamp_ms()?,
        "task": task_identity(app),
        "type": if command.is_game_action() { "action" } else { "command" },
        "action": command,
        "state": response_state(response),
        "result": {
            "ok": response["ok"],
            "command": response["command"],
            "elapsed_ms": elapsed_ms,
            "score": score,
        },
        "score": score,
        "score_delta": score - previous_score,
    });
    append_observer_event(app, &event)?;
    inner.events.push(event);
    inner.last_observer_elapsed_ms = Some(elapsed_ms);
    Ok(())
}

fn record_lifecycle(app: &Arc<App>, event_type: &str) -> Result<(), String> {
    let mut inner = app.inner.lock().map_err(|_| "state lock poisoned")?;
    inner.sequence += 1;
    let event = json!({
        "schema": EVENT_SCHEMA,
        "sequence": inner.sequence,
        "timestamp_ms": timestamp_ms()?,
        "task": task_identity(app),
        "type": event_type,
        "action": Value::Null,
        "state": inner.session.snapshot(),
        "result": {"ok": true, "content_sha256": app.content_hash},
        "score": 0,
        "score_delta": 0,
    });
    append_observer_event(app, &event)?;
    inner.events.push(event);
    Ok(())
}

fn start_clock_publisher(app: Arc<App>) {
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_secs(1));
            if let Err(error) = publish_clock(&app) {
                eprintln!("kitchen-server clock publisher: {error}");
                return;
            }
        }
    });
}

fn publish_clock(app: &Arc<App>) -> Result<(), String> {
    let mut inner = app.inner.lock().map_err(|_| "state lock poisoned")?;
    if !inner.session.started() {
        return Ok(());
    }
    sync_clock(&mut inner)?;
    let elapsed_ms = inner.session.elapsed_ms();
    if inner.last_observer_elapsed_ms == Some(elapsed_ms) {
        return Ok(());
    }
    let score = inner.session.snapshot().campaign.score;
    let previous_score = last_observer_score(&inner);
    inner.sequence += 1;
    let event = json!({
        "schema": EVENT_SCHEMA,
        "sequence": inner.sequence,
        "timestamp_ms": timestamp_ms()?,
        "task": task_identity(app),
        "type": "clock",
        "action": Value::Null,
        "state": inner.session.snapshot(),
        "result": {"ok": true, "elapsed_ms": elapsed_ms, "score": score},
        "score": score,
        "score_delta": score - previous_score,
    });
    append_observer_event(app, &event)?;
    inner.events.push(event);
    inner.last_observer_elapsed_ms = Some(elapsed_ms);
    drop(inner);
    app.changed.notify_all();
    Ok(())
}

fn last_observer_score(inner: &Inner) -> i64 {
    inner
        .events
        .last()
        .and_then(|event| event.get("score"))
        .and_then(Value::as_i64)
        .unwrap_or(0)
}

fn initialize_files(
    audit_path: &Path,
    event_path: &Path,
    data: &GameData,
    config: SessionConfig,
    content_hash: &str,
) -> Result<(), String> {
    if audit_path.exists() || event_path.exists() {
        return Err(
            "kitchen audit/event files already exist; use a fresh run directory".to_owned(),
        );
    }
    append_json_line(
        audit_path,
        &json!({
            "schema": AUDIT_SCHEMA,
            "api_version": API_VERSION,
            "content_sha256": content_hash,
            "level": data.level.number,
            "scene": data.layout.scene,
            "time_scale": config.time_scale,
            "seed": config.seed,
        }),
    )?;
    File::create(event_path)
        .map_err(|error| format!("could not create {}: {error}", event_path.display()))?;
    Ok(())
}

fn content_hash(root: &Path, data: &GameData, config: SessionConfig) -> Result<String, String> {
    let mut digest = Sha256::new();
    for path in [
        root.join("campaign.json"),
        root.join("levels")
            .join(format!("{}.json", data.layout.scene)),
    ] {
        digest.update(
            fs::read(&path)
                .map_err(|error| format!("could not read {}: {error}", path.display()))?,
        );
    }
    digest.update(data.level.number.to_le_bytes());
    digest.update(config.time_scale.to_le_bytes());
    digest.update(config.seed.to_le_bytes());
    Ok(format!("{:x}", digest.finalize()))
}

fn response_state(response: &Value) -> Value {
    if response["ok"] == false {
        return response["state"].clone();
    }
    if response["data"]["schema"].as_str() == Some("overcooked-state-v1") {
        response["data"].clone()
    } else {
        response["data"]["state"].clone()
    }
}

fn task_identity(app: &App) -> Value {
    json!({
        "id": "overcooked",
        "label": format!("Overcooked 1 · Level {}", app.level),
        "kind": "game",
        "campaign": "overcooked-1-main",
        "level": app.level,
        "scene": app.scene,
    })
}

fn read_json_body<T: for<'de> Deserialize<'de>>(request: &mut Request) -> Result<T, String> {
    let mut body = Vec::new();
    request
        .as_reader()
        .take((MAX_BODY_BYTES + 1) as u64)
        .read_to_end(&mut body)
        .map_err(|error| format!("could not read request body: {error}"))?;
    if body.len() > MAX_BODY_BYTES {
        return Err("request body is too large".to_owned());
    }
    serde_json::from_slice(&body).map_err(|error| format!("invalid JSON request: {error}"))
}

fn require_empty_body(request: &mut Request) -> Result<(), String> {
    let mut byte = [0];
    if request
        .as_reader()
        .read(&mut byte)
        .map_err(|error| format!("could not read request body: {error}"))?
        != 0
    {
        return Err("endpoint requires an empty request body".to_owned());
    }
    Ok(())
}

fn split_url(url: &str) -> (&str, &str) {
    url.split_once('?').map_or((url, ""), |parts| parts)
}

fn query_parameters(query: &str) -> Vec<(&str, &str)> {
    query
        .split('&')
        .filter_map(|item| item.split_once('='))
        .collect()
}

fn parameter_u64(parameters: &[(&str, &str)], name: &str, default: u64) -> Result<u64, String> {
    let Some((_, value)) = parameters.iter().find(|(key, _)| *key == name) else {
        return Ok(default);
    };
    value
        .parse()
        .map_err(|_| format!("query parameter {name} must be a non-negative integer"))
}

fn env_u64(name: &str, default: u64) -> Result<u64, String> {
    env::var(name).map_or(Ok(default), |value| {
        value
            .parse()
            .map_err(|_| format!("{name} must be a non-negative integer"))
    })
}

fn env_path(name: &str, default: PathBuf) -> PathBuf {
    env::var_os(name).map(PathBuf::from).unwrap_or(default)
}

fn ensure_parent(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    }
    Ok(())
}

fn append_json_line(path: &Path, value: &Value) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("could not open {}: {error}", path.display()))?;
    serde_json::to_writer(&mut file, value)
        .map_err(|error| format!("could not serialize {}: {error}", path.display()))?;
    file.write_all(b"\n")
        .and_then(|_| file.flush())
        .map_err(|error| format!("could not write {}: {error}", path.display()))
}

fn append_observer_event(app: &App, value: &Value) -> Result<(), String> {
    let path = if app.recorder_inbox_path.exists() {
        &app.recorder_inbox_path
    } else {
        &app.event_path
    };
    append_json_line(path, value)
}

fn timestamp_ms() -> Result<u64, String> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock is before Unix epoch: {error}"))?
        .as_millis();
    u64::try_from(millis).map_err(|_| "timestamp does not fit in u64".to_owned())
}

fn respond(request: Request, status: StatusCode, body: Value) -> Result<(), String> {
    let bytes = serde_json::to_vec(&body)
        .map_err(|error| format!("could not serialize response: {error}"))?;
    request
        .respond(
            Response::from_data(bytes)
                .with_chunked_threshold(usize::MAX)
                .with_status_code(status)
                .with_header(
                    Header::from_bytes("Content-Type", "application/json; charset=utf-8")
                        .expect("static header"),
                )
                .with_header(
                    Header::from_bytes("Access-Control-Allow-Origin", "*").expect("static header"),
                )
                .with_header(
                    Header::from_bytes("Access-Control-Allow-Headers", "Content-Type")
                        .expect("static header"),
                ),
        )
        .map_err(|error| format!("could not send response: {error}"))
}
