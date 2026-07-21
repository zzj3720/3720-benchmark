use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sausage_terminal::{
    CAMPAIGN_ID, Campaign, CampaignEntries, Command, Session, SessionRecord, execute,
};
use serde::Deserialize;
use serde_json::{Value, json};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

const EVENT_SCHEMA: &str = "benchmark-observer-event-v1";
const BATCH_SCHEMA: &str = "benchmark-observer-batch-v1";
const AUDIT_SCHEMA: &str = "sausage-audit-v1";
const MAX_BODY_BYTES: usize = 65_536;

struct Inner {
    session: Session<'static>,
    events: Vec<Value>,
    sequence: u64,
}

struct App {
    inner: Mutex<Inner>,
    changed: Condvar,
    state_path: PathBuf,
    audit_path: PathBuf,
    event_path: PathBuf,
}

#[derive(Deserialize)]
struct MoveBody {
    directions: Vec<sausage_terminal::Direction>,
}

#[derive(Deserialize)]
struct UndoBody {
    #[serde(default = "default_undo_count")]
    count: usize,
}

fn default_undo_count() -> usize {
    1
}

fn main() {
    if let Err(error) = serve() {
        eprintln!("sausage-server: {error}");
        std::process::exit(2);
    }
}

fn serve() -> Result<(), String> {
    let root = sausage_terminal::data_root();
    let campaign_path = env_path(
        "SAUSAGE_CAMPAIGN",
        root.join("campaign").join("merged_binary.gz"),
    );
    let entries_path = env_path(
        "SAUSAGE_ENTRIES",
        root.join("campaign").join("entries.tar.gz"),
    );
    let state_path = env_path(
        "SAUSAGE_STATE",
        PathBuf::from("/var/lib/sausage/session.json"),
    );
    let audit_path = env_path(
        "SAUSAGE_AUDIT",
        PathBuf::from("/var/lib/sausage/audit.jsonl"),
    );
    let event_path = env_path(
        "SAUSAGE_EVENTS",
        PathBuf::from("/var/lib/sausage/events.jsonl"),
    );
    ensure_parent(&state_path)?;
    ensure_parent(&audit_path)?;
    ensure_parent(&event_path)?;

    let campaign = Box::leak(Box::new(Campaign::load_gzip(&campaign_path)?));
    let entries = Box::leak(Box::new(CampaignEntries::load(&entries_path)?));
    let session = if state_path.exists() {
        let bytes = fs::read(&state_path)
            .map_err(|error| format!("could not read {}: {error}", state_path.display()))?;
        let record: SessionRecord = serde_json::from_slice(&bytes)
            .map_err(|error| format!("invalid {}: {error}", state_path.display()))?;
        Session::restore(campaign, entries, record)?
    } else {
        Session::new(campaign, entries)?
    };
    initialize_audit(&audit_path)?;
    let events = load_events(&event_path)?;
    let sequence = events
        .last()
        .and_then(|event| event.get("sequence"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let app = Arc::new(App {
        inner: Mutex::new(Inner {
            session,
            events,
            sequence,
        }),
        changed: Condvar::new(),
        state_path,
        audit_path,
        event_path,
    });
    record_lifecycle(&app, "sidecar_started")?;

    let address = env::var("SAUSAGE_LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:3720".to_owned());
    let server = Server::http(&address)
        .map_err(|error| format!("could not listen on {address}: {error}"))?;
    println!("sausage-server listening on {address}");
    for request in server.incoming_requests() {
        let app = Arc::clone(&app);
        thread::spawn(move || {
            if let Err(error) = handle(request, &app) {
                eprintln!("sausage-server: {error}");
            }
        });
    }
    Ok(())
}

fn handle(mut request: Request, app: &Arc<App>) -> Result<(), String> {
    let (path, query) = split_url(request.url());
    if request.method() == &Method::Options {
        return respond(request, StatusCode(204), Value::Null);
    }
    if request.method() == &Method::Get && path == "/v1/observe/snapshot" {
        let inner = app.inner.lock().map_err(|_| "state lock poisoned")?;
        let body = json!({
            "schema": "benchmark-observer-snapshot-v1",
            "task": task_identity(),
            "latest_sequence": inner.sequence,
            "state": inner.session.snapshot()?,
        });
        drop(inner);
        return respond(request, StatusCode(200), body);
    }
    if request.method() == &Method::Get && path == "/v1/observe/events" {
        let parameters = query_parameters(query);
        let after = parameter_u64(&parameters, "after", 0)?;
        let limit = parameter_u64(&parameters, "limit", 256)?.clamp(1, 1000) as usize;
        let wait_ms = parameter_u64(&parameters, "wait_ms", 0)?.min(30_000);
        let mut inner = app.inner.lock().map_err(|_| "state lock poisoned")?;
        if inner.sequence <= after && wait_ms > 0 {
            let (next, _) = app
                .changed
                .wait_timeout_while(inner, Duration::from_millis(wait_ms), |state| {
                    state.sequence <= after
                })
                .map_err(|_| "state lock poisoned")?;
            inner = next;
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
        return respond(request, StatusCode(200), body);
    }

    let command = match (request.method(), path) {
        (&Method::Get, "/v1/show" | "/v1/status") => Command::Show,
        (&Method::Get, "/v1/levels") => Command::Levels,
        (&Method::Get, "/v1/submit") => Command::Submit,
        (&Method::Post, "/v1/move") => {
            let body: MoveBody = read_json_body(&mut request)?;
            Command::Move {
                directions: body.directions,
            }
        }
        (&Method::Post, "/v1/undo") => {
            let body: UndoBody = read_json_body(&mut request)?;
            Command::Undo { count: body.count }
        }
        (&Method::Post, "/v1/restart") => {
            require_empty_body(&mut request)?;
            Command::Restart
        }
        _ => {
            return respond(
                request,
                StatusCode(404),
                json!({"ok": false, "error": {"code": "not_found", "message": "unknown endpoint"}}),
            );
        }
    };
    let mut inner = app.inner.lock().map_err(|_| "state lock poisoned")?;
    let response = execute(&mut inner.session, &command);
    if response["ok"] == true
        && matches!(
            command,
            Command::Move { .. } | Command::Undo { .. } | Command::Restart
        )
    {
        save_session(&app.state_path, &inner.session.record())?;
    }
    let audit_sequence = inner.sequence + 1;
    append_json_line(
        &app.audit_path,
        &json!({
            "sequence": audit_sequence,
            "command": command,
            "response": response,
        }),
    )?;
    inner.sequence = audit_sequence;
    let event = json!({
        "schema": EVENT_SCHEMA,
        "sequence": inner.sequence,
        "timestamp_ms": timestamp_ms()?,
        "task": task_identity(),
        "type": "command",
        "action": command,
        "state": response_state(&response),
        "result": {
            "ok": response["ok"],
            "command": response["command"],
        },
    });
    append_json_line(&app.event_path, &event)?;
    inner.events.push(event);
    let response_for_client = response.clone();
    drop(inner);
    app.changed.notify_all();
    respond(request, StatusCode(200), response_for_client)
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
        "state": inner.session.snapshot()?,
        "result": {"ok": true},
    });
    append_json_line(&app.event_path, &event)?;
    inner.events.push(event);
    drop(inner);
    app.changed.notify_all();
    Ok(())
}

fn response_state(response: &Value) -> Value {
    match response.get("command").and_then(Value::as_str) {
        Some("show") => response["data"].clone(),
        Some("levels") | Some("undo") | Some("restart") | Some("submit") => {
            response["data"]["state"].clone()
        }
        Some("move") => response["data"]["snapshot"].clone(),
        _ => Value::Null,
    }
}

fn task_identity() -> Value {
    json!({
        "id": "sausage-roll",
        "label": "Stephen's Sausage Roll",
        "kind": "game",
        "campaign": CAMPAIGN_ID,
    })
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

fn respond(request: Request, status: StatusCode, body: Value) -> Result<(), String> {
    let bytes = serde_json::to_vec(&body)
        .map_err(|error| format!("could not serialize response: {error}"))?;
    let response = Response::from_data(bytes)
        .with_status_code(status)
        .with_header(json_header())
        .with_header(cors_header());
    request
        .respond(response)
        .map_err(|error| format!("could not send response: {error}"))
}

fn json_header() -> Header {
    Header::from_bytes("Content-Type", "application/json; charset=utf-8")
        .expect("static header is valid")
}

fn cors_header() -> Header {
    Header::from_bytes("Access-Control-Allow-Origin", "*").expect("static header is valid")
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

fn save_session(path: &Path, record: &SessionRecord) -> Result<(), String> {
    let temporary = path.with_extension("tmp");
    let bytes = serde_json::to_vec(record)
        .map_err(|error| format!("could not serialize session: {error}"))?;
    fs::write(&temporary, bytes)
        .map_err(|error| format!("could not write {}: {error}", temporary.display()))?;
    fs::rename(&temporary, path)
        .map_err(|error| format!("could not replace {}: {error}", path.display()))
}

fn initialize_audit(path: &Path) -> Result<(), String> {
    if path.exists() && path.metadata().is_ok_and(|metadata| metadata.len() > 0) {
        return Ok(());
    }
    append_json_line(
        path,
        &json!({
            "schema": AUDIT_SCHEMA,
            "api_version": sausage_terminal::API_VERSION,
            "campaign": CAMPAIGN_ID,
        }),
    )
}

fn load_events(path: &Path) -> Result<Vec<Value>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    BufReader::new(
        File::open(path).map_err(|error| format!("could not open {}: {error}", path.display()))?,
    )
    .lines()
    .enumerate()
    .map(|(index, line)| {
        let line = line.map_err(|error| format!("could not read {}: {error}", path.display()))?;
        serde_json::from_str(&line)
            .map_err(|error| format!("invalid {} line {}: {error}", path.display(), index + 1))
    })
    .collect()
}

fn append_json_line(path: &Path, value: &Value) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("could not open {}: {error}", path.display()))?;
    serde_json::to_writer(&mut file, value)
        .map_err(|error| format!("could not append {}: {error}", path.display()))?;
    file.write_all(b"\n")
        .map_err(|error| format!("could not append {}: {error}", path.display()))
}

fn timestamp_ms() -> Result<u64, String> {
    let value = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system time precedes Unix epoch: {error}"))?
        .as_millis();
    u64::try_from(value).map_err(|_| "timestamp does not fit in u64".to_owned())
}
