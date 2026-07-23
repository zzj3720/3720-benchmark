use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use flate2::Compression;
use flate2::write::GzEncoder;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sokoban_benchmark::{API_VERSION, Campaign, Command, Direction, Session, execute_observed};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

const AUDIT_SCHEMA: &str = "sokoban-audit-v1";
const EVENT_SCHEMA: &str = "benchmark-observer-event-v1";
const BATCH_SCHEMA: &str = "benchmark-observer-batch-v1";
const MAX_BODY_BYTES: usize = 32_768;

struct Inner {
    session: Session<'static>,
    audit_sequence: u64,
    event_sequence: u64,
    events: Vec<Value>,
}

struct App {
    inner: Mutex<Inner>,
    changed: Condvar,
    audit_path: PathBuf,
    event_path: PathBuf,
    recorder_inbox_path: PathBuf,
    campaign_hash: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LevelBody {
    level: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MoveBody {
    directions: Vec<Direction>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UndoBody {
    #[serde(default = "one")]
    steps: usize,
}

const fn one() -> usize {
    1
}

fn main() {
    if let Err(error) = serve() {
        eprintln!("sokoban-server: {error}");
        std::process::exit(2);
    }
}

fn serve() -> Result<(), String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let campaign_path = env_path("SOKOBAN_CAMPAIGN", root.join("data/campaign/sokoban.json"));
    let audit_path = env_path(
        "SOKOBAN_AUDIT",
        PathBuf::from("/var/lib/sokoban/audit.jsonl"),
    );
    let event_path = env_path(
        "SOKOBAN_EVENTS",
        PathBuf::from("/var/lib/sokoban/events.jsonl"),
    );
    let recorder_inbox_path = env_path(
        "BENCHMARK_OBSERVER_INBOX",
        PathBuf::from("/logs/artifacts/observer/game-inbox.jsonl"),
    );
    ensure_parent(&audit_path)?;
    ensure_parent(&event_path)?;
    let campaign_hash = sha256_file(&campaign_path)?;
    let campaign = Box::leak(Box::new(Campaign::load(&campaign_path)?));
    initialize_files(&audit_path, &event_path, campaign, &campaign_hash)?;
    let app = Arc::new(App {
        inner: Mutex::new(Inner {
            session: Session::new(campaign),
            audit_sequence: 0,
            event_sequence: 0,
            events: Vec::new(),
        }),
        changed: Condvar::new(),
        audit_path,
        event_path,
        recorder_inbox_path,
        campaign_hash,
    });
    record_lifecycle(&app)?;

    let address = env::var("SOKOBAN_LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:3720".to_owned());
    let server = Server::http(&address)
        .map_err(|error| format!("could not listen on {address}: {error}"))?;
    println!("sokoban-server listening on {address}");
    for request in server.incoming_requests() {
        let app = Arc::clone(&app);
        thread::spawn(move || {
            if let Err(error) = handle(request, &app) {
                eprintln!("sokoban-server: {error}");
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
        let inner = app.inner.lock().map_err(|_| "state lock poisoned")?;
        let body = json!({
            "schema": "benchmark-observer-snapshot-v1",
            "task": task_identity(),
            "latest_sequence": inner.event_sequence,
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
            (&Method::Get, "/v1/levels") => {
                let tier = query_parameters(query)
                    .into_iter()
                    .find(|(key, _)| *key == "tier")
                    .map(|(_, value)| value.to_owned());
                Command::Levels { tier }
            }
            (&Method::Post, "/v1/select") => {
                let body: LevelBody = read_json_body(&mut request)?;
                Command::Select { level: body.level }
            }
            (&Method::Post, "/v1/move") => {
                let body: MoveBody = read_json_body(&mut request)?;
                Command::Move {
                    directions: body.directions,
                }
            }
            (&Method::Post, "/v1/undo") => {
                let body: UndoBody = read_json_body(&mut request)?;
                Command::Undo { steps: body.steps }
            }
            (&Method::Post, "/v1/reset") => {
                require_empty_body(&mut request)?;
                Command::Reset
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
    let previous_score = inner.session.score();
    let (response, observer_steps) = execute_observed(&mut inner.session, &command);
    record_command(
        app,
        &mut inner,
        previous_score,
        &command,
        &response,
        &observer_steps,
    )?;
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
    if inner.event_sequence <= after && wait_ms > 0 {
        let (next, _) = app
            .changed
            .wait_timeout_while(inner, Duration::from_millis(wait_ms), |state| {
                state.event_sequence <= after
            })
            .map_err(|_| "state lock poisoned")?;
        inner = next;
    }
    let events = inner
        .events
        .iter()
        .filter(|event| {
            event["sequence"]
                .as_u64()
                .is_some_and(|value| value > after)
        })
        .take(limit)
        .cloned()
        .collect::<Vec<_>>();
    let body = json!({
        "schema": BATCH_SCHEMA,
        "task": task_identity(),
        "after": after,
        "latest_sequence": inner.event_sequence,
        "events": events,
    });
    drop(inner);
    respond(request, StatusCode(200), body)
}

fn record_command(
    app: &App,
    inner: &mut Inner,
    previous_score: usize,
    command: &Command,
    response: &Value,
    observer_steps: &[Value],
) -> Result<(), String> {
    inner.audit_sequence += 1;
    append_json_line(
        &app.audit_path,
        &json!({
            "sequence": inner.audit_sequence,
            "command": command,
            "response": response,
        }),
    )?;
    inner.event_sequence += 1;
    let score = inner.session.score();
    let mut event = json!({
        "schema": EVENT_SCHEMA,
        "sequence": inner.event_sequence,
        "timestamp_ms": timestamp_ms()?,
        "task": task_identity(),
        "type": if command.is_game_action() { "action" } else { "command" },
        "action": command,
        "state": response_state(response),
        "result": {
            "ok": response["ok"],
            "command": response["command"],
            "score": score,
        },
        "score": score,
        "score_delta": score as i64 - previous_score as i64,
    });
    if !observer_steps.is_empty() {
        event["instruction_index"] = instruction_index(observer_steps);
        event["instruction_trace"] = encoded_instruction_trace(observer_steps)?;
    }
    append_observer_event(app, &event)?;
    inner.events.push(event);
    Ok(())
}

fn instruction_index(steps: &[Value]) -> Value {
    Value::Array(
        steps
            .iter()
            .map(|step| {
                json!({
                    "index": step["index"],
                    "action": step["action"],
                    "result": step["result"],
                    "score": step["score"],
                    "score_delta": step["score_delta"],
                })
            })
            .collect(),
    )
}

fn encoded_instruction_trace(steps: &[Value]) -> Result<Value, String> {
    let bytes = serde_json::to_vec(steps)
        .map_err(|error| format!("could not serialize instruction trace: {error}"))?;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(&bytes)
        .map_err(|error| format!("could not compress instruction trace: {error}"))?;
    let compressed = encoder
        .finish()
        .map_err(|error| format!("could not finish instruction trace: {error}"))?;
    Ok(json!({
        "encoding": "gzip+base64",
        "count": steps.len(),
        "uncompressed_bytes": bytes.len(),
        "data": BASE64.encode(compressed),
    }))
}

fn record_lifecycle(app: &Arc<App>) -> Result<(), String> {
    let mut inner = app.inner.lock().map_err(|_| "state lock poisoned")?;
    inner.event_sequence += 1;
    let event = json!({
        "schema": EVENT_SCHEMA,
        "sequence": inner.event_sequence,
        "timestamp_ms": timestamp_ms()?,
        "task": task_identity(),
        "type": "sidecar_started",
        "action": Value::Null,
        "state": inner.session.snapshot(),
        "result": {"ok": true, "campaign_sha256": app.campaign_hash},
        "score": 0,
        "score_delta": 0,
    });
    append_observer_event(app, &event)?;
    inner.events.push(event);
    Ok(())
}

fn initialize_files(
    audit_path: &Path,
    event_path: &Path,
    campaign: &Campaign,
    campaign_hash: &str,
) -> Result<(), String> {
    if audit_path.exists() || event_path.exists() {
        return Err(
            "sokoban audit/event files already exist; use a fresh run directory".to_owned(),
        );
    }
    append_json_line(
        audit_path,
        &json!({
            "schema": AUDIT_SCHEMA,
            "api_version": API_VERSION,
            "campaign": campaign.id,
            "campaign_sha256": campaign_hash,
        }),
    )?;
    File::create(event_path)
        .map_err(|error| format!("could not create {}: {error}", event_path.display()))?;
    Ok(())
}

fn response_state(response: &Value) -> Value {
    if response["ok"] == false {
        return response["state"].clone();
    }
    if response["data"]["schema"].as_str() == Some("sokoban-state-v1") {
        response["data"].clone()
    } else {
        response["data"]["state"].clone()
    }
}

fn task_identity() -> Value {
    json!({
        "id": "sokoban",
        "label": "Sokoban Classics",
        "kind": "game",
        "campaign": "sokoban-classics-v1",
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

fn sha256_file(path: &Path) -> Result<String, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("could not read {}: {error}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
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
