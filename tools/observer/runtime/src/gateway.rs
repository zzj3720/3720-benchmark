use std::collections::{BTreeMap, HashMap};
use std::convert::Infallible;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::Json;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Router, body::Body};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use flate2::read::GzDecoder;
use futures_util::StreamExt;
use notify::{RecursiveMode, Watcher};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;

use crate::{canonical_json, replay_projection};

const EMPTY_EXPERIENCE: &str = r#"{"updated_at":null,"source_count":0,"counts":{"plan":0,"verified":0,"rejected":0,"solved":0},"plan":[],"verified":[],"rejected":[],"solved":[]}"#;

#[derive(Clone)]
struct App {
    root: PathBuf,
    archive: PathBuf,
    journals: PathBuf,
    revision: Arc<AtomicU64>,
    changes: broadcast::Sender<u64>,
    _watcher: Arc<Mutex<notify::RecommendedWatcher>>,
}

#[derive(Debug, Clone)]
struct Attempt {
    id: u64,
    start: usize,
    end: usize,
    reference: Option<String>,
    title: Option<String>,
    score: i64,
    successful: bool,
    kind: &'static str,
}

#[derive(Debug)]
struct NativeRun {
    id: String,
    summary: Value,
    detail: Value,
    replay_events: Vec<Value>,
    attempts: Vec<Attempt>,
    objects: PathBuf,
    activity: Vec<Value>,
}

#[derive(Deserialize)]
struct RunQuery {
    replay_attempt: Option<u64>,
}

#[derive(Deserialize)]
struct SubscribeQuery {
    run_id: Option<String>,
}

pub async fn serve(root: PathBuf, host: &str, port: u16) -> Result<(), String> {
    let root = root.canonicalize().map_err(display_error)?;
    let journals = root.join(".harbor/run-journals");
    let archive = root.join(".harbor/live-archive");
    let revision = Arc::new(AtomicU64::new(1));
    let (changes, _) = broadcast::channel(32);
    let notify_revision = Arc::clone(&revision);
    let notify_changes = changes.clone();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if event.is_ok() {
            let next = notify_revision.fetch_add(1, Ordering::Relaxed) + 1;
            let _ = notify_changes.send(next);
        }
    })
    .map_err(display_error)?;
    if journals.is_dir() {
        watcher
            .watch(&journals, RecursiveMode::Recursive)
            .map_err(display_error)?;
    }
    let app = App {
        root,
        archive,
        journals,
        revision,
        changes,
        _watcher: Arc::new(Mutex::new(watcher)),
    };
    let router = Router::new()
        .route("/health", get(health))
        .route("/v1/runs", get(runs))
        .route("/v1/runs/{run_id}", get(run))
        .route("/v1/assets/{asset_id}", get(asset))
        .route("/v1/subscribe", get(subscribe))
        .with_state(app);
    let listener = TcpListener::bind((host, port))
        .await
        .map_err(display_error)?;
    println!("3720 Rust live gateway listening on http://{host}:{port}");
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(display_error)
}

async fn health() -> impl IntoResponse {
    Json(json!({"ok": true, "runtime": "rust"}))
}

async fn runs(State(app): State<App>) -> Response {
    match list_runs(&app) {
        Ok(runs) => json_response(
            StatusCode::OK,
            json!({
                "schema": "benchmark-live-runs-v1",
                "generated_at": now_ms(),
                "runs": runs,
            }),
        ),
        Err(error) => error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

async fn run(
    State(app): State<App>,
    AxumPath(run_id): AxumPath<String>,
    Query(query): Query<RunQuery>,
) -> Response {
    if !safe_id(&run_id) {
        return error_response(StatusCode::BAD_REQUEST, "invalid run id");
    }
    if let Some(attempt_id) = query.replay_attempt {
        return match native_run(&app, &run_id) {
            Ok(Some(run)) => match native_replay(&run, attempt_id) {
                Ok(Some(value)) => json_response(StatusCode::OK, value),
                Ok(None) => error_response(StatusCode::NOT_FOUND, "unknown replay attempt"),
                Err(error) => error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
            },
            Ok(None) => archive_response(
                &app.archive
                    .join("replays")
                    .join(&run_id)
                    .join(format!("{attempt_id}.json.gz")),
                "unknown replay attempt",
            ),
            Err(error) => error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
        };
    }
    match native_run(&app, &run_id) {
        Ok(Some(run)) => json_response(
            StatusCode::OK,
            json!({"schema": "benchmark-live-run-v1", "run": run.detail}),
        ),
        Ok(None) => archive_response(
            &app.archive.join("runs").join(format!("{run_id}.json.gz")),
            "unknown run",
        ),
        Err(error) => error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

async fn asset(State(app): State<App>, AxumPath(asset_id): AxumPath<String>) -> Response {
    if asset_id.len() != 64 || !asset_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return error_response(StatusCode::BAD_REQUEST, "invalid asset id");
    }
    let archived = app
        .archive
        .join("assets")
        .join(format!("{asset_id}.json.gz"));
    if archived.is_file() {
        return archive_response(&archived, "unknown asset");
    }
    let Ok(chains) = fs::read_dir(&app.journals) else {
        return error_response(StatusCode::NOT_FOUND, "unknown asset");
    };
    for chain in chains.flatten() {
        let object = chain
            .path()
            .join("objects")
            .join(format!("{asset_id}.json.gz"));
        if object.is_file() {
            return decoded_file_response(&object, "public, max-age=31536000, immutable");
        }
    }
    error_response(StatusCode::NOT_FOUND, "unknown asset")
}

async fn subscribe(
    State(app): State<App>,
    Query(query): Query<SubscribeQuery>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let initial = app.revision.load(Ordering::Relaxed);
    let receiver = app.changes.subscribe();
    let stream_app = app.clone();
    let run_id = query.run_id.clone();
    let initial_stream = tokio_stream::once(initial);
    let changes = BroadcastStream::new(receiver).filter_map(|value| async move { value.ok() });
    let stream = initial_stream.chain(changes).map(move |revision| {
        let value = subscription_value(&stream_app, run_id.as_deref(), revision)
            .unwrap_or_else(|error| json!({"error": error, "revision": revision}));
        Ok(Event::default().json_data(value).expect("JSON SSE"))
    });
    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keepalive"),
    )
}

fn subscription_value(app: &App, run_id: Option<&str>, revision: u64) -> Result<Value, String> {
    let mut runs = list_runs(app)?;
    if run_id.is_some() {
        for run in &mut runs {
            run.as_object_mut()
                .expect("run object")
                .remove("score_history");
        }
    }
    let selected = run_id.and_then(|id| {
        runs.iter()
            .find(|run| run.get("id").and_then(Value::as_str) == Some(id))
            .map(|run| {
                json!({
                    "id": id,
                    "latest_sequence": run.get("latest_sequence").cloned().unwrap_or(Value::from(0)),
                    "status": run.get("status").cloned().unwrap_or(Value::Null),
                })
            })
    });
    Ok(json!({
        "schema": "benchmark-live-subscription-v1",
        "generated_at": now_ms(),
        "revision": revision,
        "runs": runs,
        "selected": selected,
    }))
}

fn list_runs(app: &App) -> Result<Vec<Value>, String> {
    let mut by_id = BTreeMap::<String, Value>::new();
    let archived = app.archive.join("index.json.gz");
    if archived.is_file() {
        let value = read_gzip_json(&archived)?;
        if let Some(runs) = value.get("runs").and_then(Value::as_array) {
            for run in runs {
                if let Some(id) = run.get("id").and_then(Value::as_str) {
                    by_id.insert(id.to_owned(), run.clone());
                }
            }
        }
    }
    if let Ok(entries) = fs::read_dir(&app.journals) {
        for entry in entries.flatten() {
            let Some(id) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if let Some(run) = native_run(app, &id)? {
                by_id.insert(id, run.summary);
            }
        }
    }
    let mut runs = by_id.into_values().collect::<Vec<_>>();
    runs.sort_by(|left, right| {
        let left_key = (
            text(left, "game"),
            !boolean(left, "live"),
            -number(left, "score"),
            text(left, "model"),
        );
        let right_key = (
            text(right, "game"),
            !boolean(right, "live"),
            -number(right, "score"),
            text(right, "model"),
        );
        left_key.cmp(&right_key)
    });
    Ok(runs)
}

fn native_run(app: &App, run_id: &str) -> Result<Option<NativeRun>, String> {
    let chain = app.journals.join(run_id);
    let journal = chain.join("journal.jsonl");
    if !journal.is_file() {
        return Ok(None);
    }
    let mut rows = Vec::new();
    for line in BufReader::new(File::open(&journal).map_err(display_error)?)
        .lines()
        .map_while(Result::ok)
    {
        if let Ok(row) = serde_json::from_str::<Value>(&line) {
            rows.push(row);
        }
    }
    let registration = rows
        .iter()
        .rev()
        .find(|row| row.get("type").and_then(Value::as_str) == Some("segment_registered"))
        .and_then(|row| row.get("payload"))
        .and_then(Value::as_object);
    let Some(registration) = registration else {
        return Ok(None);
    };
    let model = registration
        .get("model")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    if model.is_none() {
        return Ok(None);
    }
    let task = registration
        .get("task")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_owned();
    if !matches!(
        task.as_str(),
        "parabox-intro" | "swarm-farming" | "sausage-roll" | "emergency-operator" | "sokoban"
    ) {
        return Ok(None);
    }
    let objects = chain.join("objects");
    let mut events = Vec::new();
    let mut activity = Vec::new();
    let mut experience_markdown = None;
    let mut experience_updated_at = None;
    for row in &rows {
        match row.get("source").and_then(Value::as_str) {
            Some("game") => events.push(materialize_game_event(row, &objects)?),
            Some("agent") if row.get("type").and_then(Value::as_str) == Some("agent_message") => {
                if let Some(text) = row.pointer("/payload/text").and_then(Value::as_str) {
                    activity.push(json!({
                        "timestamp_ms": row.get("source_timestamp_ms").cloned().unwrap_or(Value::Null),
                        "text": text,
                    }));
                }
            }
            Some("agent")
                if row.get("type").and_then(Value::as_str) == Some("experience_updated") =>
            {
                experience_markdown = row
                    .pointer("/payload/markdown")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                experience_updated_at = row.get("source_timestamp_ms").cloned();
            }
            _ => {}
        }
    }
    let state = events
        .iter()
        .rev()
        .find_map(event_state)
        .unwrap_or_else(|| json!({}));
    let (score, total, objective) = score(&task, &state, &events, &app.root);
    let live = rows
        .iter()
        .rposition(|row| row.get("type").and_then(Value::as_str) == Some("segment_registered"))
        .is_some_and(|registered| {
            !rows[registered + 1..]
                .iter()
                .any(|row| row.get("type").and_then(Value::as_str) == Some("segment_finished"))
        });
    let disposition = rows
        .iter()
        .rev()
        .find(|row| row.get("type").and_then(Value::as_str) == Some("segment_finished"))
        .and_then(|row| row.pointer("/payload/disposition"))
        .and_then(Value::as_str);
    let termination = if live {
        json!({"kind": "live", "resumable": false})
    } else if score >= total && total > 0 {
        json!({"kind": "completed", "resumable": false})
    } else {
        match disposition {
            Some("agent_stopped") => json!({"kind": "agent_stopped", "resumable": false}),
            Some("error") => {
                json!({"kind": "resumable", "resumable": true, "reason": "infrastructure error"})
            }
            Some("cancelled") => {
                json!({"kind": "stopped", "resumable": true, "reason": "cancelled"})
            }
            _ => json!({"kind": "resumable", "resumable": true}),
        }
    };
    let started_at = rows
        .first()
        .and_then(|row| row.get("source_timestamp_ms"))
        .cloned()
        .unwrap_or(Value::Null);
    let finished_at = rows
        .iter()
        .rev()
        .find(|row| row.get("type").and_then(Value::as_str) == Some("segment_finished"))
        .and_then(|row| row.get("source_timestamp_ms"))
        .cloned()
        .unwrap_or(Value::Null);
    let consumed_ms = rows
        .last()
        .and_then(|row| row.get("effective_elapsed_ms"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let latest_sequence = rows
        .last()
        .and_then(|row| row.get("sequence"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let mut history = Vec::new();
    let mut previous = None;
    for event in &events {
        let value = event.get("score").and_then(Value::as_i64).unwrap_or(0);
        if previous != Some(value) {
            history.push(json!({
                "timestamp_ms": event.get("timestamp_ms").cloned().unwrap_or(Value::Null),
                "elapsed_ms": event.get("effective_elapsed_ms").cloned().unwrap_or(Value::from(0)),
                "score": value,
            }));
            previous = Some(value);
        }
    }
    let last_score = history
        .iter()
        .rev()
        .find(|point| point.get("score").and_then(Value::as_i64).unwrap_or(0) > 0);
    let (game, task_name) = task_labels(&task);
    let agent = registration
        .get("agent")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let effort = registration
        .get("effort")
        .and_then(Value::as_str)
        .unwrap_or("default");
    let job = registration
        .get("job_name")
        .and_then(Value::as_str)
        .unwrap_or(run_id);
    let latest = events.last();
    let summary = json!({
        "id": run_id,
        "job": job,
        "trial": registration.get("trial").cloned().unwrap_or(Value::Null),
        "task_id": task,
        "task": task_name,
        "game": game,
        "model": model.unwrap(),
        "model_id": model.unwrap(),
        "agent": agent,
        "effort": effort,
        "live": live,
        "status": if live {"running"} else {"finished"},
        "termination": termination,
        "sidecar_only": false,
        "score": score,
        "total": total,
        "objective": objective,
        "started_at": started_at,
        "finished_at": finished_at,
        "last_activity_at": latest.and_then(|event| event.get("timestamp_ms")).cloned().unwrap_or(Value::Null),
        "last_score_at": last_score.and_then(|point| point.get("timestamp_ms")).cloned().unwrap_or(Value::Null),
        "last_score_elapsed_ms": last_score.and_then(|point| point.get("elapsed_ms")).cloned().unwrap_or(Value::Null),
        "consumed_ms": consumed_ms,
        "observed_at": now_ms(),
        "latest_sequence": latest_sequence,
        "latest_action": latest.and_then(|event| event.get("action")).cloned().unwrap_or(Value::Null),
        "latest_result": latest.and_then(|event| event.get("result")).cloned().unwrap_or(Value::Null),
        "score_history": history,
    });
    let replay_events = logical_replay_events(&events)?;
    let attempts = attempts(&task, &replay_events, &app.root);
    let groups = replay_groups(&attempts);
    let asset_refs = asset_references(&events);
    let experience = experience_markdown
        .as_deref()
        .map(|markdown| experience(markdown, experience_updated_at))
        .unwrap_or_else(|| serde_json::from_str(EMPTY_EXPERIENCE).expect("experience"));
    let mut detail = summary.clone();
    let detail_object = detail.as_object_mut().expect("summary object");
    detail_object.insert("state".into(), state);
    detail_object.insert("asset_refs".into(), asset_refs);
    detail_object.insert("replay_groups".into(), groups);
    detail_object.insert("agent_experience".into(), experience);
    Ok(Some(NativeRun {
        id: run_id.to_owned(),
        summary,
        detail,
        replay_events,
        attempts,
        objects,
        activity,
    }))
}

fn asset_references(events: &[Value]) -> Value {
    let mut references = Map::new();
    for event in events {
        let Some(assets) = event.get("assets").and_then(Value::as_object) else {
            continue;
        };
        for (name, descriptor) in assets {
            let Some(object) = descriptor.get("object").and_then(Value::as_str) else {
                continue;
            };
            references.insert(
                name.clone(),
                json!({
                    "id": object,
                    "media_type": "application/json",
                    "bytes": descriptor
                        .get("uncompressed_bytes")
                        .cloned()
                        .unwrap_or(Value::from(0)),
                }),
            );
        }
    }
    Value::Object(references)
}

fn native_replay(run: &NativeRun, attempt_id: u64) -> Result<Option<Value>, String> {
    let Some(attempt) = run.attempts.iter().find(|attempt| attempt.id == attempt_id) else {
        return Ok(None);
    };
    let mut events = run.replay_events.clone();
    for event in &mut events {
        materialize_descriptor(event, "instruction_trace", &run.objects)?;
        materialize_descriptor(event, "state_snapshot", &run.objects)?;
    }
    let projection = replay_projection(&events, attempt.start, attempt.end)?;
    let activity = run
        .activity
        .iter()
        .filter(|message| {
            let timestamp = message.get("timestamp_ms").and_then(Value::as_u64);
            let start = events
                .get(attempt.start)
                .and_then(|event| event.get("timestamp_ms"))
                .and_then(Value::as_u64);
            let end = events
                .get(attempt.end.saturating_sub(1))
                .and_then(|event| event.get("timestamp_ms"))
                .and_then(Value::as_u64);
            matches!((timestamp, start, end), (Some(value), Some(start), Some(end)) if value >= start && value <= end)
        })
        .cloned()
        .collect::<Vec<_>>();
    Ok(Some(json!({
        "schema": "benchmark-live-attempt-replay-v2",
        "run_id": run.id,
        "attempt_id": attempt.id,
        "kind": attempt.kind,
        "reference": attempt.reference,
        "title": attempt.title,
        "score": attempt.score,
        "successful": attempt.successful,
        "asset_refs": asset_references(&events),
        "activity": activity,
        "frames": projection.get("frames").cloned().unwrap_or_else(|| json!([])),
        "operations": projection.get("operations").cloned().unwrap_or_else(|| json!([])),
        "skipped_unchanged": projection.get("skipped_unchanged").cloned().unwrap_or(Value::from(0)),
        "eliminated_history_frames": projection.get("eliminated_history_frames").cloned().unwrap_or(Value::from(0)),
    })))
}

fn logical_replay_events(events: &[Value]) -> Result<Vec<Value>, String> {
    let mut output = Vec::<Value>::new();
    let mut redo = Vec::<Vec<Value>>::new();
    for event in events {
        let command = event
            .pointer("/action/command")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_ascii_lowercase();
        let target = event_state(event)
            .as_ref()
            .map(canonical_json)
            .transpose()?;
        match command.as_str() {
            "undo" => {
                let matching = target.as_ref().and_then(|target| {
                    output.iter().rposition(|candidate| {
                        event_state(candidate)
                            .as_ref()
                            .map(canonical_json)
                            .transpose()
                            .ok()
                            .flatten()
                            .as_ref()
                            == Some(target)
                    })
                });
                if let Some(matching) = matching {
                    let removed = output.split_off(matching + 1);
                    if !removed.is_empty() {
                        redo.push(removed);
                    }
                }
            }
            "redo" => {
                if let Some(restored) = redo.pop() {
                    let matching = target
                        .as_ref()
                        .and_then(|target| {
                            restored.iter().position(|candidate| {
                                event_state(candidate)
                                    .as_ref()
                                    .map(canonical_json)
                                    .transpose()
                                    .ok()
                                    .flatten()
                                    .as_ref()
                                    == Some(target)
                            })
                        })
                        .unwrap_or(restored.len().saturating_sub(1));
                    output.extend(restored.into_iter().take(matching + 1));
                }
            }
            _ => {
                redo.clear();
                output.push(event.clone());
            }
        }
    }
    Ok(output)
}

fn materialize_game_event(row: &Value, objects: &Path) -> Result<Value, String> {
    let mut event = row
        .get("payload")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    event.insert(
        "sequence".into(),
        row.get("sequence").cloned().unwrap_or(Value::Null),
    );
    event.insert(
        "timestamp_ms".into(),
        row.get("source_timestamp_ms")
            .cloned()
            .unwrap_or(Value::Null),
    );
    event.insert(
        "effective_elapsed_ms".into(),
        row.get("effective_elapsed_ms")
            .cloned()
            .unwrap_or(Value::from(0)),
    );
    let mut value = Value::Object(event);
    if let Some(snapshot) = object_json(value.get("state_snapshot"), objects)? {
        value["state"] = snapshot;
    }
    Ok(value)
}

fn materialize_descriptor(value: &mut Value, key: &str, objects: &Path) -> Result<(), String> {
    let Some(descriptor) = value.get(key).and_then(Value::as_object) else {
        return Ok(());
    };
    if descriptor.get("encoding").and_then(Value::as_str) != Some("gzip") {
        return Ok(());
    }
    let Some(id) = descriptor.get("object").and_then(Value::as_str) else {
        return Ok(());
    };
    let compressed = fs::read(objects.join(format!("{id}.json.gz"))).map_err(display_error)?;
    let mut replacement = descriptor.clone();
    replacement.insert("encoding".into(), Value::String("gzip+base64".into()));
    replacement.insert("data".into(), Value::String(BASE64.encode(compressed)));
    value[key] = Value::Object(replacement);
    Ok(())
}

fn object_json(descriptor: Option<&Value>, objects: &Path) -> Result<Option<Value>, String> {
    let Some(descriptor) = descriptor.and_then(Value::as_object) else {
        return Ok(None);
    };
    let Some(id) = descriptor.get("object").and_then(Value::as_str) else {
        return Ok(None);
    };
    let path = objects.join(format!("{id}.json.gz"));
    if !path.is_file() {
        return Ok(None);
    }
    read_gzip_json(&path).map(Some)
}

fn event_state(event: &Value) -> Option<Value> {
    event
        .get("state")
        .filter(|value| value.as_object().is_some_and(|object| !object.is_empty()))
        .cloned()
}

fn attempts(task: &str, events: &[Value], root: &Path) -> Vec<Attempt> {
    let mut output = Vec::new();
    let mut start = 0;
    let mut boundary_reference = None;
    let mut boundary_title = None;
    for (index, event) in events.iter().enumerate() {
        let command = event
            .pointer("/action/command")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_ascii_lowercase();
        let (reference, title, kind) = event_context(task, event, root);
        if command == "select" {
            boundary_reference = reference.clone();
            boundary_title = title.clone();
        }
        if command == "restart" || command == "reset" {
            finish_attempt(
                &mut output,
                events,
                start,
                index,
                boundary_reference.clone(),
                boundary_title.clone(),
                false,
                0,
                kind,
            );
            start = index + 1;
            boundary_reference = reference;
            boundary_title = title;
            continue;
        }
        let delta = event
            .get("score_delta")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        if delta > 0 {
            finish_attempt(
                &mut output,
                events,
                start,
                index + 1,
                reference.or_else(|| boundary_reference.clone()),
                title.or_else(|| boundary_title.clone()),
                true,
                event.get("score").and_then(Value::as_i64).unwrap_or(delta),
                kind,
            );
            start = index + 1;
            boundary_reference = None;
            boundary_title = None;
        }
    }
    let (reference, title, kind) = events
        .get(start)
        .map(|event| event_context(task, event, root))
        .unwrap_or((None, None, "level"));
    finish_attempt(
        &mut output,
        events,
        start,
        events.len(),
        boundary_reference.or(reference),
        boundary_title.or(title),
        false,
        0,
        kind,
    );
    output
}

#[allow(clippy::too_many_arguments)]
fn finish_attempt(
    output: &mut Vec<Attempt>,
    events: &[Value],
    start: usize,
    end: usize,
    reference: Option<String>,
    title: Option<String>,
    successful: bool,
    score: i64,
    kind: &'static str,
) {
    let ignored = [
        "", "restart", "reset", "undo", "redo", "show", "inspect", "status", "list", "select",
        "levels", "submit",
    ];
    let first = (start..end).find(|index| {
        let command = events[*index]
            .pointer("/action/command")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_ascii_lowercase();
        !ignored.contains(&command.as_str())
    });
    let Some(first) = first else {
        return;
    };
    output.push(Attempt {
        id: events[first]
            .get("sequence")
            .and_then(Value::as_u64)
            .unwrap_or(first as u64 + 1),
        start,
        end,
        reference,
        title,
        score,
        successful,
        kind,
    });
}

fn replay_groups(attempts: &[Attempt]) -> Value {
    let mut order = Vec::<String>::new();
    let mut groups = HashMap::<String, Value>::new();
    for attempt in attempts
        .iter()
        .filter(|attempt| attempt.successful || attempt.kind == "overworld")
    {
        let reference = attempt
            .reference
            .clone()
            .unwrap_or_else(|| format!("{}:{}", attempt.kind, attempt.id));
        if !groups.contains_key(&reference) {
            order.push(reference.clone());
            groups.insert(
                reference.clone(),
                json!({
                    "kind": attempt.kind,
                    "reference": reference,
                    "title": attempt.title,
                    "score": attempt.score,
                    "attempts": [],
                }),
            );
        }
    }
    for attempt in attempts {
        let Some(reference) = attempt.reference.as_ref() else {
            continue;
        };
        if let Some(group) = groups.get_mut(reference) {
            group["attempts"]
                .as_array_mut()
                .expect("attempt list")
                .push(json!({
                    "id": attempt.id,
                    "successful": attempt.successful,
                    "score": if attempt.successful {Some(attempt.score)} else {None},
                }));
        }
    }
    Value::Array(
        order
            .into_iter()
            .filter_map(|reference| groups.remove(&reference))
            .collect(),
    )
}

fn event_context(
    task: &str,
    event: &Value,
    root: &Path,
) -> (Option<String>, Option<String>, &'static str) {
    let state = event.get("state").unwrap_or(&Value::Null);
    if task == "sausage-roll" && state.get("mode").and_then(Value::as_str) == Some("overworld") {
        let score = state
            .pointer("/campaign/score")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        return (
            Some(format!("overworld:{score}")),
            Some(
                state
                    .pointer("/overworld/title")
                    .and_then(Value::as_str)
                    .unwrap_or("Land's End")
                    .to_owned(),
            ),
            "overworld",
        );
    }
    let reference = state
        .pointer("/level/reference")
        .or_else(|| state.pointer("/level/id"))
        .or_else(|| event.get("selected"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let title = state
        .pointer("/level/title")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            reference
                .as_deref()
                .and_then(|reference| parabox_title(root, reference))
        });
    (reference, title, "level")
}

fn score(task: &str, state: &Value, events: &[Value], root: &Path) -> (i64, i64, String) {
    match task {
        "parabox-intro" => {
            let score = state
                .pointer("/campaign/score")
                .or_else(|| state.pointer("/campaign/solved"))
                .and_then(Value::as_i64)
                .or_else(|| {
                    events
                        .last()
                        .and_then(|event| event.get("score"))
                        .and_then(Value::as_i64)
                })
                .unwrap_or(0);
            let total = state
                .pointer("/campaign/total")
                .and_then(Value::as_i64)
                .unwrap_or(364);
            let reference = state
                .pointer("/level/reference")
                .or_else(|| state.pointer("/level/id"))
                .and_then(Value::as_str)
                .or_else(|| {
                    events
                        .last()
                        .and_then(|event| event.get("selected"))
                        .and_then(Value::as_str)
                })
                .unwrap_or_default();
            let title = state
                .pointer("/level/title")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| parabox_title(root, reference))
                .unwrap_or_else(|| "Waiting for state".into());
            (
                score,
                total,
                if reference.is_empty() {
                    title
                } else {
                    format!("{reference} / {title}")
                },
            )
        }
        "sausage-roll" => {
            let score = state
                .pointer("/campaign/score")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let total = state
                .pointer("/campaign/total")
                .and_then(Value::as_i64)
                .unwrap_or(86);
            let objective = if state.get("mode").and_then(Value::as_str) == Some("overworld") {
                "Land's End / 大地图".into()
            } else {
                let id = state
                    .pointer("/level/id")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let title = state
                    .pointer("/level/title")
                    .and_then(Value::as_str)
                    .unwrap_or("Waiting for state");
                if id.is_empty() {
                    title.into()
                } else {
                    format!("{id} / {title}")
                }
            };
            (score, total, objective)
        }
        "emergency-operator" => {
            let score = state
                .pointer("/campaign/score")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let total = state
                .pointer("/campaign/max_score")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let status = state
                .pointer("/shift/status")
                .and_then(Value::as_str)
                .unwrap_or("not_started");
            (
                score,
                total,
                if status == "not_started" {
                    "Start shift".into()
                } else {
                    "Monitor dispatch".into()
                },
            )
        }
        "sokoban" => {
            let score = state
                .pointer("/campaign/score")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let total = state
                .pointer("/campaign/max_score")
                .and_then(Value::as_i64)
                .unwrap_or(305);
            let id = state
                .pointer("/level/id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let title = state
                .pointer("/level/title")
                .and_then(Value::as_str)
                .unwrap_or("Select a level");
            (
                score,
                total,
                if id.is_empty() {
                    title.into()
                } else {
                    format!("{id} / {title}")
                },
            )
        }
        _ => {
            let score = state.get("score").and_then(Value::as_i64).unwrap_or(0);
            (score, 1_000_000, "Make curry".into())
        }
    }
}

fn parabox_title(root: &Path, reference: &str) -> Option<String> {
    let index = root.join("games/parabox-intro/data/campaign/index.tsv");
    fs::read_to_string(index).ok()?.lines().find_map(|line| {
        let (id, title) = line.split_once('\t')?;
        (id == reference).then(|| title.to_owned())
    })
}

fn task_labels(task: &str) -> (&'static str, &'static str) {
    match task {
        "parabox-intro" => ("parabox", "Patrick's Parabox"),
        "swarm-farming" => ("swarm", "Swarm Farming"),
        "sausage-roll" => ("sausage", "Stephen's Sausage Roll"),
        "emergency-operator" => ("operator", "Emergency Operator"),
        "sokoban" => ("sokoban", "Sokoban Classics"),
        _ => ("unknown", "Unknown"),
    }
}

fn experience(markdown: &str, updated_at: Option<Value>) -> Value {
    let mut categories = BTreeMap::<&str, Vec<String>>::from([
        ("plan", Vec::new()),
        ("verified", Vec::new()),
        ("rejected", Vec::new()),
        ("solved", Vec::new()),
    ]);
    let mut heading = String::new();
    for raw in markdown.lines() {
        let line = raw.trim();
        if line.starts_with('#') {
            heading = line.trim_start_matches('#').trim().to_ascii_lowercase();
            continue;
        }
        let Some(item) = line
            .strip_prefix("- ")
            .or_else(|| line.strip_prefix("* "))
            .or_else(|| line.strip_prefix("+ "))
            .map(str::trim)
            .filter(|item| !item.is_empty())
        else {
            continue;
        };
        let lower = item.to_ascii_lowercase();
        let category = if lower.contains("solved")
            || lower.contains("已解")
            || heading.contains("solved")
            || heading.contains("已解")
        {
            "solved"
        } else if [
            "reject",
            "dead-end",
            "dead end",
            "invalid",
            "死路",
            "不可行",
        ]
        .iter()
        .any(|marker| lower.contains(marker) || heading.contains(marker))
        {
            "rejected"
        } else if [
            "verified",
            "confirmed",
            "reusable",
            "mechanic",
            "rule",
            "规律",
            "经验",
            "机制",
        ]
        .iter()
        .any(|marker| lower.contains(marker) || heading.contains(marker))
        {
            "verified"
        } else {
            "plan"
        };
        let normalized = item.split_whitespace().collect::<Vec<_>>().join(" ");
        let normalized = normalized.chars().take(700).collect::<String>();
        let entries = categories.get_mut(category).expect("experience category");
        if !entries.contains(&normalized) {
            entries.push(normalized);
        }
    }
    json!({
        "updated_at": updated_at.unwrap_or(Value::Null),
        "source_count": 1,
        "counts": {
            "plan": categories["plan"].len(),
            "verified": categories["verified"].len(),
            "rejected": categories["rejected"].len(),
            "solved": categories["solved"].len(),
        },
        "plan": categories["plan"].iter().rev().take(8).cloned().collect::<Vec<_>>(),
        "verified": categories["verified"].iter().rev().take(8).cloned().collect::<Vec<_>>(),
        "rejected": categories["rejected"].iter().rev().take(8).cloned().collect::<Vec<_>>(),
        "solved": categories["solved"].iter().rev().take(8).cloned().collect::<Vec<_>>(),
    })
}

fn archive_response(path: &Path, not_found: &str) -> Response {
    if !path.is_file() {
        return error_response(StatusCode::NOT_FOUND, not_found);
    }
    decoded_file_response(path, "no-store")
}

fn decoded_file_response(path: &Path, cache_control: &'static str) -> Response {
    match read_gzip(path) {
        Ok(body) => response(StatusCode::OK, body, cache_control),
        Err(error) => error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

fn json_response(status: StatusCode, value: Value) -> Response {
    match serde_json::to_vec(&value) {
        Ok(body) => response(status, body, "no-store"),
        Err(error) => error_response(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }
}

fn error_response(status: StatusCode, error: impl Into<String>) -> Response {
    let body = serde_json::to_vec(&json!({"error": error.into()})).expect("error JSON");
    response(status, body, "no-store")
}

fn response(status: StatusCode, body: Vec<u8>, cache_control: &'static str) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(
        "content-type",
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    headers.insert("cache-control", HeaderValue::from_static(cache_control));
    headers.insert("access-control-allow-origin", HeaderValue::from_static("*"));
    (status, headers, Body::from(body)).into_response()
}

fn read_gzip(path: &Path) -> Result<Vec<u8>, String> {
    let mut decoded = Vec::new();
    GzDecoder::new(File::open(path).map_err(display_error)?)
        .read_to_end(&mut decoded)
        .map_err(display_error)?;
    Ok(decoded)
}

fn read_gzip_json(path: &Path) -> Result<Value, String> {
    serde_json::from_slice(&read_gzip(path)?).map_err(display_error)
}

fn safe_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn number(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(Value::as_i64).unwrap_or(0)
}

fn boolean(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn display_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scored_events_close_reset_delimited_attempts() {
        let events = vec![
            json!({"sequence": 1, "action": {"command": "select"}, "selected": "a1"}),
            json!({"sequence": 2, "action": {"command": "move"}, "selected": "a1"}),
            json!({"sequence": 3, "action": {"command": "reset"}, "selected": "a1"}),
            json!({"sequence": 4, "action": {"command": "move"}, "selected": "a1", "score": 1, "score_delta": 1}),
        ];
        let attempts = attempts("parabox-intro", &events, Path::new("/missing"));
        assert_eq!(attempts.len(), 2);
        assert!(!attempts[0].successful);
        assert!(attempts[1].successful);
        assert_eq!(attempts[1].score, 1);
    }

    #[test]
    fn history_operations_are_eliminated_before_reset_segmentation() {
        let events = vec![
            json!({"sequence": 1, "action": {"command": "move"}, "state": {"position": 1}}),
            json!({"sequence": 2, "action": {"command": "reset"}, "state": {"position": 0}}),
            json!({"sequence": 3, "action": {"command": "move"}, "state": {"position": 2}}),
            json!({"sequence": 4, "action": {"command": "undo"}, "state": {"position": 1}}),
            json!({"sequence": 5, "action": {"command": "move"}, "state": {"position": 3}, "score": 1, "score_delta": 1}),
        ];
        let logical = logical_replay_events(&events).expect("logical events");
        assert_eq!(
            logical
                .iter()
                .filter_map(|event| event.pointer("/action/command").and_then(Value::as_str))
                .collect::<Vec<_>>(),
            ["move", "move"]
        );
        let attempts = attempts("sausage-roll", &logical, Path::new("/missing"));
        assert_eq!(attempts.len(), 1);
        assert!(attempts[0].successful);
    }

    #[test]
    fn shared_assets_become_content_addressed_references() {
        let references = asset_references(&[json!({
            "assets": {
                "overworld_map": {
                    "encoding": "gzip",
                    "object": "abc123",
                    "uncompressed_bytes": 4096
                }
            }
        })]);
        assert_eq!(references["overworld_map"]["id"], "abc123");
        assert_eq!(references["overworld_map"]["bytes"], 4096);
        assert_eq!(
            references["overworld_map"]["media_type"],
            "application/json"
        );
    }

    #[test]
    fn experience_uses_only_explicit_markdown_bullets() {
        let value = experience(
            "# Current plan\n- try the west route\n# Verified mechanics\n- confirmed: boxes preserve direction\n# Rejected\n- dead-end at the north wall",
            Some(Value::from(42)),
        );
        assert_eq!(value["counts"]["plan"], 1);
        assert_eq!(value["counts"]["verified"], 1);
        assert_eq!(value["counts"]["rejected"], 1);
        assert_eq!(value["updated_at"], 42);
    }

    #[test]
    fn safe_ids_reject_paths() {
        assert!(safe_id("parabox-run_1"));
        assert!(!safe_id("../journal"));
        assert!(!safe_id("run/child"));
    }

    #[test]
    fn sokoban_uses_campaign_score_and_level_objective() {
        let state = json!({
            "campaign": {"score": 25, "max_score": 305},
            "level": {"id": "novoban-025", "title": "Novoban 25"}
        });
        assert_eq!(
            score("sokoban", &state, &[], Path::new("/missing")),
            (25, 305, "novoban-025 / Novoban 25".into())
        );
        assert_eq!(task_labels("sokoban"), ("sokoban", "Sokoban Classics"));
    }
}
