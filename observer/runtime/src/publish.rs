//! Replicate the read-only live API to a remote store.
//!
//! The publisher runs beside the recorder. It projects every run with the same
//! code the gateway serves, uploads immutable bodies once, pushes changed run
//! summaries, and heartbeats so the remote side can tell when this host is gone.
//! Everything it sends is idempotent: a restart re-derives what is missing from
//! its local upload ledger and continues.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use flate2::Compression;
use flate2::write::GzEncoder;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

use crate::gateway::{Payload, Projector};
use crate::storage;

/// One body to store under `key`. `body` is already encoded as `encoding`.
pub struct Upload {
    pub key: String,
    /// Ledger entries (key, decoded digest, immutable) recorded once the sink
    /// accepts the body. A bundle records every file it contains.
    pub records: Vec<(String, String, bool)>,
    pub body: Vec<u8>,
    pub content_type: &'static str,
    pub encoding: Option<&'static str>,
    pub cache_control: &'static str,
}

pub trait Sink {
    fn put(&mut self, uploads: &[Upload]) -> Result<(), String>;
    fn runs(&mut self, message: &Value) -> Result<(), String>;
    fn heartbeat(&mut self, message: &Value) -> Result<(), String>;
}

const JSON: &str = "application/json; charset=utf-8";
const IMMUTABLE: &str = "public, max-age=31536000, immutable";
const NO_STORE: &str = "no-store";
const BATCH_BYTES: usize = 16 * 1024 * 1024;
const BATCH_COUNT: usize = 200;
const LIVE_REPLAY_INTERVAL: Duration = Duration::from_secs(15);
const RAW_TAIL_INTERVAL: Duration = Duration::from_secs(600);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
/// Immutable journal files are backed up in tar bundles of about this size,
/// so small objects do not each cost a storage write.
const RAW_BUNDLE_BYTES: usize = 8 * 1024 * 1024;

pub struct Publisher<S: Sink> {
    projector: Projector,
    sink: S,
    ledger: Connection,
    pending: Vec<Upload>,
    pending_bytes: usize,
    markers: Vec<String>,
    revisions: BTreeMap<String, String>,
    pushed: HashMap<String, String>,
    order: Vec<String>,
    live_replays: HashMap<String, Instant>,
    levels_dirty: bool,
    raw_tails: HashMap<String, Instant>,
    last_heartbeat: Option<Instant>,
    started_at: u64,
    host: String,
}

impl<S: Sink> Publisher<S> {
    pub fn open(root: &Path, state: &Path, sink: S) -> Result<Self, String> {
        fs::create_dir_all(state).map_err(display)?;
        let projector = Projector::open(root, &state.join("index"))?;
        let ledger = Connection::open(state.join("ledger.sqlite")).map_err(display)?;
        ledger
            .execute_batch(
                "PRAGMA journal_mode=WAL;
                 CREATE TABLE IF NOT EXISTS uploaded (key TEXT PRIMARY KEY, digest TEXT NOT NULL);
                 PRAGMA synchronous=NORMAL;
                 CREATE TABLE IF NOT EXISTS finished (key TEXT PRIMARY KEY);
                 CREATE TABLE IF NOT EXISTS attempts (
                     run TEXT NOT NULL, id INTEGER NOT NULL, reference TEXT NOT NULL,
                     title TEXT, kind TEXT NOT NULL, successful INTEGER NOT NULL,
                     score INTEGER NOT NULL, status TEXT NOT NULL,
                     PRIMARY KEY (run, id));",
            )
            .map_err(display)?;
        Ok(Self {
            projector,
            sink,
            ledger,
            pending: Vec::new(),
            pending_bytes: 0,
            markers: Vec::new(),
            revisions: BTreeMap::new(),
            pushed: HashMap::new(),
            order: Vec::new(),
            live_replays: HashMap::new(),
            levels_dirty: true,
            raw_tails: HashMap::new(),
            last_heartbeat: None,
            started_at: now_ms(),
            host: hostname(),
        })
    }

    /// Publish everything that changed since the previous call.
    pub fn cycle(&mut self) -> Result<bool, String> {
        let revisions = self.projector.source_revisions();
        let first = self.revisions.is_empty() && self.order.is_empty();
        let changed = revisions
            .iter()
            .filter(|(id, revision)| self.revisions.get(*id) != Some(*revision))
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        let live_due = self
            .live_replays
            .values()
            .any(|at| at.elapsed() >= LIVE_REPLAY_INTERVAL);
        if !first && changed.is_empty() && !live_due {
            return Ok(false);
        }
        let runs = self.projector.runs()?;
        let archive_changed = first || changed.iter().any(|id| id == "@archive");
        for run in &runs {
            let Some(id) = run.get("id").and_then(Value::as_str) else {
                continue;
            };
            let native = self.projector.is_native(id);
            let due = if native {
                first
                    || changed.iter().any(|changed| changed == id)
                    || self
                        .live_replays
                        .get(id)
                        .is_some_and(|at| at.elapsed() >= LIVE_REPLAY_INTERVAL)
            } else {
                archive_changed
            };
            if due {
                self.publish_run(id, native)?;
            }
        }
        for id in &changed {
            if id != "@archive" {
                self.publish_raw(id)?;
            }
        }
        if archive_changed {
            self.publish_archive_assets()?;
        }
        let listed = runs.iter().filter_map(|run| run.get("id").and_then(Value::as_str)).collect::<Vec<_>>();
        if listed != self.order.iter().map(String::as_str).collect::<Vec<_>>() {
            self.levels_dirty = true;
        }
        if self.levels_dirty {
            self.publish_levels(&runs)?;
            self.levels_dirty = false;
        }
        // Bodies first: a summary must never point at a revision whose detail
        // has not been stored yet.
        self.settle()?;
        self.push_runs(&runs)?;
        self.revisions = revisions;
        Ok(true)
    }

    pub fn heartbeat_if_due(&mut self) -> Result<(), String> {
        if self
            .last_heartbeat
            .is_some_and(|at| at.elapsed() < HEARTBEAT_INTERVAL)
        {
            return Ok(());
        }
        self.sink.heartbeat(&json!({
            "schema": "benchmark-live-heartbeat-v1",
            "publisher": self.host,
            "started_at_ms": self.started_at,
            "sent_at_ms": now_ms(),
            "runs": self.order.len(),
        }))?;
        self.last_heartbeat = Some(Instant::now());
        Ok(())
    }

    fn publish_run(&mut self, id: &str, native: bool) -> Result<(), String> {
        let Some(detail) = optional(self.projector.detail(id))? else {
            return Ok(());
        };
        let parsed: Value = serde_json::from_slice(&detail).map_err(display)?;
        // `observed_at` is the projection's wall clock; it alone never makes a
        // detail worth re-uploading.
        let mut stable = parsed.clone();
        if let Some(run) = stable.get_mut("run").and_then(Value::as_object_mut) {
            run.remove("observed_at");
        }
        let digest = storage::digest(&serde_json::to_vec(&stable).map_err(display)?);
        self.stage_as(format!("pub/runs/{id}/detail.json"), detail, digest, NO_STORE, false)?;
        let run = parsed.get("run").unwrap_or(&parsed);
        if let Some(assets) = run.get("asset_refs").and_then(Value::as_object) {
            let ids = assets
                .values()
                .filter_map(|asset| asset.get("id").and_then(Value::as_str))
                .map(str::to_owned)
                .collect::<Vec<_>>();
            for asset in ids {
                self.publish_asset(&asset)?;
            }
        }
        if !native {
            return self.publish_archived_replays(id);
        }
        let mut attempts = attempts(run.get("replay_groups"));
        // A run without recorded attempts (new, or from before the level
        // index existed) walks its whole catalog once.
        let full_walk = !self.has_attempts(id)?;
        self.record_attempts(id, run.get("replay_groups"))?;
        let mut more = run
            .get("replay_catalog_more")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let mut before = run.get("replay_catalog_before").and_then(Value::as_u64);
        while more {
            let Some(cursor) = before else { break };
            let key = format!("pub/runs/{id}/catalog/{cursor}.json");
            // Older pages only hold closed attempts, so a stored page and
            // everything behind it are already complete.
            if self.is_finished(&key)? && !full_walk {
                break;
            }
            let page = optional(self.projector.catalog(id, cursor))?
                .ok_or_else(|| format!("{id}: catalog page {cursor} disappeared"))?;
            let value: Value = serde_json::from_slice(&page).map_err(display)?;
            attempts.extend(self::attempts(value.get("groups")));
            self.record_attempts(id, value.get("groups"))?;
            more = value.get("more").and_then(Value::as_bool).unwrap_or(false);
            before = value.get("before").and_then(Value::as_u64);
            self.stage(key, page, IMMUTABLE, true)?;
        }
        // The walk stops at a stored catalog page, so attempts behind it come
        // from the ledger; any of them without a stored body (after the replay
        // keys change, say) is published too.
        let listed = attempts.iter().map(|(attempt, _)| *attempt).collect::<HashSet<_>>();
        let recorded = self.recorded_attempts(id)?;
        attempts.extend(recorded.into_iter().filter(|(attempt, _)| !listed.contains(attempt)));
        let mut running = false;
        for (attempt, closed) in attempts {
            let marker = format!("pub/runs/{id}/attempts/{attempt}");
            if closed && self.is_finished(&marker)? {
                continue;
            }
            self.publish_replay(id, attempt, if closed { IMMUTABLE } else { NO_STORE })?;
            if closed {
                self.enqueue_marker(marker)?;
            } else {
                running = true;
            }
        }
        if running {
            self.live_replays.insert(id.to_owned(), Instant::now());
        } else {
            self.live_replays.remove(id);
        }
        Ok(())
    }

    /// One body per attempt, plus its first frame alone for level thumbnails.
    fn publish_replay(&mut self, id: &str, attempt: u64, cache: &'static str) -> Result<(), String> {
        let Some(body) = optional(self.projector.replay(id, attempt))? else {
            return Ok(());
        };
        let mut preview: Value = serde_json::from_slice(&body).map_err(display)?;
        if let Some(frames) = preview.get_mut("frames").and_then(Value::as_array_mut) {
            frames.truncate(1);
        }
        for key in ["operations", "activity"] {
            preview[key] = json!([]);
        }
        self.stage(format!("pub/runs/{id}/attempts/{attempt}.json"), body, cache, false)?;
        self.stage(format!("pub/runs/{id}/attempts/{attempt}.preview.json"), serde_json::to_vec(&preview).map_err(display)?, cache, false)?;
        Ok(())
    }

    fn publish_archived_replays(&mut self, id: &str) -> Result<(), String> {
        let directory = self.projector.archive().join("replays").join(id);
        let Ok(entries) = fs::read_dir(&directory) else {
            return Ok(());
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(attempt) = name
                .strip_suffix(".json.gz")
                .or_else(|| name.strip_suffix(".json.zst"))
                .and_then(|value| value.parse::<u64>().ok())
            else {
                continue;
            };
            let key = format!("pub/runs/{id}/attempts/{attempt}.json");
            if self.is_finished(&key)? {
                continue;
            }
            if let Some(page) = optional(self.projector.replay(id, attempt))? {
                self.stage(key, page, IMMUTABLE, true)?;
            }
        }
        Ok(())
    }

    fn publish_asset(&mut self, asset: &str) -> Result<(), String> {
        let key = format!("pub/assets/{asset}");
        if self.is_finished(&key)? {
            return Ok(());
        }
        if let Some(body) = optional(self.projector.asset(asset))? {
            self.stage(key, body, IMMUTABLE, true)?;
        }
        Ok(())
    }

    fn publish_archive_assets(&mut self) -> Result<(), String> {
        let directory = self.projector.archive().join("assets");
        let Ok(entries) = fs::read_dir(&directory) else {
            return Ok(());
        };
        let ids = entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                name.strip_suffix(".json.gz")
                    .or_else(|| name.strip_suffix(".json.zst"))
                    .map(str::to_owned)
            })
            .collect::<Vec<_>>();
        for id in ids {
            self.publish_asset(&id)?;
        }
        Ok(())
    }

    /// Back up the authority itself, byte for byte, outside the public prefix.
    /// Sealed history and objects never change and go into tar bundles; the
    /// index and the open tail are stored individually and overwritten.
    fn publish_raw(&mut self, chain_id: &str) -> Result<(), String> {
        let chain = self.projector.journals().join(chain_id);
        if !chain.is_dir() {
            return Ok(());
        }
        let tail_due = self
            .raw_tails
            .get(chain_id)
            .is_none_or(|at| at.elapsed() >= RAW_TAIL_INTERVAL);
        let mut bundle: Vec<(String, Vec<u8>)> = Vec::new();
        let mut bundle_bytes = 0;
        for entry in walkdir::WalkDir::new(&chain)
            .sort_by_file_name()
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
        {
            let path = entry.path();
            let relative = path
                .strip_prefix(&chain)
                .map_err(display)?
                .to_string_lossy()
                .replace('\\', "/");
            let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
            if name.starts_with('.') || matches!(name, "writer.lock" | "writer-lease.json") {
                continue;
            }
            let immutable = relative.starts_with("history/") || relative.starts_with("objects/");
            let key = format!("raw/{chain_id}/{relative}");
            if immutable {
                if self.is_finished(&key)? {
                    continue;
                }
                let bytes = fs::read(path).map_err(display)?;
                bundle_bytes += bytes.len();
                bundle.push((relative, bytes));
                if bundle_bytes >= RAW_BUNDLE_BYTES {
                    self.enqueue_bundle(chain_id, std::mem::take(&mut bundle))?;
                    bundle_bytes = 0;
                }
                continue;
            }
            if name == "journal.jsonl" && !tail_due {
                continue;
            }
            let bytes = fs::read(path).map_err(display)?;
            let digest = storage::digest(&bytes);
            if self.uploaded_digest(&key)?.as_deref() == Some(digest.as_str()) {
                continue;
            }
            self.enqueue(Upload {
                records: vec![(key.clone(), digest, false)],
                key,
                body: gzip(&bytes)?,
                content_type: "application/octet-stream",
                encoding: Some("gzip"),
                cache_control: NO_STORE,
            })?;
        }
        if !bundle.is_empty() {
            self.enqueue_bundle(chain_id, bundle)?;
        }
        if tail_due {
            self.raw_tails.insert(chain_id.to_owned(), Instant::now());
        }
        Ok(())
    }

    /// Store files as one tar archive named by its content digest.
    fn enqueue_bundle(&mut self, chain_id: &str, files: Vec<(String, Vec<u8>)>) -> Result<(), String> {
        let mut archive = tar::Builder::new(Vec::new());
        let mut records = Vec::with_capacity(files.len());
        for (relative, bytes) in &files {
            let mut header = tar::Header::new_ustar();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_mtime(0);
            archive
                .append_data(&mut header, relative, bytes.as_slice())
                .map_err(display)?;
            records.push((format!("raw/{chain_id}/{relative}"), storage::digest(bytes), true));
        }
        let body = archive.into_inner().map_err(display)?;
        let key = format!("raw/{chain_id}/bundles/{}.tar", &storage::digest(&body)[..32]);
        self.enqueue(Upload {
            key,
            records,
            body,
            content_type: "application/x-tar",
            encoding: None,
            cache_control: NO_STORE,
        })
    }

    fn has_attempts(&self, run: &str) -> Result<bool, String> {
        self.ledger
            .query_row("SELECT 1 FROM attempts WHERE run = ?1 LIMIT 1", params![run], |_| Ok(()))
            .optional()
            .map(|row| row.is_some())
            .map_err(display)
    }

    /// Every attempt of `run` recorded so far, and whether it is closed.
    fn recorded_attempts(&self, run: &str) -> Result<Vec<(u64, bool)>, String> {
        let mut statement = self
            .ledger
            .prepare("SELECT id, status FROM attempts WHERE run = ?1 ORDER BY id")
            .map_err(display)?;
        statement
            .query_map(params![run], |row| {
                Ok((row.get::<_, i64>(0)? as u64, row.get::<_, String>(1)? != "running"))
            })
            .map_err(display)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(display)
    }

    /// Remember every attempt of a catalog page for the cross-run level index.
    fn record_attempts(&mut self, run: &str, groups: Option<&Value>) -> Result<(), String> {
        let transaction = self.ledger.unchecked_transaction().map_err(display)?;
        for group in groups.and_then(Value::as_array).into_iter().flatten() {
            let reference = group.get("reference").and_then(Value::as_str).unwrap_or_default();
            let title = group.get("title").and_then(Value::as_str);
            let kind = group.get("kind").and_then(Value::as_str).unwrap_or("level");
            for attempt in group.get("attempts").and_then(Value::as_array).into_iter().flatten() {
                let Some(id) = attempt.get("id").and_then(Value::as_u64) else { continue };
                let changed = transaction
                    .execute(
                        "INSERT INTO attempts (run, id, reference, title, kind, successful, score, status)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                         ON CONFLICT(run, id) DO UPDATE SET successful = excluded.successful,
                             score = excluded.score, status = excluded.status, title = excluded.title
                         WHERE successful != excluded.successful OR score != excluded.score
                             OR status != excluded.status OR title IS NOT excluded.title",
                        params![
                            run,
                            id as i64,
                            reference,
                            title,
                            kind,
                            attempt.get("successful").and_then(Value::as_bool).unwrap_or(false),
                            attempt.get("score").and_then(Value::as_i64).unwrap_or(0),
                            attempt.get("status").and_then(Value::as_str).unwrap_or("completed"),
                        ],
                    )
                    .map_err(display)?;
                self.levels_dirty |= changed > 0;
            }
        }
        transaction.commit().map_err(display)
    }

    /// Per game: an index of every level across runs, and one file per level
    /// listing each run's attempts, so viewers can compare models on a level.
    fn publish_levels(&mut self, runs: &[Value]) -> Result<(), String> {
        let mut games: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for run in runs {
            if let (Some(id), Some(game)) = (run.get("id").and_then(Value::as_str), run.get("game").and_then(Value::as_str)) {
                games.entry(game.to_owned()).or_default().push(id.to_owned());
            }
        }
        for (game, run_ids) in games {
            // reference -> (title, kind, [(run, [attempt])]) with runs in ranking order.
            let mut levels: BTreeMap<String, (Option<String>, String, Vec<(String, Vec<Value>)>)> = BTreeMap::new();
            for run in &run_ids {
                let mut statement = self
                    .ledger
                    .prepare("SELECT reference, title, kind, id, successful, score, status FROM attempts WHERE run = ?1 ORDER BY id")
                    .map_err(display)?;
                let rows = statement
                    .query_map(params![run], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, Option<String>>(1)?,
                            row.get::<_, String>(2)?,
                            json!({"id": row.get::<_, i64>(3)?, "successful": row.get::<_, bool>(4)?, "score": row.get::<_, i64>(5)?, "status": row.get::<_, String>(6)?}),
                        ))
                    })
                    .map_err(display)?;
                for row in rows {
                    let (reference, title, kind, attempt) = row.map_err(display)?;
                    let level = levels.entry(reference).or_insert_with(|| (None, kind, Vec::new()));
                    if level.0.is_none() {
                        level.0 = title;
                    }
                    match level.2.last_mut() {
                        Some((last, attempts)) if last == run => attempts.push(attempt),
                        _ => level.2.push((run.clone(), vec![attempt])),
                    }
                }
            }
            let mut ordered = levels.into_iter().collect::<Vec<_>>();
            ordered.sort_by(|(left, (_, left_kind, _)), (right, (_, right_kind, _))| {
                (left_kind != "overworld", natural_key(left)).cmp(&(right_kind != "overworld", natural_key(right)))
            });
            let mut index = Vec::with_capacity(ordered.len());
            for (reference, (title, kind, runs)) in ordered {
                let key = storage::digest(reference.as_bytes())[..16].to_owned();
                let passed = |attempts: &[Value]| attempts.iter().any(|attempt| attempt["successful"] == json!(true));
                let summary = runs
                    .iter()
                    .map(|(run, attempts)| json!({
                        "run": run,
                        "attempts": attempts.len(),
                        "passed": passed(attempts),
                        "best": attempts.iter().filter_map(|attempt| attempt["score"].as_i64()).max().unwrap_or(0),
                    }))
                    .collect::<Vec<_>>();
                let sample = runs
                    .first()
                    .and_then(|(run, attempts)| attempts.first().map(|attempt| json!({"run": run, "attempt": attempt["id"]})));
                index.push(json!({
                    "key": key,
                    "reference": reference,
                    "title": title,
                    "kind": kind,
                    "attempts": runs.iter().map(|(_, attempts)| attempts.len()).sum::<usize>(),
                    "passed_runs": runs.iter().filter(|(_, attempts)| passed(attempts)).count(),
                    "runs": summary,
                    "sample": sample,
                }));
                let detail = json!({
                    "schema": "benchmark-live-level-v1",
                    "game": game,
                    "key": key,
                    "reference": reference,
                    "title": title,
                    "kind": kind,
                    "runs": runs.iter().map(|(run, attempts)| json!({"run": run, "attempts": attempts})).collect::<Vec<_>>(),
                });
                self.stage(format!("pub/games/{game}/levels/{key}.json"), serde_json::to_vec(&detail).map_err(display)?, NO_STORE, false)?;
            }
            let body = json!({"schema": "benchmark-live-levels-v1", "game": game, "levels": index});
            self.stage(format!("pub/games/{game}/levels.json"), serde_json::to_vec(&body).map_err(display)?, NO_STORE, false)?;
        }
        Ok(())
    }

    fn push_runs(&mut self, runs: &[Value]) -> Result<(), String> {
        let order = runs
            .iter()
            .filter_map(|run| run.get("id").and_then(Value::as_str).map(str::to_owned))
            .collect::<Vec<_>>();
        let mut updates = Vec::new();
        let mut signatures = HashMap::new();
        for run in runs {
            let Some(id) = run.get("id").and_then(Value::as_str) else {
                continue;
            };
            let mut stable = run.clone();
            if let Some(object) = stable.as_object_mut() {
                object.remove("observed_at");
            }
            let signature = storage::digest(&serde_json::to_vec(&stable).map_err(display)?);
            if self.pushed.get(id) != Some(&signature) {
                updates.push(run.clone());
            }
            signatures.insert(id.to_owned(), signature);
        }
        let removed = self
            .order
            .iter()
            .filter(|id| !signatures.contains_key(*id))
            .cloned()
            .collect::<Vec<_>>();
        if updates.is_empty() && removed.is_empty() && order == self.order {
            return Ok(());
        }
        self.sink.runs(&json!({
            "schema": "benchmark-live-ingest-runs-v1",
            "generated_at": now_ms(),
            "order": order,
            "runs": updates,
            "removed": removed,
        }))?;
        self.pushed = signatures;
        self.order = order;
        Ok(())
    }

    /// Queue a JSON body unless the same bytes were already stored at `key`.
    fn stage(
        &mut self,
        key: String,
        body: Vec<u8>,
        cache: &'static str,
        finished: bool,
    ) -> Result<(), String> {
        let digest = storage::digest(&body);
        self.stage_as(key, body, digest, cache, finished)
    }

    fn stage_as(
        &mut self,
        key: String,
        body: Vec<u8>,
        digest: String,
        cache: &'static str,
        finished: bool,
    ) -> Result<(), String> {
        if self.uploaded_digest(&key)?.as_deref() == Some(digest.as_str()) {
            return Ok(());
        }
        self.enqueue(Upload {
            records: vec![(key.clone(), digest, finished)],
            key,
            body: gzip(&body)?,
            content_type: JSON,
            encoding: Some("gzip"),
            cache_control: cache,
        })
    }

    /// Record a completed group (such as every page of a closed replay) once
    /// the batch holding its last body is accepted.
    fn enqueue_marker(&mut self, key: String) -> Result<(), String> {
        self.markers.push(key);
        Ok(())
    }

    fn enqueue(&mut self, upload: Upload) -> Result<(), String> {
        self.pending_bytes += upload.body.len();
        self.pending.push(upload);
        if self.pending.len() >= BATCH_COUNT || self.pending_bytes >= BATCH_BYTES {
            self.flush()?;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), String> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let batch = std::mem::take(&mut self.pending);
        self.pending_bytes = 0;
        if let Err(error) = self.sink.put(&batch) {
            // Nothing in this batch was recorded; force a full recheck so the
            // next cycle stages it again.
            self.markers.clear();
            self.revisions.clear();
            self.order.clear();
            return Err(error);
        }
        let transaction = self.ledger.unchecked_transaction().map_err(display)?;
        for (key, digest, finished) in batch.iter().flat_map(|upload| &upload.records) {
            transaction
                .execute(
                    "INSERT INTO uploaded (key, digest) VALUES (?1, ?2)
                     ON CONFLICT(key) DO UPDATE SET digest = excluded.digest",
                    params![key, digest],
                )
                .map_err(display)?;
            if *finished {
                transaction
                    .execute("INSERT OR IGNORE INTO finished (key) VALUES (?1)", params![key])
                    .map_err(display)?;
            }
        }
        transaction.commit().map_err(display)
    }

    /// Flush bodies, then record group markers whose bodies are now stored.
    fn settle(&mut self) -> Result<(), String> {
        self.flush()?;
        for key in std::mem::take(&mut self.markers) {
            self.ledger
                .execute("INSERT OR IGNORE INTO finished (key) VALUES (?1)", params![key])
                .map_err(display)?;
        }
        Ok(())
    }

    fn uploaded_digest(&self, key: &str) -> Result<Option<String>, String> {
        self.ledger
            .query_row("SELECT digest FROM uploaded WHERE key = ?1", params![key], |row| row.get(0))
            .optional()
            .map_err(display)
    }

    fn is_finished(&self, key: &str) -> Result<bool, String> {
        self.ledger
            .query_row("SELECT 1 FROM finished WHERE key = ?1", params![key], |_| Ok(()))
            .optional()
            .map(|row| row.is_some())
            .map_err(display)
    }

}

/// Orders level references the way people number them: a2 before a10.
fn natural_key(reference: &str) -> Vec<(u8, u64, String)> {
    let mut parts = Vec::new();
    let mut digits = String::new();
    let mut text = String::new();
    for character in reference.chars() {
        if character.is_ascii_digit() {
            if !text.is_empty() {
                parts.push((1, 0, std::mem::take(&mut text)));
            }
            digits.push(character);
        } else {
            if !digits.is_empty() {
                parts.push((0, digits.parse().unwrap_or(u64::MAX), String::new()));
                digits.clear();
            }
            text.push(character.to_ascii_lowercase());
        }
    }
    if !text.is_empty() {
        parts.push((1, 0, text));
    }
    if !digits.is_empty() {
        parts.push((0, digits.parse().unwrap_or(u64::MAX), String::new()));
    }
    parts
}

/// `(attempt id, closed)` for every attempt in a catalog's groups.
fn attempts(groups: Option<&Value>) -> Vec<(u64, bool)> {
    groups
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|group| group.get("attempts").and_then(Value::as_array))
        .flatten()
        .filter_map(|attempt| {
            Some((
                attempt.get("id")?.as_u64()?,
                attempt.get("status").and_then(Value::as_str) != Some("running"),
            ))
        })
        .collect()
}

/// A missing body is not an error for a publisher; other failures are.
fn optional(payload: Payload) -> Result<Option<Vec<u8>>, String> {
    match payload {
        Ok(body) => Ok(Some(body)),
        Err((404, _)) => Ok(None),
        Err((status, error)) => Err(format!("projection failed ({status}): {error}")),
    }
}

fn gzip(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).map_err(display)?;
    encoder.finish().map_err(display)
}

fn hostname() -> String {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn display(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// Writes every upload and message into a directory. Used for verification.
pub struct DirectorySink {
    root: PathBuf,
    messages: u64,
}

impl DirectorySink {
    pub fn new(root: PathBuf) -> Self {
        Self { root, messages: 0 }
    }

    fn message(&mut self, kind: &str, value: &Value) -> Result<(), String> {
        self.messages += 1;
        let path = self.root.join("messages").join(format!("{:08}-{kind}.json", self.messages));
        fs::create_dir_all(path.parent().expect("message directory")).map_err(display)?;
        fs::write(path, serde_json::to_vec(value).map_err(display)?).map_err(display)
    }
}

impl Sink for DirectorySink {
    fn put(&mut self, uploads: &[Upload]) -> Result<(), String> {
        for upload in uploads {
            let path = self.root.join("objects").join(&upload.key);
            fs::create_dir_all(path.parent().expect("object directory")).map_err(display)?;
            fs::write(path, &upload.body).map_err(display)?;
        }
        Ok(())
    }

    fn runs(&mut self, message: &Value) -> Result<(), String> {
        self.message("runs", message)
    }

    fn heartbeat(&mut self, message: &Value) -> Result<(), String> {
        self.message("heartbeat", message)
    }
}

/// Sends uploads and messages to the Cloudflare ingest endpoint.
pub struct HttpSink {
    endpoint: String,
    token: String,
    agent: ureq::Agent,
}

impl HttpSink {
    pub fn new(endpoint: &str, token: String) -> Self {
        Self {
            endpoint: endpoint.trim_end_matches('/').to_owned(),
            token,
            agent: ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(120))
                .build(),
        }
    }

    fn post(&self, path: &str, content_type: &str, gzipped: bool, body: &[u8]) -> Result<(), String> {
        let mut request = self
            .agent
            .post(&format!("{}{path}", self.endpoint))
            .set("authorization", &format!("Bearer {}", self.token))
            .set("content-type", content_type);
        if gzipped {
            request = request.set("x-body-encoding", "gzip");
        }
        match request.send_bytes(body)
        {
            Ok(_) => Ok(()),
            Err(ureq::Error::Status(status, response)) => Err(format!(
                "ingest {path} returned {status}: {}",
                response.into_string().unwrap_or_default()
            )),
            Err(error) => Err(format!("ingest {path} failed: {error}")),
        }
    }
}

impl Sink for HttpSink {
    /// Frame: u32 big-endian manifest length, manifest JSON, then every body in
    /// manifest order.
    fn put(&mut self, uploads: &[Upload]) -> Result<(), String> {
        let manifest = uploads
            .iter()
            .map(|upload| {
                json!({
                    "key": upload.key,
                    "bytes": upload.body.len(),
                    "content_type": upload.content_type,
                    "encoding": upload.encoding,
                    "cache_control": upload.cache_control,
                })
            })
            .collect::<Vec<_>>();
        let manifest = serde_json::to_vec(&manifest).map_err(display)?;
        let mut frame = Vec::with_capacity(
            4 + manifest.len() + uploads.iter().map(|upload| upload.body.len()).sum::<usize>(),
        );
        frame.extend_from_slice(&(manifest.len() as u32).to_be_bytes());
        frame.extend_from_slice(&manifest);
        for upload in uploads {
            frame.extend_from_slice(&upload.body);
        }
        self.post("/api/ingest/objects", "application/octet-stream", false, &frame)
    }

    fn runs(&mut self, message: &Value) -> Result<(), String> {
        self.post("/api/ingest/runs", JSON, true, &gzip(&serde_json::to_vec(message).map_err(display)?)?)
    }

    fn heartbeat(&mut self, message: &Value) -> Result<(), String> {
        self.post("/api/ingest/heartbeat", JSON, true, &gzip(&serde_json::to_vec(message).map_err(display)?)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Default)]
    struct Recorded {
        keys: Vec<String>,
        runs: Vec<Value>,
        heartbeats: usize,
        fail_puts: bool,
    }

    #[derive(Clone, Default)]
    struct RecordingSink(Rc<RefCell<Recorded>>);

    impl Sink for RecordingSink {
        fn put(&mut self, uploads: &[Upload]) -> Result<(), String> {
            let mut recorded = self.0.borrow_mut();
            if recorded.fail_puts {
                return Err("remote unavailable".into());
            }
            recorded.keys.extend(uploads.iter().map(|upload| upload.key.clone()));
            Ok(())
        }
        fn runs(&mut self, message: &Value) -> Result<(), String> {
            self.0.borrow_mut().runs.push(message.clone());
            Ok(())
        }
        fn heartbeat(&mut self, _message: &Value) -> Result<(), String> {
            self.0.borrow_mut().heartbeats += 1;
            Ok(())
        }
    }

    fn fixture(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("publisher-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let chain = root.join(".harbor/run-journals/run-a");
        let objects = chain.join("objects");
        fs::create_dir_all(&objects).unwrap();
        let state = storage::store_object(
            &objects,
            br#"{"campaign":{"score":1,"max_score":305,"complete":false},"level":{"reference":"l1","title":"One"}}"#,
        )
        .unwrap();
        let rows = [
            json!({"sequence":1,"source":"runtime","type":"segment_registered","segment_id":"seg-1","payload":{"task":"3720/sokoban","model":"test-model"}}),
            json!({"sequence":2,"source":"game","type":"score_changed","effective_elapsed_ms":10,"payload":{"action":{"command":"move"},"score":1,"score_delta":1,"state_snapshot":state}}),
            json!({"sequence":3,"source":"runtime","type":"segment_finished","payload":{"disposition":"agent_stopped"}}),
        ];
        fs::write(
            chain.join("journal.jsonl"),
            rows.iter().map(|row| format!("{row}\n")).collect::<String>(),
        )
        .unwrap();
        root
    }

    #[test]
    fn first_cycle_publishes_bodies_then_summaries_and_later_cycles_only_changes() {
        let root = fixture("cycle");
        let sink = RecordingSink::default();
        let mut publisher = Publisher::open(&root, &root.join("state"), sink.clone()).unwrap();
        assert!(publisher.cycle().unwrap());
        {
            let recorded = sink.0.borrow();
            assert!(recorded.keys.contains(&"pub/runs/run-a/detail.json".to_owned()));
            assert!(recorded.keys.contains(&"raw/run-a/journal.jsonl".to_owned()));
            assert!(recorded.keys.iter().any(|key| key.starts_with("raw/run-a/bundles/")));
            assert!(!recorded.keys.iter().any(|key| key.starts_with("raw/run-a/objects/")));
            assert!(recorded.keys.contains(&"pub/games/sokoban/levels.json".to_owned()));
            assert_eq!(recorded.runs.len(), 1);
            assert_eq!(recorded.runs[0]["order"], json!(["run-a"]));
            assert_eq!(recorded.runs[0]["runs"][0]["score"], 1);
        }
        let uploaded = sink.0.borrow().keys.len();
        assert!(!publisher.cycle().unwrap(), "unchanged authority must not republish");
        assert_eq!(sink.0.borrow().keys.len(), uploaded);
        assert_eq!(sink.0.borrow().runs.len(), 1);

        // A restarted publisher reuses its ledger: only summaries are re-sent.
        drop(publisher);
        let mut restarted = Publisher::open(&root, &root.join("state"), sink.clone()).unwrap();
        restarted.cycle().unwrap();
        assert_eq!(sink.0.borrow().keys.len(), uploaded);
        assert_eq!(sink.0.borrow().runs.len(), 2);
        restarted.heartbeat_if_due().unwrap();
        restarted.heartbeat_if_due().unwrap();
        assert_eq!(sink.0.borrow().heartbeats, 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejected_batches_are_not_recorded_and_are_resent() {
        let root = fixture("retry");
        let sink = RecordingSink::default();
        sink.0.borrow_mut().fail_puts = true;
        let mut publisher = Publisher::open(&root, &root.join("state"), sink.clone()).unwrap();
        assert!(publisher.cycle().is_err());
        assert!(sink.0.borrow().runs.is_empty(), "summaries must wait for their bodies");
        sink.0.borrow_mut().fail_puts = false;
        assert!(publisher.cycle().unwrap());
        assert!(sink.0.borrow().keys.contains(&"pub/runs/run-a/detail.json".to_owned()));
        assert_eq!(sink.0.borrow().runs.len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn level_references_sort_naturally() {
        let mut references = vec!["a10", "a2", "b1", "microban-010", "microban-009", "a1"];
        references.sort_by_key(|reference| natural_key(reference));
        assert_eq!(references, vec!["a1", "a2", "a10", "b1", "microban-009", "microban-010"]);
    }
}
