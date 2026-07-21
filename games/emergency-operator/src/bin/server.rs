use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use operator_terminal::{API_VERSION, Campaign, Command, Session, execute};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

const AUDIT_SCHEMA: &str = "emergency-operator-audit-v1";
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
    campaign_hash: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdBody {
    id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AnswerBody {
    call: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SayBody {
    call: String,
    choice: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DispatchBody {
    unit: String,
    incident: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UnitBody {
    unit: String,
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
        eprintln!("operator-server: {error}");
        std::process::exit(2);
    }
}

fn serve() -> Result<(), String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let campaign_path = env_path("OPERATOR_CAMPAIGN", root.join("data/campaign/pilot.json"));
    let audit_path = env_path(
        "OPERATOR_AUDIT",
        PathBuf::from("/var/lib/operator/audit.jsonl"),
    );
    let event_path = env_path(
        "OPERATOR_EVENTS",
        PathBuf::from("/var/lib/operator/events.jsonl"),
    );
    ensure_parent(&audit_path)?;
    ensure_parent(&event_path)?;
    let campaign_hash = sha256_file(&campaign_path)?;
    let campaign = Box::leak(Box::new(Campaign::load(&campaign_path)?));
    initialize_files(&audit_path, &event_path, campaign, &campaign_hash)?;
    let app = Arc::new(App {
        inner: Mutex::new(Inner {
            session: Session::new(campaign),
            started_at: None,
            sequence: 0,
            events: Vec::new(),
            last_observer_elapsed_ms: None,
        }),
        changed: Condvar::new(),
        audit_path,
        event_path,
        campaign_hash,
    });
    record_lifecycle(&app, "sidecar_started")?;
    start_clock_publisher(Arc::clone(&app));

    let address = env::var("OPERATOR_LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:3720".to_owned());
    let server = Server::http(&address)
        .map_err(|error| format!("could not listen on {address}: {error}"))?;
    println!("operator-server listening on {address}");
    for request in server.incoming_requests() {
        let app = Arc::clone(&app);
        thread::spawn(move || {
            if let Err(error) = handle(request, &app) {
                eprintln!("operator-server: {error}");
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
            "task": task_identity(),
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
            (&Method::Post, "/v1/answer") => {
                let body: AnswerBody = read_json_body(&mut request)?;
                Command::Answer { call: body.call }
            }
            (&Method::Post, "/v1/say") => {
                let body: SayBody = read_json_body(&mut request)?;
                Command::Say {
                    call: body.call,
                    choice: body.choice,
                }
            }
            (&Method::Post, "/v1/dispatch") => {
                let body: DispatchBody = read_json_body(&mut request)?;
                Command::Dispatch {
                    unit: body.unit,
                    incident: body.incident,
                }
            }
            (&Method::Post, "/v1/recall") => {
                let body: UnitBody = read_json_body(&mut request)?;
                Command::Recall { unit: body.unit }
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
        "task": task_identity(),
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
        "task": task_identity(),
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
    append_json_line(&app.event_path, &event)?;
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
        "task": task_identity(),
        "type": event_type,
        "action": Value::Null,
        "state": inner.session.snapshot(),
        "result": {"ok": true, "campaign_sha256": app.campaign_hash},
        "score": 0,
        "score_delta": 0,
    });
    append_json_line(&app.event_path, &event)?;
    inner.events.push(event);
    Ok(())
}

fn start_clock_publisher(app: Arc<App>) {
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_secs(1));
            if let Err(error) = publish_clock(&app) {
                eprintln!("operator-server clock publisher: {error}");
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
        "task": task_identity(),
        "type": "clock",
        "action": Value::Null,
        "state": inner.session.snapshot(),
        "result": {
            "ok": true,
            "elapsed_ms": elapsed_ms,
            "score": score,
        },
        "score": score,
        "score_delta": score - previous_score,
    });
    append_json_line(&app.event_path, &event)?;
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
    campaign: &Campaign,
    campaign_hash: &str,
) -> Result<(), String> {
    if audit_path.exists() || event_path.exists() {
        return Err(
            "operator audit/event files already exist; use a fresh run directory".to_owned(),
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
    if response["data"]["schema"].as_str() == Some("emergency-operator-state-v1") {
        response["data"].clone()
    } else {
        response["data"]["state"].clone()
    }
}

fn task_identity() -> Value {
    json!({
        "id": "emergency-operator",
        "label": "Emergency Operator",
        "kind": "game",
        "campaign": "emergency-operator-pilot-v1",
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

#[allow(dead_code)]
fn read_events(path: &Path) -> Result<Vec<Value>, String> {
    let file =
        File::open(path).map_err(|error| format!("could not open {}: {error}", path.display()))?;
    BufReader::new(file)
        .lines()
        .map(|line| {
            let line = line.map_err(|error| format!("could not read event: {error}"))?;
            serde_json::from_str(&line).map_err(|error| format!("invalid event: {error}"))
        })
        .collect()
}
