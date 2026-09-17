//! Durable storage, independent of the HTTP content encoding and game schema.
//! New objects use a hash of the decoded bytes and zstd. Legacy gzip IDs remain
//! valid aliases during migration; journal envelopes are never rewritten.
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);
pub const OBJECT_SCHEMA: &str = "benchmark-object-v2";
pub const DEFAULT_CHUNK_BYTES: usize = 16 * 1024 * 1024;
pub const JOURNAL_INDEX: &str = "journal-index.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JournalChunk {
    pub file: String,
    pub first_sequence: u64,
    pub last_sequence: u64,
    pub records: u64,
    pub uncompressed_bytes: u64,
    pub compressed_bytes: u64,
    pub content_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JournalIndex {
    pub schema: String,
    pub chunk_bytes: usize,
    pub chunks: Vec<JournalChunk>,
}

pub fn journal_index(chain: &Path) -> Result<JournalIndex, String> {
    let path = chain.join(JOURNAL_INDEX);
    if !path.exists() {
        return Ok(JournalIndex {
            schema: "benchmark-journal-index-v2".into(),
            chunk_bytes: DEFAULT_CHUNK_BYTES,
            chunks: Vec::new(),
        });
    }
    let index: JournalIndex =
        serde_json::from_reader(File::open(path).map_err(error)?).map_err(error)?;
    if index.schema != "benchmark-journal-index-v2"
        || index.chunk_bytes == 0
        || index.chunk_bytes > 32 * 1024 * 1024
    {
        return Err("unsupported journal index schema".into());
    }
    let mut previous = 0;
    for chunk in &index.chunks {
        if !chunk.file.starts_with("history/")
            || chunk.file.contains("..")
            || chunk.file.matches('/').count() != 1
            || chunk.first_sequence <= previous
            || chunk.first_sequence > chunk.last_sequence
            || chunk.uncompressed_bytes > index.chunk_bytes as u64
        {
            return Err("invalid journal chunk index".into());
        }
        previous = chunk.last_sequence;
    }
    Ok(index)
}

/// Immutable segments first, then the append-only hot tail. No segment is
/// expanded to disk or retained after its reader has advanced to the next one.
pub fn journal_sources(chain: &Path) -> Result<Vec<PathBuf>, String> {
    let index = journal_index(chain)?;
    let mut sources = index
        .chunks
        .iter()
        .map(|chunk| chain.join(&chunk.file))
        .collect::<Vec<_>>();
    let tail = chain.join("journal.jsonl");
    if tail.is_file() {
        sources.push(tail);
    } else if sources.is_empty() && chain.join("journal.jsonl.zst").is_file() {
        sources.push(chain.join("journal.jsonl.zst"));
    }
    Ok(sources)
}

pub fn journal_reader(chain: &Path) -> Result<Box<dyn Read>, String> {
    let paths = journal_sources(chain)?;
    Ok(Box::new(JournalReader {
        paths: paths.into_iter(),
        current: None,
        buffer: Vec::new(),
        offset: 0,
        previous_sequence: 0,
    }))
}

struct JournalReader {
    paths: std::vec::IntoIter<PathBuf>,
    current: Option<BufReader<Box<dyn Read>>>,
    buffer: Vec<u8>,
    offset: usize,
    previous_sequence: u64,
}

impl Read for JournalReader {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        while self.offset == self.buffer.len() {
            self.buffer.clear();
            self.offset = 0;
            if self.current.is_none() {
                let Some(path) = self.paths.next() else {
                    return Ok(0);
                };
                self.current = Some(BufReader::new(
                    reader(&path).map_err(std::io::Error::other)?,
                ));
            }
            let count = self
                .current
                .as_mut()
                .unwrap()
                .take((32 * 1024 * 1024 + 1) as u64)
                .read_until(b'\n', &mut self.buffer)?;
            if count == 0 {
                self.current = None;
                continue;
            }
            if count > 32 * 1024 * 1024 {
                return Err(std::io::Error::other("journal record exceeds 32 MiB"));
            }
            if !self.buffer.ends_with(b"\n") {
                return Err(std::io::Error::other("incomplete journal record"));
            }
            if self.buffer.iter().all(u8::is_ascii_whitespace) {
                self.buffer.clear();
                continue;
            }
            let row: Value = serde_json::from_slice(&self.buffer).map_err(std::io::Error::other)?;
            let sequence = row
                .get("sequence")
                .and_then(Value::as_u64)
                .ok_or_else(|| std::io::Error::other("missing journal sequence"))?;
            // A crash after publishing the index but before retiring the old
            // tail can leave the sealed prefix in both places. Skip it once.
            if sequence <= self.previous_sequence {
                self.buffer.clear();
                continue;
            }
            self.previous_sequence = sequence;
        }
        let count = output.len().min(self.buffer.len() - self.offset);
        output[..count].copy_from_slice(&self.buffer[self.offset..self.offset + count]);
        self.offset += count;
        Ok(count)
    }
}

pub fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("missing parent directory")?;
    fs::create_dir_all(parent).map_err(error)?;
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".{}.{}.{sequence}.tmp",
        path.file_name().unwrap().to_string_lossy(),
        std::process::id()
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(error)?;
        file.write_all(bytes).map_err(error)?;
        file.sync_all().map_err(error)?;
        fs::rename(&temporary, path).map_err(error)?;
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(error)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

/// A legacy filename may resolve to its migrated zstd sibling.
pub fn resolve(path: &Path) -> PathBuf {
    if path.extension().is_some_and(|extension| extension == "gz") {
        let zstd = path.with_extension("zst");
        if zstd.is_file() {
            return zstd;
        }
    }
    path.to_path_buf()
}

pub fn reader(path: &Path) -> Result<Box<dyn Read>, String> {
    let path = resolve(path);
    let file = File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    match path.extension().and_then(|value| value.to_str()) {
        Some("zst") => Ok(Box::new(
            zstd::stream::read::Decoder::new(file).map_err(error)?,
        )),
        Some("gz") => Ok(Box::new(GzDecoder::new(file))),
        _ => Ok(Box::new(file)),
    }
}

pub fn read(path: &Path) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader(path)?.read_to_end(&mut bytes).map_err(error)?;
    Ok(bytes)
}

pub fn read_json(path: &Path) -> Result<Value, String> {
    serde_json::from_reader(reader(path)?).map_err(error)
}

pub fn store_object(directory: &Path, decoded: &[u8]) -> Result<Value, String> {
    let hash = digest(decoded);
    let destination = directory.join(format!("{hash}.json.zst"));
    let encoded = zstd::stream::encode_all(decoded, 9).map_err(error)?;
    if !destination.is_file() {
        atomic_write(&destination, &encoded)?;
    }
    Ok(json!({
        "schema": OBJECT_SCHEMA,
        "object": hash,
        "encoding": "zstd",
        "media_type": "application/json",
        "uncompressed_bytes": decoded.len(),
        "compressed_bytes": encoded.len(),
        "content_sha256": hash,
    }))
}

pub fn object_bytes(descriptor: &Value, directory: &Path) -> Result<Option<Vec<u8>>, String> {
    let Some(id) = descriptor.get("object").and_then(Value::as_str) else {
        return Ok(None);
    };
    if id.len() != 64 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("invalid observer object id".into());
    }
    let path = resolve(&directory.join(format!("{id}.json.gz")));
    let bytes = read(&path)?;
    if descriptor
        .get("uncompressed_bytes")
        .and_then(Value::as_u64)
        .is_some_and(|expected| expected != bytes.len() as u64)
    {
        return Err(format!("observer object {id} length mismatch"));
    }
    if descriptor
        .get("content_sha256")
        .and_then(Value::as_str)
        .is_some_and(|expected| expected != digest(&bytes))
    {
        return Err(format!("observer object {id} checksum mismatch"));
    }
    Ok(Some(bytes))
}

pub fn journal_source(chain: &Path) -> PathBuf {
    let plain = chain.join("journal.jsonl");
    if plain.is_file() {
        plain
    } else if chain.join(JOURNAL_INDEX).is_file() {
        chain.join(JOURNAL_INDEX)
    } else {
        chain.join("journal.jsonl.zst")
    }
}

/// Resume the exact authority, retaining its compressed recovery copy until
/// the next verified archive. The caller must own the chain's writer lock.
pub fn restore_journal(chain: &Path) -> Result<PathBuf, String> {
    let plain = chain.join("journal.jsonl");
    if chain.join(JOURNAL_INDEX).is_file() {
        return Ok(plain);
    }
    if !plain.exists() && chain.join("journal.jsonl.zst").is_file() {
        let bytes = read(&chain.join("journal.jsonl.zst"))?;
        validate_journal(&bytes)?;
        atomic_write(&plain, &bytes)?;
    }
    Ok(plain)
}

pub fn validate_journal(bytes: &[u8]) -> Result<(), String> {
    if !bytes.is_empty() && !bytes.ends_with(b"\n") {
        return Err("journal has an incomplete final record".into());
    }
    let mut previous = 0;
    for line in BufReader::new(bytes).lines() {
        let line = line.map_err(error)?;
        if line.trim().is_empty() {
            continue;
        }
        let row: Value = serde_json::from_str(&line).map_err(error)?;
        if row.get("schema").and_then(Value::as_str) != Some(crate::RUN_EVENT_SCHEMA) {
            return Err("unsupported run event schema".into());
        }
        let sequence = row
            .get("sequence")
            .and_then(Value::as_u64)
            .ok_or("missing run sequence")?;
        if sequence <= previous {
            return Err("run sequence is not increasing".into());
        }
        previous = sequence;
    }
    Ok(())
}

fn error(value: impl std::fmt::Display) -> String {
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zstd_objects_roundtrip_and_detect_corruption() {
        let path = std::env::temp_dir().join(format!("observer-zstd-{}", std::process::id()));
        let descriptor = store_object(&path, br#"{"state":[1,2,3]}"#).unwrap();
        assert_eq!(
            object_bytes(&descriptor, &path).unwrap().unwrap(),
            br#"{"state":[1,2,3]}"#
        );
        let mut invalid = descriptor.clone();
        invalid["content_sha256"] = json!("bad");
        assert!(
            object_bytes(&invalid, &path)
                .unwrap_err()
                .contains("checksum")
        );
        fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn incomplete_and_reordered_journals_are_rejected() {
        assert!(validate_journal(b"{\"sequence\":1}").is_err());
        let row = json!({"schema":crate::RUN_EVENT_SCHEMA,"sequence":1}).to_string();
        assert!(validate_journal(format!("{row}\n{row}\n").as_bytes()).is_err());
        assert!(validate_journal(format!("{row}\n").as_bytes()).is_ok());
    }
}
