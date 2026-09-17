//! Disposable SQLite query index over immutable zstd segments and one hot tail.
//! It stores descriptors, never expands the complete run in memory. The index
//! can be removed and rebuilt; the recorder never writes to it.
use crate::{contract, storage};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const CATALOG_PAGE: usize = 200;
pub const REPLAY_BYTES: u64 = 1024 * 1024;
const INDEX_VERSION: &str = "6";

pub struct RunIndex {
    connection: Connection,
    pub objects: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexedAttempt {
    pub id: u64,
    pub start: u64,
    pub end: u64,
    pub context: contract::Context,
    pub score: i64,
    pub successful: bool,
    pub closed: bool,
}

#[derive(Default, Serialize, Deserialize)]
struct Cursor {
    sequence: u64,
    task: String,
    #[serde(default)]
    visible: bool,
    previous_score: Option<i64>,
    #[serde(default)]
    context: Option<contract::Context>,
    attempt: Option<IndexedAttempt>,
}

impl RunIndex {
    pub fn open(chain: &Path, cache: &Path) -> Result<Self, String> {
        fs::create_dir_all(cache).map_err(error)?;
        let connection =
            Connection::open(cache.join(format!("projection-v{INDEX_VERSION}.sqlite")))
                .map_err(error)?;
        connection
            .busy_timeout(Duration::from_secs(30))
            .map_err(error)?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA cache_size=-4096; PRAGMA temp_store=FILE; PRAGMA mmap_size=0;
            CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS inputs (name TEXT PRIMARY KEY, signature TEXT NOT NULL, offset INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS events (sequence INTEGER PRIMARY KEY, source TEXT NOT NULL, kind TEXT NOT NULL, timestamp INTEGER, row TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS event_source ON events(source,sequence);
            CREATE INDEX IF NOT EXISTS event_kind ON events(kind,sequence);
            CREATE TABLE IF NOT EXISTS scores (sequence INTEGER PRIMARY KEY, timestamp INTEGER, elapsed INTEGER NOT NULL, score INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS attempts (id INTEGER PRIMARY KEY, row TEXT NOT NULL);").map_err(error)?;
        let version: Option<String> = connection
            .query_row("SELECT value FROM meta WHERE key='version'", [], |row| {
                row.get(0)
            })
            .optional()
            .map_err(error)?;
        if version.as_deref() != Some(INDEX_VERSION) {
            connection.execute_batch("DELETE FROM events; DELETE FROM scores; DELETE FROM attempts; DELETE FROM inputs; DELETE FROM meta;").map_err(error)?;
            connection
                .execute("INSERT INTO meta VALUES('version',?1)", [INDEX_VERSION])
                .map_err(error)?;
        }
        let mut index = Self {
            connection,
            objects: chain.join("objects"),
        };
        index.sync(chain)?;
        Ok(index)
    }

    fn sync(&mut self, chain: &Path) -> Result<(), String> {
        for path in storage::journal_sources(chain)? {
            let name = path
                .strip_prefix(chain)
                .map_err(error)?
                .to_string_lossy()
                .into_owned();
            let metadata = fs::metadata(&path).map_err(error)?;
            let signature = format!("{}:{:?}", metadata.len(), metadata.modified().ok());
            let previous: Option<(String, u64)> = self
                .connection
                .query_row(
                    "SELECT signature,offset FROM inputs WHERE name=?1",
                    [&name],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(error)?;
            if previous
                .as_ref()
                .is_some_and(|(value, _)| value == &signature)
            {
                continue;
            }
            let plain = path
                .extension()
                .is_some_and(|extension| extension == "jsonl");
            let offset = if plain {
                previous
                    .as_ref()
                    .map(|(_, offset)| *offset)
                    .filter(|offset| *offset <= metadata.len())
                    .unwrap_or(0)
            } else {
                0
            };
            let source: Box<dyn Read> = if plain {
                let mut file = File::open(&path).map_err(error)?;
                file.seek(SeekFrom::Start(offset)).map_err(error)?;
                Box::new(file.take(metadata.len() - offset))
            } else {
                storage::reader(&path)?
            };
            let mut reader = BufReader::new(source);
            let mut line = Vec::new();
            let mut committed = offset;
            let transaction = self.connection.transaction().map_err(error)?;
            let saved: Option<String> = transaction
                .query_row("SELECT value FROM meta WHERE key='cursor'", [], |row| {
                    row.get(0)
                })
                .optional()
                .map_err(error)?;
            let mut cursor = saved
                .map(|value| serde_json::from_str::<Cursor>(&value).map_err(error))
                .transpose()?
                .unwrap_or_default();
            loop {
                line.clear();
                let count = reader
                    .by_ref()
                    .take(32 * 1024 * 1024 + 1)
                    .read_until(b'\n', &mut line)
                    .map_err(error)?;
                if count == 0 {
                    break;
                }
                if count > 32 * 1024 * 1024 {
                    return Err("journal record exceeds memory limit".into());
                }
                if !line.ends_with(b"\n") {
                    if plain {
                        break;
                    } else {
                        return Err("incomplete archived record".into());
                    }
                }
                committed += count as u64;
                if line.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                let mut row: Value = serde_json::from_slice(&line).map_err(error)?;
                let sequence = row
                    .get("sequence")
                    .and_then(Value::as_u64)
                    .ok_or("missing journal sequence")?;
                if sequence <= cursor.sequence {
                    continue;
                }
                cursor.sequence = sequence;
                let source = row
                    .get("source")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let kind = row
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                if kind == "segment_registered" {
                    cursor.task = row
                        .pointer("/payload/task")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .rsplit('/')
                        .next()
                        .unwrap_or_default()
                        .into();
                    cursor.visible = row
                        .pointer("/payload/model")
                        .and_then(Value::as_str)
                        .is_some_and(|model| !model.is_empty());
                }
                if source == "agent"
                    && !matches!(kind.as_str(), "agent_message" | "experience_updated")
                {
                    continue;
                }
                if source == "game" {
                    if !cursor.visible {
                        continue;
                    }
                    let payload = row.get_mut("payload").ok_or("missing game payload")?;
                    if payload.pointer("/context/schema").and_then(Value::as_str)
                        != Some(contract::CONTEXT_SCHEMA)
                    {
                        let decoded =
                            storage::object_bytes(&payload["state_snapshot"], &self.objects)?
                                .map(|bytes| serde_json::from_slice::<Value>(&bytes).map_err(error))
                                .transpose()?;
                        let state = payload
                            .get("state")
                            .or(decoded.as_ref())
                            .unwrap_or(&Value::Null);
                        let mut context = contract::context(&cursor.task, payload, state);
                        if context.reference == "session"
                            && let Some(previous) = &cursor.context
                        {
                            context = previous.clone();
                        }
                        payload["context"] = json!(context);
                    }
                    if payload.get("action").is_none() && payload.get("command").is_some() {
                        payload["action"] = json!({"command":payload["command"]});
                    }
                    if let Some(assets) = payload.get("assets").and_then(Value::as_object) {
                        let previous: Option<String> = transaction
                            .query_row("SELECT value FROM meta WHERE key='assets'", [], |row| {
                                row.get(0)
                            })
                            .optional()
                            .map_err(error)?;
                        let mut merged = previous
                            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                            .unwrap_or_else(|| json!({}));
                        merged.as_object_mut().unwrap().extend(assets.clone());
                        transaction
                            .execute(
                                "INSERT OR REPLACE INTO meta VALUES('assets',?1)",
                                [merged.to_string()],
                            )
                            .map_err(error)?;
                    }
                    let score = payload
                        .get("score")
                        .and_then(Value::as_i64)
                        .unwrap_or(cursor.previous_score.unwrap_or(0));
                    if cursor.previous_score != Some(score) {
                        transaction
                            .execute(
                                "INSERT OR REPLACE INTO scores VALUES(?1,?2,?3,?4)",
                                params![
                                    sequence,
                                    row["source_timestamp_ms"].as_u64(),
                                    row["effective_elapsed_ms"].as_u64().unwrap_or(0),
                                    score
                                ],
                            )
                            .map_err(error)?;
                        cursor.previous_score = Some(score);
                    }
                    update_attempt(&transaction, &mut cursor, &row)?;
                }
                transaction
                    .execute(
                        "INSERT OR REPLACE INTO events VALUES(?1,?2,?3,?4,?5)",
                        params![
                            sequence,
                            source,
                            kind,
                            row["source_timestamp_ms"].as_u64(),
                            row.to_string()
                        ],
                    )
                    .map_err(error)?;
            }
            transaction
                .execute(
                    "INSERT OR REPLACE INTO meta VALUES('cursor',?1)",
                    [serde_json::to_string(&cursor).map_err(error)?],
                )
                .map_err(error)?;
            // Do not mark an incomplete tail as fully indexed. Its next append
            // resumes at the last newline, even across gateway restarts.
            let signature = if plain && committed < metadata.len() {
                String::new()
            } else {
                signature
            };
            transaction
                .execute(
                    "INSERT OR REPLACE INTO inputs VALUES(?1,?2,?3)",
                    params![name, signature, committed],
                )
                .map_err(error)?;
            transaction.commit().map_err(error)?;
        }
        Ok(())
    }

    pub fn summary_rows(&self) -> Result<Vec<Value>, String> {
        let mut statement=self.connection.prepare("SELECT row FROM events WHERE sequence IN (
            SELECT sequence FROM events WHERE source='runtime' ORDER BY sequence DESC LIMIT 64
        ) OR sequence IN (SELECT sequence FROM events WHERE source='game' ORDER BY sequence DESC LIMIT 1)
          OR sequence IN (SELECT sequence FROM events WHERE kind='experience_updated' ORDER BY sequence DESC LIMIT 1)
          OR sequence IN (SELECT sequence FROM events WHERE kind='agent_message' ORDER BY sequence DESC LIMIT 20)
          OR sequence=(SELECT sequence FROM events WHERE source='game' AND (json_type(row,'$.payload.state')='object' OR json_type(row,'$.payload.state_snapshot')='object') ORDER BY sequence DESC LIMIT 1)
          OR sequence=(SELECT MIN(sequence) FROM events) ORDER BY sequence").map_err(error)?;
        let mut rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(error)?
            .map(|row| serde_json::from_str(&row.map_err(error)?).map_err(error))
            .collect::<Result<Vec<Value>, String>>()?;
        let assets: Option<String> = self
            .connection
            .query_row("SELECT value FROM meta WHERE key='assets'", [], |row| {
                row.get(0)
            })
            .optional()
            .map_err(error)?;
        if let Some(assets) = assets
            && let Some(latest) = rows.iter_mut().rev().find(|row| row["source"] == "game")
        {
            latest["payload"]["assets"] = serde_json::from_str(&assets).map_err(error)?;
        }
        Ok(rows)
    }

    pub fn history(&self) -> Result<(Vec<Value>, u64), String> {
        let count: u64 = self
            .connection
            .query_row("SELECT count(*) FROM scores", [], |row| row.get(0))
            .map_err(error)?;
        let stride = count.div_ceil(2048).max(1);
        let mut statement=self.connection.prepare("SELECT timestamp,elapsed,score FROM (SELECT *,row_number() OVER (ORDER BY sequence) AS position FROM scores) WHERE position=1 OR position=?1 OR position%?2=0 ORDER BY sequence").map_err(error)?;
        let points=statement.query_map(params![count,stride],|row|Ok(json!({"timestamp_ms":row.get::<_,Option<u64>>(0)?,"elapsed_ms":row.get::<_,u64>(1)?,"score":row.get::<_,i64>(2)?}))).map_err(error)?.collect::<Result<Vec<_>,_>>().map_err(error)?;
        Ok((points, count))
    }

    pub fn catalog(&self, before: Option<u64>) -> Result<(Vec<IndexedAttempt>, bool), String> {
        let mut statement = self
            .connection
            .prepare("SELECT row FROM attempts WHERE id<?1 ORDER BY id DESC LIMIT ?2")
            .map_err(error)?;
        let mut entries = statement
            .query_map(
                params![before.unwrap_or(i64::MAX as u64), CATALOG_PAGE + 1],
                |row| row.get::<_, String>(0),
            )
            .map_err(error)?
            .map(|row| serde_json::from_str(&row.map_err(error)?).map_err(error))
            .collect::<Result<Vec<IndexedAttempt>, String>>()?;
        let more = entries.len() > CATALOG_PAGE;
        entries.truncate(CATALOG_PAGE);
        entries.reverse();
        Ok((entries, more))
    }

    pub fn attempt(&self, id: u64) -> Result<Option<IndexedAttempt>, String> {
        self.connection
            .query_row("SELECT row FROM attempts WHERE id=?1", [id], |row| {
                row.get::<_, String>(0)
            })
            .optional()
            .map_err(error)?
            .map(|text| serde_json::from_str(&text).map_err(error))
            .transpose()
    }

    pub fn game_window(
        &self,
        start: u64,
        end: u64,
        after: Option<u64>,
    ) -> Result<(Vec<Value>, Option<u64>), String> {
        let after = after.unwrap_or(start.saturating_sub(1));
        let mut statement=self.connection.prepare("SELECT row FROM events WHERE source='game' AND sequence>=?1 AND sequence<=?2 AND sequence>?3 ORDER BY sequence LIMIT 129").map_err(error)?;
        let mut output = Vec::new();
        let mut bytes = 0;
        let mut instructions = 0;
        let mut next = None;
        for text in statement
            .query_map(params![start, end, after], |row| row.get::<_, String>(0))
            .map_err(error)?
        {
            let text = text.map_err(error)?;
            let row: Value = serde_json::from_str(&text).map_err(error)?;
            let cost = row
                .pointer("/payload/state_snapshot/uncompressed_bytes")
                .and_then(Value::as_u64)
                .unwrap_or(text.len() as u64)
                + row
                    .pointer("/payload/instruction_trace/uncompressed_bytes")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
            let count = row
                .pointer("/payload/instruction_trace/count")
                .and_then(Value::as_u64)
                .unwrap_or(1);
            if !output.is_empty()
                && (bytes + cost > REPLAY_BYTES
                    || output.len() >= 128
                    || instructions + count > 1024)
            {
                next = output
                    .last()
                    .and_then(|row: &Value| row["sequence"].as_u64());
                break;
            }
            if cost > 4 * 1024 * 1024 {
                return Err("individual replay operation exceeds the 4 MiB memory budget".into());
            }
            bytes += cost;
            instructions += count;
            output.push(row);
        }
        Ok((output, next))
    }

    pub fn anchor(&self, before: u64) -> Result<Option<Value>, String> {
        self.connection.query_row("SELECT row FROM events WHERE source='game' AND sequence<?1 AND (json_type(row,'$.payload.state')='object' OR json_type(row,'$.payload.state_snapshot')='object') ORDER BY sequence DESC LIMIT 1",[before],|row|row.get::<_,String>(0)).optional().map_err(error)?.map(|text|serde_json::from_str(&text).map_err(error)).transpose()
    }

    pub fn activity(&self, start: Option<u64>, end: Option<u64>) -> Result<Vec<Value>, String> {
        let mut statement=self.connection.prepare("SELECT timestamp,json_extract(row,'$.payload.text') FROM events WHERE kind='agent_message' AND timestamp>=?1 AND timestamp<=?2 ORDER BY sequence DESC LIMIT 100").map_err(error)?;
        statement.query_map(params![start.unwrap_or(0),end.unwrap_or(i64::MAX as u64)],|row|Ok(json!({"timestamp_ms":row.get::<_,Option<u64>>(0)?,"text":row.get::<_,String>(1)?}))).map_err(error)?.collect::<Result<Vec<_>,_>>().map_err(error)
    }
}

fn update_attempt(connection: &Connection, cursor: &mut Cursor, row: &Value) -> Result<(), String> {
    let sequence = row["sequence"].as_u64().unwrap();
    let payload = &row["payload"];
    let command = payload
        .pointer("/action/command")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let delta = payload
        .get("score_delta")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let mut context = contract::context(
        &cursor.task,
        payload,
        payload.get("state").unwrap_or(&Value::Null),
    );
    let next_context = payload
        .get("context_after")
        .map(|context| contract::context(&cursor.task, &json!({"context":context}), &Value::Null))
        .unwrap_or_else(|| context.clone());
    if cursor.task == "parabox-intro"
        && delta > 0
        && let Some(reference) = payload
            .get("solved_levels")
            .and_then(Value::as_array)
            .and_then(|levels| levels.last())
            .or_else(|| payload.get("selected_before"))
            .and_then(Value::as_str)
    {
        context.reference = reference.into();
        if let Some(previous) = &cursor.context
            && previous.reference == reference
        {
            context.title = previous.title.clone();
        }
    }
    cursor.context = Some(next_context);
    let reset = matches!(command.as_str(), "reset" | "restart")
        && payload.pointer("/result/ok").and_then(Value::as_bool) != Some(false);
    if cursor.attempt.as_ref().is_some_and(|attempt| {
        attempt.context.reference != context.reference || reset || command == "select"
    }) && let Some(mut attempt) = cursor.attempt.take()
    {
        attempt.closed = true;
        save_attempt(connection, &attempt)?;
    }
    if reset {
        return Ok(());
    }
    let meaningful = !matches!(
        command.as_str(),
        "" | "undo"
            | "redo"
            | "show"
            | "inspect"
            | "status"
            | "list"
            | "select"
            | "levels"
            | "submit"
    );
    if cursor.attempt.is_none() && meaningful && (!context.complete || delta > 0) {
        cursor.attempt = Some(IndexedAttempt {
            id: sequence,
            start: sequence,
            end: sequence,
            context: context.clone(),
            score: 0,
            successful: false,
            closed: false,
        });
    }
    if let Some(attempt) = cursor.attempt.as_mut() {
        attempt.end = sequence;
        attempt.score += delta;
        if (context.boundary == "score" && delta > 0)
            || (context.boundary == "episode" && context.complete)
        {
            attempt.closed = true;
            attempt.successful = attempt.score > 0;
        }
        save_attempt(connection, attempt)?;
        if attempt.closed {
            cursor.attempt = None;
        }
    }
    Ok(())
}
fn save_attempt(connection: &Connection, attempt: &IndexedAttempt) -> Result<(), String> {
    connection
        .execute(
            "INSERT OR REPLACE INTO attempts VALUES(?1,?2)",
            params![attempt.id, serde_json::to_string(attempt).map_err(error)?],
        )
        .map_err(error)?;
    Ok(())
}
fn error(value: impl std::fmt::Display) -> String {
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn row(sequence: u64, command: &str, level: &str, delta: i64) -> Value {
        json!({"schema":crate::RUN_EVENT_SCHEMA,"sequence":sequence,"source":"game","type":"game_event","source_timestamp_ms":sequence*100,"effective_elapsed_ms":sequence*10,
            "payload":{"action":{"command":command},"state":{"level":{"id":level,"title":level}},"score":delta,"score_delta":delta}})
    }
    fn setup(name: &str) -> (PathBuf, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("observer-index-{name}-{}", std::process::id()));
        let chain = root.join("chain");
        fs::create_dir_all(&chain).unwrap();
        let registration = json!({"schema":crate::RUN_EVENT_SCHEMA,"sequence":1,"source":"runtime","type":"segment_registered","payload":{"task":"sokoban","model":"test"}});
        fs::write(chain.join("journal.jsonl"), format!("{registration}\n")).unwrap();
        (root, chain)
    }
    fn append(chain: &Path, rows: &[Value]) {
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(chain.join("journal.jsonl"))
            .unwrap();
        for row in rows {
            writeln!(file, "{row}").unwrap();
        }
    }
    #[test]
    fn partial_tail_and_restart_do_not_lose_or_duplicate_events() {
        let (root, chain) = setup("tail");
        let complete = row(2, "move", "one", 0).to_string() + "\n";
        let split = complete.len() / 2;
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(chain.join("journal.jsonl"))
            .unwrap();
        file.write_all(&complete.as_bytes()[..split]).unwrap();
        {
            let index = RunIndex::open(&chain, &root.join("cache")).unwrap();
            assert!(index.catalog(None).unwrap().0.is_empty());
        }
        file.write_all(&complete.as_bytes()[split..]).unwrap();
        drop(file);
        {
            let index = RunIndex::open(&chain, &root.join("cache")).unwrap();
            assert_eq!(index.catalog(None).unwrap().0.len(), 1);
        }
        {
            let index = RunIndex::open(&chain, &root.join("cache")).unwrap();
            assert_eq!(index.game_window(1, 10, None).unwrap().0.len(), 1);
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn reset_and_level_switch_keep_failed_attempts_in_the_catalog() {
        let (root, chain) = setup("attempts");
        append(
            &chain,
            &[
                row(2, "move", "one", 0),
                row(3, "reset", "one", 0),
                row(4, "move", "one", 1),
                row(5, "select", "two", 0),
                row(6, "move", "two", 0),
            ],
        );
        let index = RunIndex::open(&chain, &root.join("cache")).unwrap();
        let (attempts, _) = index.catalog(None).unwrap();
        assert_eq!(attempts.len(), 3);
        assert!(attempts[0].closed);
        assert!(!attempts[0].successful);
        assert!(attempts[1].successful);
        assert_eq!(attempts[2].context.reference, "two");
        assert!(!attempts[2].closed);
        drop(index);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn shift_scores_stay_in_one_episode_and_terminal_reads_do_not_create_attempts() {
        let (root, chain) = setup("shift");
        let mut rows = vec![
            row(2, "start", "one", 0),
            row(3, "dispatch", "one", 40),
            row(4, "wait", "one", 60),
            row(5, "wait", "one", 0),
        ];
        for (i, row) in rows.iter_mut().enumerate() {
            row["payload"]["state"] =
                json!({"shift":{"id":"duty","status":if i>=2{"complete"}else{"running"}}});
        }
        append(&chain, &rows);
        let index = RunIndex::open(&chain, &root.join("cache")).unwrap();
        let (attempts, _) = index.catalog(None).unwrap();
        assert_eq!(attempts.len(), 1);
        assert_eq!(attempts[0].score, 100);
        assert!(attempts[0].successful);
        assert_eq!(attempts[0].context.kind, "shift");
        drop(index);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn replay_pages_obey_the_decoded_byte_budget() {
        let (root, chain) = setup("window");
        let mut rows = (2..6)
            .map(|sequence| row(sequence, "move", "one", 0))
            .collect::<Vec<_>>();
        for row in &mut rows {
            crate::contract::stamp(&mut row["payload"]);
            row["payload"]["state_snapshot"] = json!({"uncompressed_bytes":3*1024*1024});
        }
        append(&chain, &rows);
        let index = RunIndex::open(&chain, &root.join("cache")).unwrap();
        let (first, next) = index.game_window(2, 5, None).unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(next, Some(2));
        let (second, _) = index.game_window(2, 5, next).unwrap();
        assert_eq!(second[0]["sequence"], 3);
        drop(index);
        fs::remove_dir_all(root).unwrap();
    }
}
