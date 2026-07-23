use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

pub mod gateway;
mod replay;
pub use replay::replay_projection;

pub const RUN_EVENT_SCHEMA: &str = "benchmark-run-event-v1";
pub const RUN_MANIFEST_SCHEMA: &str = "benchmark-run-manifest-v1";

#[derive(Debug, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Request {
    Ping,
    Register(Register),
    Lifecycle(Lifecycle),
    Shutdown,
}

#[derive(Debug, Deserialize)]
pub struct Register {
    pub segment_id: String,
    pub created_at_ms: u64,
    pub observer_dir: PathBuf,
    pub job_id: String,
    pub job_name: String,
    pub trial_id: String,
    pub trial_name: String,
    pub task: String,
    pub model: Option<String>,
    pub agent: Option<String>,
    pub effort: Option<String>,
    pub agent_dir: Option<PathBuf>,
    pub workspace_dir: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
pub struct Lifecycle {
    pub segment_id: String,
    pub event: String,
    pub timestamp_ms: u64,
    #[serde(default)]
    pub exception_type: Option<String>,
    #[serde(default)]
    pub rewards: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct Response {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    pub fn ok() -> Self {
        Self {
            ok: true,
            error: None,
        }
    }

    pub fn error(error: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: Some(error.into()),
        }
    }
}

#[derive(Debug)]
struct Segment {
    id: String,
    inbox_path: PathBuf,
    base_elapsed_ms: u64,
    source_sequences: HashSet<u64>,
    inbox_offset: u64,
    inbox_remainder: Vec<u8>,
    inbox_line_sequence: u64,
    execution_windows: Vec<(u64, Option<u64>)>,
    agent_dir: Option<PathBuf>,
    workspace_dir: Option<PathBuf>,
    agent_files: HashMap<PathBuf, SourceTail>,
    note_digest: Option<Vec<u8>>,
}

#[derive(Debug, Default)]
struct SourceTail {
    offset: u64,
    remainder: Vec<u8>,
}

impl Segment {
    fn effective_at(&self, timestamp_ms: u64) -> u64 {
        self.execution_windows
            .iter()
            .fold(self.base_elapsed_ms, |elapsed, (started, finished)| {
                if timestamp_ms <= *started {
                    elapsed
                } else {
                    elapsed
                        + finished
                            .unwrap_or(timestamp_ms)
                            .min(timestamp_ms)
                            .saturating_sub(*started)
                }
            })
    }
}

#[derive(Debug)]
struct State {
    chain_id: String,
    journal_path: PathBuf,
    object_dir: PathBuf,
    chain_sequence: u64,
    chain_elapsed_ms: u64,
    parent_segment_id: Option<String>,
    segment: Option<Segment>,
}

impl State {
    fn register(&mut self, request: Register) -> Result<(), String> {
        if self.segment.is_some() {
            return Err("only one segment may be registered per recorder".into());
        }
        fs::create_dir_all(&request.observer_dir).map_err(display_error)?;
        let inbox_path = request.observer_dir.join("game-inbox.jsonl");
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&inbox_path)
            .map_err(display_error)?;
        let manifest_path = request.observer_dir.join("manifest.json");
        let manifest = json!({
            "schema": RUN_MANIFEST_SCHEMA,
            "chain_id": self.chain_id,
            "segment_id": request.segment_id,
            "parent_segment_id": self.parent_segment_id,
            "job_id": request.job_id,
            "job_name": request.job_name,
            "trial_id": request.trial_id,
            "trial_name": request.trial_name,
            "task": request.task,
            "created_at_ms": request.created_at_ms,
            "journal_path": absolute(&self.journal_path),
            "objects_path": absolute(&self.object_dir),
            "inbox": "game-inbox.jsonl",
        });
        atomic_json(&manifest_path, &manifest)?;
        let segment = Segment {
            id: request.segment_id.clone(),
            inbox_path,
            base_elapsed_ms: self.chain_elapsed_ms,
            source_sequences: HashSet::new(),
            inbox_offset: 0,
            inbox_remainder: Vec::new(),
            inbox_line_sequence: 0,
            execution_windows: Vec::new(),
            agent_dir: request.agent_dir.clone(),
            workspace_dir: request.workspace_dir.clone(),
            agent_files: HashMap::new(),
            note_digest: None,
        };
        self.segment = Some(segment);
        if self.chain_sequence == 0 {
            self.append_runtime(
                request.created_at_ms,
                "chain_created",
                json!({"job_id": request.job_id, "job_name": request.job_name}),
            )?;
        }
        self.append_runtime(
            request.created_at_ms,
            "segment_registered",
            json!({
                "parent_segment_id": self.parent_segment_id,
                "job_id": request.job_id,
                "job_name": request.job_name,
                "trial": request.trial_name,
                "task": request.task,
                "model": request.model,
                "agent": request.agent,
                "effort": request.effort,
                "agent_dir": request.agent_dir.as_ref().map(|path| absolute(path)),
                "workspace_dir": request.workspace_dir.as_ref().map(|path| absolute(path)),
                "observer_inbox": absolute(&self.segment.as_ref().expect("segment").inbox_path),
            }),
        )
    }

    fn lifecycle(&mut self, request: Lifecycle) -> Result<(), String> {
        if self.segment.as_ref().map(|segment| segment.id.as_str())
            != Some(request.segment_id.as_str())
        {
            return Err(format!("unknown segment {}", request.segment_id));
        }
        self.drain()?;
        match request.event.as_str() {
            "agent-start" => {
                self.segment
                    .as_mut()
                    .expect("segment")
                    .execution_windows
                    .push((request.timestamp_ms, None));
                self.append_runtime(request.timestamp_ms, "agent_execution_started", json!({}))
            }
            "agent-end" => {
                if let Some((_, finished)) = self
                    .segment
                    .as_mut()
                    .expect("segment")
                    .execution_windows
                    .last_mut()
                    && finished.is_none()
                {
                    *finished = Some(request.timestamp_ms);
                }
                self.drain()?;
                self.append_runtime(request.timestamp_ms, "agent_execution_finished", json!({}))
            }
            "environment-start" => {
                self.append_runtime(request.timestamp_ms, "environment_starting", json!({}))
            }
            "verification-start" => {
                self.append_runtime(request.timestamp_ms, "verification_started", json!({}))
            }
            "cancel" => self.append_runtime(request.timestamp_ms, "segment_cancelled", json!({})),
            "end" => {
                self.drain()?;
                let disposition = match request.exception_type.as_deref() {
                    Some("CancelledError") => "cancelled",
                    Some(_) => "error",
                    None => "agent_stopped",
                };
                self.append_runtime(
                    request.timestamp_ms,
                    "segment_finished",
                    json!({
                        "disposition": disposition,
                        "exception_type": request.exception_type,
                        "rewards": request.rewards,
                    }),
                )?;
                let segment = self.segment.as_ref().expect("segment");
                self.chain_elapsed_ms = self
                    .chain_elapsed_ms
                    .max(segment.effective_at(request.timestamp_ms));
                self.parent_segment_id = Some(segment.id.clone());
                Ok(())
            }
            other => Err(format!("unknown lifecycle event {other}")),
        }
    }

    fn drain(&mut self) -> Result<(), String> {
        self.drain_game()?;
        self.drain_agent()?;
        self.drain_notes()
    }

    fn drain_game(&mut self) -> Result<(), String> {
        let pending = {
            let Some(segment) = self.segment.as_mut() else {
                return Ok(());
            };
            let mut file = match File::open(&segment.inbox_path) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(error) => return Err(error.to_string()),
            };
            file.seek(SeekFrom::Start(segment.inbox_offset))
                .map_err(display_error)?;
            let mut addition = Vec::new();
            file.read_to_end(&mut addition).map_err(display_error)?;
            if addition.is_empty() {
                return Ok(());
            }
            segment.inbox_offset += addition.len() as u64;
            let mut content = std::mem::take(&mut segment.inbox_remainder);
            content.extend(addition);
            let mut lines = content
                .split(|byte| *byte == b'\n')
                .map(Vec::from)
                .collect::<Vec<_>>();
            segment.inbox_remainder = lines.pop().unwrap_or_default();

            let mut pending = Vec::new();
            for line in lines {
                if line.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                segment.inbox_line_sequence += 1;
                let Ok(payload) = serde_json::from_slice::<Value>(&line) else {
                    continue;
                };
                let Some(object) = payload.as_object() else {
                    continue;
                };
                let source_sequence = object
                    .get("sequence")
                    .and_then(Value::as_u64)
                    .unwrap_or(segment.inbox_line_sequence);
                if !segment.source_sequences.insert(source_sequence) {
                    continue;
                }
                let timestamp_ms = object
                    .get("timestamp_ms")
                    .and_then(Value::as_u64)
                    .unwrap_or_else(now_ms);
                let event_type = if object
                    .get("score_delta")
                    .and_then(Value::as_i64)
                    .unwrap_or(0)
                    > 0
                {
                    "score_changed"
                } else {
                    "game_event"
                };
                pending.push((source_sequence, event_type, timestamp_ms, payload));
            }
            pending
        };
        for (source_sequence, event_type, timestamp_ms, payload) in pending {
            self.append(
                "game",
                Some(source_sequence),
                event_type,
                timestamp_ms,
                payload,
            )?;
        }
        Ok(())
    }

    fn drain_agent(&mut self) -> Result<(), String> {
        let pending = {
            let Some(segment) = self.segment.as_mut() else {
                return Ok(());
            };
            let Some(agent_dir) = segment.agent_dir.as_ref() else {
                return Ok(());
            };
            if !agent_dir.is_dir() {
                return Ok(());
            }
            let paths = WalkDir::new(agent_dir)
                .into_iter()
                .filter_map(Result::ok)
                .filter(|entry| {
                    entry.file_type().is_file()
                        && entry.path().extension().and_then(|value| value.to_str())
                            == Some("jsonl")
                })
                .map(|entry| entry.into_path())
                .collect::<Vec<_>>();
            let mut pending = Vec::new();
            for path in paths {
                let tail = segment.agent_files.entry(path.clone()).or_default();
                let mut file = match File::open(&path) {
                    Ok(file) => file,
                    Err(_) => continue,
                };
                let length = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
                if length < tail.offset {
                    tail.offset = 0;
                    tail.remainder.clear();
                }
                file.seek(SeekFrom::Start(tail.offset))
                    .map_err(display_error)?;
                let mut addition = Vec::new();
                file.read_to_end(&mut addition).map_err(display_error)?;
                if addition.is_empty() {
                    continue;
                }
                tail.offset += addition.len() as u64;
                let mut content = std::mem::take(&mut tail.remainder);
                content.extend(addition);
                let mut lines = content
                    .split(|byte| *byte == b'\n')
                    .map(Vec::from)
                    .collect::<Vec<_>>();
                tail.remainder = lines.pop().unwrap_or_default();
                for line in lines {
                    let Ok(row) = serde_json::from_slice::<Value>(&line) else {
                        continue;
                    };
                    let timestamp_ms = message_timestamp(&row).unwrap_or_else(now_ms);
                    pending.extend(
                        agent_records(&row)
                            .into_iter()
                            .map(|(event_type, payload)| (timestamp_ms, event_type, payload)),
                    );
                }
            }
            pending
        };
        for (timestamp_ms, event_type, payload) in pending {
            self.append("agent", None, event_type, timestamp_ms, payload)?;
        }
        Ok(())
    }

    fn drain_notes(&mut self) -> Result<(), String> {
        let update = {
            let Some(segment) = self.segment.as_mut() else {
                return Ok(());
            };
            let Some(workspace) = segment.workspace_dir.as_ref() else {
                return Ok(());
            };
            if !workspace.is_dir() {
                return Ok(());
            }
            let mut notes = WalkDir::new(workspace)
                .max_depth(2)
                .into_iter()
                .filter_map(Result::ok)
                .filter(|entry| {
                    let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                    entry.file_type().is_file()
                        && entry.path().extension().and_then(|value| value.to_str()) == Some("md")
                        && (name.contains("note") || name.contains("plan"))
                })
                .map(|entry| entry.into_path())
                .collect::<Vec<_>>();
            notes.sort();
            let mut content = String::new();
            for path in notes {
                if let Ok(text) = fs::read_to_string(&path) {
                    content.push_str(&format!("\n## {}\n{text}\n", path.display()));
                }
            }
            let digest = Sha256::digest(content.as_bytes()).to_vec();
            if segment.note_digest.as_ref() == Some(&digest) {
                None
            } else {
                segment.note_digest = Some(digest);
                (!content.is_empty()).then_some(content)
            }
        };
        if let Some(content) = update {
            self.append(
                "agent",
                None,
                "experience_updated",
                now_ms(),
                json!({"markdown": content}),
            )?;
        }
        Ok(())
    }

    fn append_runtime(
        &mut self,
        timestamp_ms: u64,
        event_type: &str,
        payload: Value,
    ) -> Result<(), String> {
        self.append("runtime", None, event_type, timestamp_ms, payload)
    }

    fn append(
        &mut self,
        source: &str,
        source_sequence: Option<u64>,
        event_type: &str,
        timestamp_ms: u64,
        payload: Value,
    ) -> Result<(), String> {
        let segment = self.segment.as_ref().ok_or("segment is not registered")?;
        self.chain_sequence += 1;
        let effective_elapsed_ms = segment.effective_at(timestamp_ms);
        self.chain_elapsed_ms = self.chain_elapsed_ms.max(effective_elapsed_ms);
        let stored_payload = if source == "game" {
            externalize_game_payload(payload, &self.object_dir)?
        } else {
            payload
        };
        append_json_line(
            &self.journal_path,
            &json!({
                "schema": RUN_EVENT_SCHEMA,
                "sequence": self.chain_sequence,
                "recorded_at_ms": now_ms(),
                "source_timestamp_ms": timestamp_ms,
                "effective_elapsed_ms": effective_elapsed_ms,
                "chain_id": self.chain_id,
                "segment_id": segment.id,
                "source": source,
                "source_sequence": source_sequence,
                "type": event_type,
                "payload": stored_payload,
            }),
        )
    }

    fn shutdown(&mut self) -> Result<(), String> {
        self.drain()?;
        if let Some(segment) = &self.segment {
            fs::write(&segment.inbox_path, b"").map_err(display_error)?;
        }
        Ok(())
    }
}

pub struct Recorder {
    state: Arc<Mutex<State>>,
    _lock: File,
    ingestor: Option<Ingestor>,
}

impl Recorder {
    pub fn open(chain_id: &str, journal_root: &Path) -> Result<Self, String> {
        let chain_id = safe_id(chain_id)?;
        let chain_dir = journal_root.join(&chain_id);
        fs::create_dir_all(&chain_dir).map_err(display_error)?;
        let lock = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(chain_dir.join("writer.lock"))
            .map_err(display_error)?;
        lock.try_lock_exclusive()
            .map_err(|_| format!("run journal {chain_id:?} already has an active writer"))?;
        let journal_path = chain_dir.join("journal.jsonl");
        let object_dir = chain_dir.join("objects");
        fs::create_dir_all(&object_dir).map_err(display_error)?;
        let (chain_sequence, chain_elapsed_ms, parent_segment_id) =
            load_chain_state(&journal_path)?;
        Ok(Self {
            state: Arc::new(Mutex::new(State {
                chain_id,
                journal_path,
                object_dir,
                chain_sequence,
                chain_elapsed_ms,
                parent_segment_id,
                segment: None,
            })),
            _lock: lock,
            ingestor: None,
        })
    }

    pub fn handle(&mut self, request: Request) -> Result<bool, String> {
        match request {
            Request::Ping => Ok(false),
            Request::Register(request) => {
                let inbox = request.observer_dir.join("game-inbox.jsonl");
                self.state.lock().map_err(poisoned)?.register(request)?;
                self.ingestor = Some(Ingestor::start(Arc::clone(&self.state), &inbox)?);
                Ok(false)
            }
            Request::Lifecycle(request) => {
                self.state.lock().map_err(poisoned)?.lifecycle(request)?;
                Ok(false)
            }
            Request::Shutdown => {
                if let Some(ingestor) = self.ingestor.take() {
                    ingestor.stop();
                }
                self.state.lock().map_err(poisoned)?.shutdown()?;
                Ok(true)
            }
        }
    }
}

struct Ingestor {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

impl Ingestor {
    fn start(state: Arc<Mutex<State>>, _inbox: &Path) -> Result<Self, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                let Ok(mut state) = state.lock() else {
                    break;
                };
                let _ = state.drain();
                drop(state);
                thread::sleep(Duration::from_millis(200));
            }
        });
        Ok(Self { stop, thread })
    }

    fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.thread.join();
    }
}

fn message_timestamp(row: &Value) -> Option<u64> {
    let value = row.get("timestamp").or_else(|| row.get("created_at"))?;
    if let Some(value) = value.as_u64() {
        return Some(if value < 1_000_000_000_000 {
            value * 1000
        } else {
            value
        });
    }
    let value = value.as_str()?;
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|timestamp| timestamp.timestamp_millis().max(0) as u64)
}

fn agent_message_text(payload: &Value) -> Option<String> {
    if payload.get("type").and_then(Value::as_str) == Some("item.completed")
        && payload.pointer("/item/type").and_then(Value::as_str) == Some("agent_message")
    {
        return payload
            .pointer("/item/text")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(|text| text.chars().take(2400).collect());
    }
    if matches!(
        payload.get("type").and_then(Value::as_str),
        Some("message" | "assistant")
    ) {
        let content = payload
            .get("content")
            .or_else(|| payload.pointer("/message/content"))
            .and_then(Value::as_array)?;
        let text = content
            .iter()
            .filter_map(|item| {
                matches!(
                    item.get("type").and_then(Value::as_str),
                    Some("output_text" | "text")
                )
                .then(|| item.get("text").and_then(Value::as_str))
                .flatten()
            })
            .collect::<Vec<_>>()
            .join("\n");
        return (!text.trim().is_empty()).then(|| text.chars().take(2400).collect());
    }
    payload
        .get("message")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(|text| text.chars().take(2400).collect())
}

fn agent_records(row: &Value) -> Vec<(&'static str, Value)> {
    let payload = row.get("payload").unwrap_or(row);
    let mut records = Vec::new();
    if let Some(text) = agent_message_text(payload) {
        records.push(("agent_message", json!({"text": text})));
    }
    if payload.get("type").and_then(Value::as_str) == Some("custom_tool_call") {
        let tool = payload
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let value = payload
            .get("input")
            .or_else(|| payload.get("arguments"))
            .cloned()
            .unwrap_or(Value::Null);
        records.push((
            "agent_action",
            json!({"kind": "tool_use", "tool": tool, "value": value}),
        ));
    }
    if row.get("type").and_then(Value::as_str) == Some("item.started") {
        if row.pointer("/item/type").and_then(Value::as_str) == Some("command_execution") {
            records.push((
                "agent_action",
                json!({
                    "kind": "command",
                    "tool": "shell",
                    "value": row.pointer("/item/command").cloned().unwrap_or(Value::Null),
                }),
            ));
        }
        if row.pointer("/item/type").and_then(Value::as_str) == Some("file_change")
            && let Some(changes) = row.pointer("/item/changes").and_then(Value::as_array)
        {
            for change in changes {
                records.push((
                    "agent_action",
                    json!({
                        "kind": "file_change",
                        "tool": "file_change",
                        "value": change.get("path").cloned().unwrap_or(Value::Null),
                    }),
                ));
            }
        }
    }
    let content = payload
        .get("content")
        .or_else(|| payload.pointer("/message/content"))
        .and_then(Value::as_array);
    if let Some(content) = content {
        for block in content {
            if block.get("type").and_then(Value::as_str) != Some("tool_use") {
                continue;
            }
            let tool = block
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let value = block.get("input").cloned().unwrap_or(Value::Null);
            records.push((
                "agent_action",
                json!({"kind": "tool_use", "tool": tool, "value": value}),
            ));
        }
    }
    records
}

fn load_chain_state(path: &Path) -> Result<(u64, u64, Option<String>), String> {
    let Ok(file) = File::open(path) else {
        return Ok((0, 0, None));
    };
    let mut sequence = 0;
    let mut elapsed = 0;
    let mut parent = None;
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let Ok(row) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        sequence = sequence.max(row.get("sequence").and_then(Value::as_u64).unwrap_or(0));
        elapsed = elapsed.max(
            row.get("effective_elapsed_ms")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        );
        if row.get("type").and_then(Value::as_str) == Some("segment_finished") {
            parent = row
                .get("segment_id")
                .and_then(Value::as_str)
                .map(str::to_owned);
        }
    }
    Ok((sequence, elapsed, parent))
}

fn externalize_game_payload(mut payload: Value, object_dir: &Path) -> Result<Value, String> {
    if let Some(object) = payload.as_object_mut()
        && let Some(Value::Object(mut state)) = object.remove("state")
    {
        if let Some(Value::Object(scene)) = object.remove("scene") {
            state.insert("observer_scene".into(), Value::Object(scene));
        }
        let encoded = canonical_json(&Value::Object(state))?;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&encoded).map_err(display_error)?;
        let compressed = encoder.finish().map_err(display_error)?;
        object.insert(
            "state_snapshot".into(),
            json!({
                "encoding": "gzip+base64",
                "uncompressed_bytes": encoded.len(),
                "data": BASE64.encode(compressed),
            }),
        );
    }
    externalize_observer_blobs(payload, object_dir)
}

fn externalize_observer_blobs(value: Value, object_dir: &Path) -> Result<Value, String> {
    match value {
        Value::Array(values) => values
            .into_iter()
            .map(|value| externalize_observer_blobs(value, object_dir))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Value::Object(mut object)
            if object.get("encoding").and_then(Value::as_str) == Some("gzip+base64")
                && object.get("data").and_then(Value::as_str).is_some() =>
        {
            let data = object
                .remove("data")
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap();
            let compressed = match BASE64.decode(&data) {
                Ok(value) => value,
                Err(_) => {
                    object.insert("data".into(), Value::String(data));
                    return Ok(Value::Object(object));
                }
            };
            let mut decoded = Vec::new();
            if GzDecoder::new(compressed.as_slice())
                .read_to_end(&mut decoded)
                .is_err()
            {
                object.insert("data".into(), Value::String(BASE64.encode(compressed)));
                return Ok(Value::Object(object));
            }
            if object
                .get("uncompressed_bytes")
                .and_then(Value::as_u64)
                .is_some_and(|expected| expected != decoded.len() as u64)
            {
                object.insert("data".into(), Value::String(BASE64.encode(compressed)));
                return Ok(Value::Object(object));
            }
            let digest = hex_sha256(&compressed);
            let destination = object_dir.join(format!("{digest}.json.gz"));
            if !destination.exists() {
                let temporary = object_dir.join(format!(".{digest}.{}.tmp", std::process::id()));
                fs::write(&temporary, &compressed).map_err(display_error)?;
                if let Err(error) = fs::rename(&temporary, &destination) {
                    let _ = fs::remove_file(&temporary);
                    if !destination.exists() {
                        return Err(error.to_string());
                    }
                }
            }
            object.insert("encoding".into(), Value::String("gzip".into()));
            object.insert("object".into(), Value::String(digest));
            object.insert(
                "compressed_bytes".into(),
                Value::from(compressed.len() as u64),
            );
            object.insert("content_sha256".into(), Value::String(hex_sha256(&decoded)));
            if let Ok(Value::Array(values)) = serde_json::from_slice::<Value>(&decoded)
                && let Some(Value::Object(final_value)) = values.last()
                && let Some(state) = final_value.get("state")
            {
                object.insert(
                    "final_state_sha256".into(),
                    Value::String(hex_sha256(&canonical_json(state)?)),
                );
            }
            Ok(Value::Object(object))
        }
        Value::Object(object) => object
            .into_iter()
            .map(|(key, value)| {
                externalize_observer_blobs(value, object_dir).map(|value| (key, value))
            })
            .collect::<Result<Map<_, _>, _>>()
            .map(Value::Object),
        other => Ok(other),
    }
}

fn append_json_line(path: &Path, value: &Value) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(display_error)?;
    serde_json::to_writer(&mut file, value).map_err(display_error)?;
    file.write_all(b"\n").map_err(display_error)
}

fn atomic_json(path: &Path, value: &Value) -> Result<(), String> {
    let temporary = path.with_extension("tmp");
    let encoded = serde_json::to_vec_pretty(value).map_err(display_error)?;
    fs::write(&temporary, encoded).map_err(display_error)?;
    fs::rename(temporary, path).map_err(display_error)
}

fn canonical_json(value: &Value) -> Result<Vec<u8>, String> {
    serde_json::to_vec(&sort_value(value)).map_err(display_error)
}

fn sort_value(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort();
            Value::Object(
                keys.into_iter()
                    .map(|key| (key.clone(), sort_value(&object[key])))
                    .collect(),
            )
        }
        Value::Array(values) => Value::Array(values.iter().map(sort_value).collect()),
        other => other.clone(),
    }
}

fn hex_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn safe_id(value: &str) -> Result<String, String> {
    let cleaned = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || "._-".contains(character) {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    let cleaned = cleaned.trim_matches(['-', '.']).to_owned();
    if cleaned.is_empty() {
        Err("chain_id must contain at least one safe character".into())
    } else {
        Ok(cleaned)
    }
}

fn absolute(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_owned())
        .to_string_lossy()
        .into_owned()
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

fn poisoned<T>(_: std::sync::PoisonError<T>) -> String {
    "recorder lock was poisoned".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_ids_are_stable() {
        assert_eq!(safe_id("a/b c").unwrap(), "a-b-c");
        assert!(safe_id("...").is_err());
    }

    #[test]
    fn effective_time_counts_only_execution_windows() {
        let segment = Segment {
            id: "segment".into(),
            inbox_path: PathBuf::new(),
            base_elapsed_ms: 2_000,
            source_sequences: HashSet::new(),
            inbox_offset: 0,
            inbox_remainder: Vec::new(),
            inbox_line_sequence: 0,
            execution_windows: vec![(10_000, Some(15_000)), (20_000, None)],
            agent_dir: None,
            workspace_dir: None,
            agent_files: HashMap::new(),
            note_digest: None,
        };
        assert_eq!(segment.effective_at(9_000), 2_000);
        assert_eq!(segment.effective_at(12_000), 4_000);
        assert_eq!(segment.effective_at(18_000), 7_000);
        assert_eq!(segment.effective_at(23_000), 10_000);
    }

    #[test]
    fn agent_records_capture_visible_messages_and_tool_actions() {
        let message = agent_records(&json!({
            "payload": {
                "type": "item.completed",
                "item": {"type": "agent_message", "text": "Inspect the lower box."}
            }
        }));
        assert_eq!(message[0].0, "agent_message");
        assert_eq!(message[0].1["text"], "Inspect the lower box.");

        let action = agent_records(&json!({
            "type": "response_item",
            "payload": {
                "type": "custom_tool_call",
                "name": "exec",
                "input": "await tools.exec_command({cmd:\"parabox move up\"})"
            }
        }));
        assert_eq!(action[0].0, "agent_action");
        assert_eq!(action[0].1["tool"], "exec");
    }
}
