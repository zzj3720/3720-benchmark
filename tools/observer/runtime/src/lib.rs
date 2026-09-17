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
use flate2::read::GzDecoder;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

pub mod archive;
pub mod contract;
pub mod gateway;
pub mod index;
mod replay;
pub mod storage;
pub use replay::replay_projection;

pub const RUN_EVENT_SCHEMA: &str = "benchmark-run-event-v1";
pub const RUN_MANIFEST_SCHEMA: &str = "benchmark-run-manifest-v1";

#[derive(Debug, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Request {
    Ping,
    Register(Register),
    Attach(Attach),
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
pub struct Attach {
    pub segment_id: String,
    pub observer_dir: PathBuf,
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
    agent_after_ms: u64,
    note_digest: Option<Vec<u8>>,
}

#[derive(Debug, Default)]
struct SourceTail {
    offset: u64,
    remainder: Vec<u8>,
    seen: HashSet<[u8; 32]>,
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
    chain_game_score: Option<i64>,
    history_chunk_bytes: usize,
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
            "journal_index_path": absolute(&self.journal_path.with_file_name(storage::JOURNAL_INDEX)),
            "history_chunk_bytes": self.history_chunk_bytes,
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
            agent_after_ms: request.created_at_ms,
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

    fn attach(&mut self, request: Attach) -> Result<(), String> {
        if self.segment.is_some() {
            return Err("only one segment may be registered per recorder".into());
        }
        let attachment = load_attachment_state(&self.journal_path, &request.segment_id)?;
        if attachment.finished {
            return Err(format!(
                "segment {} is already finished",
                request.segment_id
            ));
        }
        let reattached_at_ms = attachment.source_timestamp_ms;
        fs::create_dir_all(&request.observer_dir).map_err(display_error)?;
        let inbox_path = request.observer_dir.join("game-inbox.jsonl");
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&inbox_path)
            .map_err(display_error)?;
        let mut agent_files = HashMap::new();
        if let Some(agent_dir) = &request.agent_dir {
            for path in jsonl_files(agent_dir) {
                agent_files.insert(
                    path.clone(),
                    SourceTail {
                        offset: agent_offset_after_timestamp(&path, attachment.agent_timestamp_ms)?,
                        ..SourceTail::default()
                    },
                );
            }
        }
        self.segment = Some(Segment {
            id: request.segment_id,
            inbox_path: inbox_path.clone(),
            base_elapsed_ms: self.chain_elapsed_ms,
            source_sequences: HashSet::new(),
            inbox_offset: game_offset_after_lines(&inbox_path, attachment.game_source_sequence)?,
            inbox_remainder: Vec::new(),
            inbox_line_sequence: attachment.game_source_sequence,
            execution_windows: vec![(attachment.source_timestamp_ms, None)],
            agent_dir: request.agent_dir,
            workspace_dir: request.workspace_dir,
            agent_files,
            agent_after_ms: attachment.agent_timestamp_ms.unwrap_or(0),
            note_digest: None,
        });
        self.drain()?;
        self.append_runtime(
            reattached_at_ms,
            "recorder_reattached",
            json!({
                "observer_inbox": absolute(&inbox_path),
                "recorded_game_lines": attachment.game_source_sequence,
                "recorded_agent_through_ms": attachment.agent_timestamp_ms,
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
        let checkpoint = self.segment.as_ref().map(|segment| {
            (
                segment.inbox_offset,
                segment.inbox_remainder.clone(),
                segment.inbox_line_sequence,
            )
        });
        let chain_game_score = self.chain_game_score;
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
                if segment.source_sequences.contains(&source_sequence) {
                    continue;
                }
                let timestamp_ms = object
                    .get("timestamp_ms")
                    .and_then(Value::as_u64)
                    .unwrap_or_else(now_ms);
                if stale_resume_startup(chain_game_score, segment.base_elapsed_ms, &payload) {
                    continue;
                }
                let event_type = if object
                    .get("score_delta")
                    .and_then(Value::as_i64)
                    .unwrap_or(0)
                    != 0
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
            if self
                .segment
                .as_ref()
                .unwrap()
                .source_sequences
                .contains(&source_sequence)
            {
                continue;
            }
            let score = payload.get("score").and_then(Value::as_i64);
            if let Err(error) = self.append(
                "game",
                Some(source_sequence),
                event_type,
                timestamp_ms,
                payload,
            ) {
                if let (Some(segment), Some((offset, remainder, line_sequence))) =
                    (self.segment.as_mut(), checkpoint)
                {
                    segment.inbox_offset = offset;
                    segment.inbox_remainder = remainder;
                    segment.inbox_line_sequence = line_sequence;
                }
                return Err(error);
            }
            self.segment
                .as_mut()
                .unwrap()
                .source_sequences
                .insert(source_sequence);
            if let Some(score) = score {
                self.chain_game_score = Some(score);
            }
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
            let mut pending = Vec::new();
            for path in jsonl_files(agent_dir) {
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
                    if !first_seen(tail, &line) {
                        continue;
                    }
                    let Ok(row) = serde_json::from_slice::<Value>(&line) else {
                        continue;
                    };
                    let timestamp_ms = message_timestamp(&row).unwrap_or_else(now_ms);
                    if timestamp_ms <= segment.agent_after_ms {
                        continue;
                    }
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
        let sequence = self
            .chain_sequence
            .checked_add(1)
            .ok_or("run sequence exhausted")?;
        let effective_elapsed_ms = segment.effective_at(timestamp_ms);
        let stored_payload = if source == "game" {
            externalize_game_payload(payload, &self.object_dir)?
        } else {
            payload
        };
        append_json_line(
            &self.journal_path,
            &json!({
                "schema": RUN_EVENT_SCHEMA,
                "sequence": sequence,
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
        )?;
        self.chain_sequence = sequence;
        self.chain_elapsed_ms = self.chain_elapsed_ms.max(effective_elapsed_ms);
        if fs::metadata(&self.journal_path)
            .is_ok_and(|metadata| metadata.len() >= self.history_chunk_bytes as u64)
            && let Err(error) = archive::rotate_history(
                self.journal_path.parent().unwrap(),
                self.history_chunk_bytes,
            )
        {
            // The record is already durable; a compression failure must not
            // turn a successful append into a duplicate on ingestion retry.
            eprintln!("history rollover deferred: {error}");
        }
        Ok(())
    }

    fn shutdown(&mut self) -> Result<(), String> {
        self.drain()?;
        if let Some(segment) = &self.segment {
            fs::write(&segment.inbox_path, b"").map_err(display_error)?;
        }
        Ok(())
    }

    fn publish_lease(&self, healthy: bool) -> Result<(), String> {
        let Some(segment) = &self.segment else {
            return Ok(());
        };
        let timestamp = now_ms();
        let value = json!({
            "schema": "benchmark-writer-lease-v1",
            "chain_id": self.chain_id,
            "segment_id": segment.id,
            "updated_at_ms": timestamp,
            "sequence": self.chain_sequence,
            "healthy": healthy,
            "execution_active": segment.execution_windows.last().is_some_and(|(_, end)| end.is_none()),
            "effective_elapsed_ms": segment.effective_at(timestamp),
        });
        atomic_json(
            &self.journal_path.with_file_name("writer-lease.json"),
            &value,
        )
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
        let journal_path = storage::restore_journal(&chain_dir)?;
        let object_dir = chain_dir.join("objects");
        fs::create_dir_all(&object_dir).map_err(display_error)?;
        let (chain_sequence, chain_elapsed_ms, parent_segment_id, chain_game_score) =
            load_chain_state(&journal_path)?;
        Ok(Self {
            state: Arc::new(Mutex::new(State {
                chain_id,
                journal_path,
                object_dir,
                chain_sequence,
                chain_elapsed_ms,
                chain_game_score,
                history_chunk_bytes: archive::chunk_size(&chain_dir)?,
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
                self.state.lock().map_err(poisoned)?.publish_lease(true)?;
                self.ingestor = Some(Ingestor::start(Arc::clone(&self.state), &inbox)?);
                Ok(false)
            }
            Request::Attach(request) => {
                let inbox = request.observer_dir.join("game-inbox.jsonl");
                self.state.lock().map_err(poisoned)?.attach(request)?;
                self.state.lock().map_err(poisoned)?.publish_lease(true)?;
                self.ingestor = Some(Ingestor::start(Arc::clone(&self.state), &inbox)?);
                Ok(false)
            }
            Request::Lifecycle(request) => {
                self.state.lock().map_err(poisoned)?.lifecycle(request)?;
                self.state.lock().map_err(poisoned)?.publish_lease(true)?;
                Ok(false)
            }
            Request::Shutdown => {
                if let Some(ingestor) = self.ingestor.take() {
                    ingestor.stop();
                }
                let mut state = self.state.lock().map_err(poisoned)?;
                state.shutdown()?;
                if state
                    .segment
                    .as_ref()
                    .is_some_and(|segment| Some(&segment.id) == state.parent_segment_id.as_ref())
                {
                    archive::seal_history(
                        state
                            .journal_path
                            .parent()
                            .ok_or("missing chain directory")?,
                    )?;
                }
                Ok(true)
            }
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        if let Some(ingestor) = self.ingestor.take() {
            ingestor.stop();
        }
        if let Ok(state) = self.state.lock() {
            let _ = fs::remove_file(state.journal_path.with_file_name("writer-lease.json"));
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
            let mut last_lease = 0;
            while !thread_stop.load(Ordering::Relaxed) {
                let Ok(mut state) = state.lock() else {
                    break;
                };
                let drained = state.drain();
                if let Err(error) = &drained {
                    eprintln!("recorder ingestion failed: {error}");
                }
                if now_ms().saturating_sub(last_lease) >= 1000 {
                    if let Err(error) = state.publish_lease(drained.is_ok()) {
                        eprintln!("recorder lease failed: {error}");
                    }
                    last_lease = now_ms();
                }
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

fn first_seen(tail: &mut SourceTail, line: &[u8]) -> bool {
    tail.seen.insert(Sha256::digest(line).into())
}

fn jsonl_files(root: &Path) -> Vec<PathBuf> {
    if !root.is_dir() {
        return Vec::new();
    }
    WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_type().is_file()
                && entry.path().extension().and_then(|value| value.to_str()) == Some("jsonl")
        })
        .map(|entry| entry.into_path())
        .collect()
}

fn game_offset_after_lines(path: &Path, lines_to_skip: u64) -> Result<u64, String> {
    if lines_to_skip == 0 {
        return Ok(0);
    }
    let mut reader = BufReader::new(File::open(path).map_err(display_error)?);
    let mut offset = 0;
    let mut lines = 0;
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = reader.read_until(b'\n', &mut line).map_err(display_error)?;
        if read == 0 {
            return Err(format!(
                "observer inbox has only {lines} records; expected at least {lines_to_skip}"
            ));
        }
        offset += read as u64;
        if !line.iter().all(u8::is_ascii_whitespace) {
            lines += 1;
            if lines == lines_to_skip {
                return Ok(offset);
            }
        }
    }
}

fn agent_offset_after_timestamp(path: &Path, through_ms: Option<u64>) -> Result<u64, String> {
    let Some(through_ms) = through_ms else {
        return Ok(0);
    };
    let mut reader = BufReader::new(File::open(path).map_err(display_error)?);
    let mut offset = 0;
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = reader.read_until(b'\n', &mut line).map_err(display_error)?;
        if read == 0 {
            return Ok(offset);
        }
        let retain = serde_json::from_slice::<Value>(&line).is_ok_and(|row| {
            message_timestamp(&row).is_some_and(|timestamp| timestamp > through_ms)
                && !agent_records(&row).is_empty()
        });
        if retain {
            return Ok(offset);
        }
        offset += read as u64;
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
    if matches!(
        payload.get("type").and_then(Value::as_str),
        Some("text" | "reasoning")
    ) {
        return payload
            .pointer("/part/text")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(|text| text.chars().take(2400).collect());
    }
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
    records
}

#[derive(Default)]
struct AttachmentState {
    found: bool,
    finished: bool,
    source_timestamp_ms: u64,
    game_source_sequence: u64,
    agent_timestamp_ms: Option<u64>,
}

fn load_attachment_state(path: &Path, segment_id: &str) -> Result<AttachmentState, String> {
    let file = storage::journal_reader(path.parent().ok_or("missing chain directory")?)?;
    let mut state = AttachmentState::default();
    for line in BufReader::new(file).lines() {
        let Ok(row) = serde_json::from_str::<Value>(&line.map_err(display_error)?) else {
            continue;
        };
        if row.get("segment_id").and_then(Value::as_str) != Some(segment_id) {
            continue;
        }
        if row.get("type").and_then(Value::as_str) == Some("segment_registered") {
            state.found = true;
        }
        state.source_timestamp_ms = state.source_timestamp_ms.max(
            row.get("source_timestamp_ms")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        );
        match row.get("source").and_then(Value::as_str) {
            Some("game") => {
                state.game_source_sequence = state.game_source_sequence.max(
                    row.get("source_sequence")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                );
            }
            Some("agent") => {
                let timestamp = row
                    .get("source_timestamp_ms")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                state.agent_timestamp_ms =
                    Some(state.agent_timestamp_ms.unwrap_or(0).max(timestamp));
            }
            _ => {}
        }
        if row.get("type").and_then(Value::as_str) == Some("segment_finished") {
            state.finished = true;
        }
    }
    if !state.found {
        return Err(format!(
            "segment {segment_id} is not registered in the run journal"
        ));
    }
    if state.source_timestamp_ms == 0 {
        return Err(format!(
            "segment {segment_id} has no timestamped journal event"
        ));
    }
    Ok(state)
}

fn load_chain_state(path: &Path) -> Result<(u64, u64, Option<String>, Option<i64>), String> {
    let file = storage::journal_reader(path.parent().ok_or("missing chain directory")?)?;
    let mut sequence = 0;
    let mut elapsed = 0;
    let mut parent = None;
    let mut game_score = None;
    for line in BufReader::new(file).lines() {
        let Ok(row) = serde_json::from_str::<Value>(&line.map_err(display_error)?) else {
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
        if row.get("source").and_then(Value::as_str) == Some("game")
            && let Some(score) = row.pointer("/payload/score").and_then(Value::as_i64)
        {
            game_score = Some(score);
        }
    }
    Ok((sequence, elapsed, parent, game_score))
}

fn stale_resume_startup(
    previous_score: Option<i64>,
    base_elapsed_ms: u64,
    payload: &Value,
) -> bool {
    base_elapsed_ms > 0
        && payload.get("type").and_then(Value::as_str) == Some("sidecar_started")
        && payload.get("score").and_then(Value::as_i64) != previous_score
}

fn externalize_game_payload(mut payload: Value, object_dir: &Path) -> Result<Value, String> {
    contract::stamp(&mut payload);
    if let Some(object) = payload.as_object_mut()
        && !object.contains_key("action")
        && let Some(command) = object.get("command").and_then(Value::as_str)
    {
        object.insert("action".into(), json!({"command": command}));
    }
    if let Some(object) = payload.as_object_mut()
        && let Some(Value::Object(mut state)) = object.remove("state")
    {
        if let Some(Value::Object(scene)) = object.remove("scene") {
            state.insert("observer_scene".into(), Value::Object(scene));
        }
        let encoded = canonical_json(&Value::Object(state))?;
        object.insert(
            "state_snapshot".into(),
            storage::store_object(object_dir, &encoded)?,
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
            object.extend(
                storage::store_object(object_dir, &decoded)?
                    .as_object()
                    .unwrap()
                    .clone(),
            );
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
    let mut bytes = serde_json::to_vec(value).map_err(display_error)?;
    bytes.push(b'\n');
    let offset = file.metadata().map_err(display_error)?.len();
    if let Err(error) = file.write_all(&bytes).and_then(|_| file.sync_data()) {
        file.set_len(offset).map_err(display_error)?;
        file.sync_data().map_err(display_error)?;
        return Err(error.to_string());
    }
    Ok(())
}

fn atomic_json(path: &Path, value: &Value) -> Result<(), String> {
    let encoded = serde_json::to_vec_pretty(value).map_err(display_error)?;
    storage::atomic_write(path, &encoded)
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
    fn failed_object_write_retries_every_uncommitted_inbox_event() {
        let root =
            std::env::temp_dir().join(format!("observer-inbox-retry-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let blocked = root.join("blocked");
        fs::write(&blocked, b"not a directory").unwrap();
        let mut state = State {
            chain_id: "retry".into(),
            journal_path: root.join("journal.jsonl"),
            object_dir: blocked,
            chain_sequence: 0,
            chain_elapsed_ms: 0,
            chain_game_score: None,
            history_chunk_bytes: storage::DEFAULT_CHUNK_BYTES,
            parent_segment_id: None,
            segment: None,
        };
        let register: Register=serde_json::from_value(json!({"segment_id":"segment","created_at_ms":1,"observer_dir":root.join("inbox"),"job_id":"job","job_name":"job","trial_id":"trial","trial_name":"trial","task":"sokoban","model":"test"})).unwrap();
        state.register(register).unwrap();
        let events=(1..=2).map(|sequence|format!("{}\n",json!({"sequence":sequence,"timestamp_ms":sequence+1,"score":sequence,"score_delta":1,"state":{"level":{"id":"one"}}}))).collect::<String>();
        fs::write(root.join("inbox/game-inbox.jsonl"), events).unwrap();
        assert!(state.drain_game().is_err());
        assert_eq!(state.chain_sequence, 2);
        assert_eq!(state.segment.as_ref().unwrap().inbox_offset, 0);
        state.object_dir = root.join("objects");
        state.drain_game().unwrap();
        state.drain_game().unwrap();
        let rows = BufReader::new(storage::journal_reader(&root).unwrap())
            .lines()
            .map(|line| serde_json::from_str::<Value>(&line.unwrap()).unwrap())
            .filter(|row| row["source"] == "game")
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["source_sequence"], 1);
        assert_eq!(rows[1]["source_sequence"], 2);
        fs::remove_dir_all(root).unwrap();
    }

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
            agent_after_ms: 0,
            note_digest: None,
        };
        assert_eq!(segment.effective_at(9_000), 2_000);
        assert_eq!(segment.effective_at(12_000), 4_000);
        assert_eq!(segment.effective_at(18_000), 7_000);
        assert_eq!(segment.effective_at(23_000), 10_000);
    }

    #[test]
    fn resumed_segment_drops_only_a_conflicting_fresh_startup() {
        assert!(stale_resume_startup(
            Some(15),
            12_000,
            &json!({"type": "sidecar_started", "score": 0}),
        ));
        assert!(!stale_resume_startup(
            Some(15),
            12_000,
            &json!({"type": "sidecar_started", "score": 15}),
        ));
        assert!(!stale_resume_startup(
            Some(15),
            12_000,
            &json!({"type": "request", "score": 0}),
        ));
        assert!(!stale_resume_startup(
            None,
            0,
            &json!({"type": "sidecar_started", "score": 0}),
        ));
    }

    #[test]
    fn agent_records_capture_only_visible_messages() {
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
        assert!(action.is_empty());

        let opencode = agent_records(&json!({
            "type": "text",
            "sessionID": "ses_test",
            "part": {"type": "text", "text": "Try the lower entrance."}
        }));
        assert_eq!(opencode[0].0, "agent_message");
        assert_eq!(opencode[0].1["text"], "Try the lower entrance.");
    }

    #[test]
    fn flat_game_commands_are_normalized_when_recorded() {
        let root = std::env::temp_dir().join(format!(
            "observer-normalized-game-command-{}-{}",
            std::process::id(),
            now_ms()
        ));
        fs::create_dir_all(&root).expect("object directory");
        let payload = externalize_game_payload(
            json!({"type": "request", "command": "move", "score": 3}),
            &root,
        )
        .expect("normalized payload");
        assert_eq!(payload["action"]["command"], "move");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn rewritten_agent_session_lines_are_not_recorded_twice() {
        let root = std::env::temp_dir().join(format!(
            "observer-rewritten-agent-session-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let agent_dir = root.join("agent");
        fs::create_dir_all(&agent_dir).expect("agent directory");
        let session = agent_dir.join("session.jsonl");
        fs::write(
            &session,
            concat!(
                "{\"timestamp\":1000,\"message\":\"one\"}\n",
                "{\"timestamp\":2000,\"message\":\"a deliberately long second message\"}\n"
            ),
        )
        .expect("initial session");

        let journal_path = root.join("journal.jsonl");
        let mut state = State {
            chain_id: "rewrite-test".into(),
            journal_path: journal_path.clone(),
            object_dir: root.join("objects"),
            chain_sequence: 0,
            chain_elapsed_ms: 0,
            chain_game_score: None,
            history_chunk_bytes: storage::DEFAULT_CHUNK_BYTES,
            parent_segment_id: None,
            segment: Some(Segment {
                id: "segment".into(),
                inbox_path: root.join("inbox.jsonl"),
                base_elapsed_ms: 0,
                source_sequences: HashSet::new(),
                inbox_offset: 0,
                inbox_remainder: Vec::new(),
                inbox_line_sequence: 0,
                execution_windows: Vec::new(),
                agent_dir: Some(agent_dir),
                workspace_dir: None,
                agent_files: HashMap::new(),
                agent_after_ms: 0,
                note_digest: None,
            }),
        };
        state.drain_agent().expect("initial drain");

        fs::write(
            &session,
            concat!(
                "{\"timestamp\":1000,\"message\":\"one\"}\n",
                "{\"timestamp\":3000,\"message\":\"three\"}\n"
            ),
        )
        .expect("rewritten session");
        state.drain_agent().expect("rewrite drain");

        let messages = BufReader::new(File::open(journal_path).expect("journal"))
            .lines()
            .map(|line| serde_json::from_str::<Value>(&line.expect("journal line")).expect("event"))
            .map(|event| event["payload"]["text"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            messages,
            ["one", "a deliberately long second message", "three"]
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn restored_agent_history_is_not_replayed_in_a_new_segment() {
        let root = std::env::temp_dir().join(format!(
            "observer-restored-agent-session-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let agent_dir = root.join("agent");
        fs::create_dir_all(&agent_dir).expect("agent directory");
        let session = agent_dir.join("session.jsonl");
        fs::write(&session, "{\"timestamp\":1000,\"message\":\"restored\"}\n")
            .expect("restored session");

        let journal_path = root.join("journal.jsonl");
        let mut state = State {
            chain_id: "resume-test".into(),
            journal_path: journal_path.clone(),
            object_dir: root.join("objects"),
            chain_sequence: 0,
            chain_elapsed_ms: 0,
            chain_game_score: None,
            history_chunk_bytes: storage::DEFAULT_CHUNK_BYTES,
            parent_segment_id: None,
            segment: Some(Segment {
                id: "segment".into(),
                inbox_path: root.join("inbox.jsonl"),
                base_elapsed_ms: 0,
                source_sequences: HashSet::new(),
                inbox_offset: 0,
                inbox_remainder: Vec::new(),
                inbox_line_sequence: 0,
                execution_windows: Vec::new(),
                agent_dir: Some(agent_dir),
                workspace_dir: None,
                agent_files: HashMap::new(),
                agent_after_ms: 1_500_000,
                note_digest: None,
            }),
        };
        state.drain_agent().expect("restored history drain");
        fs::write(
            &session,
            concat!(
                "{\"timestamp\":1000,\"message\":\"restored\"}\n",
                "{\"timestamp\":2000,\"message\":\"new\"}\n"
            ),
        )
        .expect("continued session");
        state.drain_agent().expect("continued session drain");

        let messages = BufReader::new(File::open(journal_path).expect("journal"))
            .lines()
            .map(|line| serde_json::from_str::<Value>(&line.expect("journal line")).expect("event"))
            .map(|event| event["payload"]["text"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(messages, ["new"]);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn attach_continues_an_active_segment_without_replaying_recorded_rows() {
        let root = std::env::temp_dir().join(format!(
            "observer-reattach-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let journals = root.join("journals");
        let chain = journals.join("chain");
        let observer = root.join("observer");
        let agent = root.join("agent");
        fs::create_dir_all(&chain).expect("chain directory");
        fs::create_dir_all(&observer).expect("observer directory");
        fs::create_dir_all(&agent).expect("agent directory");
        let started = 1_700_000_000_000_u64;
        let journal = chain.join("journal.jsonl");
        for event in [
            json!({
                "sequence": 1,
                "source_timestamp_ms": started,
                "effective_elapsed_ms": 0,
                "segment_id": "live",
                "source": "runtime",
                "type": "segment_registered",
                "payload": {},
            }),
            json!({
                "sequence": 2,
                "source_timestamp_ms": started,
                "effective_elapsed_ms": 0,
                "segment_id": "live",
                "source": "runtime",
                "type": "agent_execution_started",
                "payload": {},
            }),
            json!({
                "sequence": 3,
                "source_timestamp_ms": started + 100,
                "effective_elapsed_ms": 100,
                "segment_id": "live",
                "source": "game",
                "source_sequence": 1,
                "type": "game_event",
                "payload": {"score": 1},
            }),
            json!({
                "sequence": 4,
                "source_timestamp_ms": started + 150,
                "effective_elapsed_ms": 150,
                "segment_id": "live",
                "source": "agent",
                "type": "agent_message",
                "payload": {"text": "old"},
            }),
        ] {
            append_json_line(&journal, &event).expect("journal event");
        }
        let inbox = observer.join("game-inbox.jsonl");
        fs::write(
            &inbox,
            [
                json!({"timestamp_ms": started + 100, "score": 1}),
                json!({"timestamp_ms": started + 200, "score": 1}),
                json!({"timestamp_ms": started + 300, "score": 2, "score_delta": 1}),
            ]
            .into_iter()
            .map(|value| serde_json::to_string(&value).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
                + "\n",
        )
        .expect("inbox");
        fs::write(
            agent.join("session.jsonl"),
            format!(
                "{{\"timestamp\":{},\"message\":\"old\"}}\n{{\"timestamp\":{},\"message\":\"new\"}}\n",
                started + 150,
                started + 250,
            ),
        )
        .expect("agent session");

        let mut recorder = Recorder::open("chain", &journals).expect("recorder");
        recorder
            .handle(Request::Attach(Attach {
                segment_id: "live".into(),
                observer_dir: observer,
                agent_dir: Some(agent),
                workspace_dir: None,
            }))
            .expect("attach");
        recorder.handle(Request::Shutdown).expect("shutdown");

        let rows = BufReader::new(File::open(&journal).expect("journal"))
            .lines()
            .map(|line| serde_json::from_str::<Value>(&line.unwrap()).unwrap())
            .collect::<Vec<_>>();
        let game_sequences = rows
            .iter()
            .filter(|row| row["source"] == "game")
            .filter_map(|row| row["source_sequence"].as_u64())
            .collect::<Vec<_>>();
        assert_eq!(game_sequences, [1, 2, 3]);
        let messages = rows
            .iter()
            .filter(|row| row["type"] == "agent_message")
            .filter_map(|row| row.pointer("/payload/text").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert_eq!(messages, ["old", "new"]);
        assert!(rows.iter().any(|row| row["type"] == "recorder_reattached"));
        fs::remove_dir_all(root).expect("cleanup");
    }
}
