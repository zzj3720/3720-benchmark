//! Verified, repeatable migration. Run beside the recorder on its host so
//! writer locks protect the entire check/compress/retire transaction.
use crate::storage;
use fs2::FileExt;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use walkdir::WalkDir;

pub fn migrate(root: &Path, apply: bool, retire: bool) -> Result<Value, String> {
    migrate_with_chunks(root, apply, retire, storage::DEFAULT_CHUNK_BYTES)
}

pub fn migrate_with_chunks(
    root: &Path,
    apply: bool,
    retire: bool,
    chunk_bytes: usize,
) -> Result<Value, String> {
    if chunk_bytes == 0 || chunk_bytes > 32 * 1024 * 1024 {
        return Err("invalid chunk size".into());
    }
    if retire && !apply {
        return Err("--retire requires --apply".into());
    }
    let harbor = root.join(".harbor");
    let mut records = Vec::new();
    let mut skipped = Vec::new();
    let mut object_files = 0_u64;
    let mut source_bytes = 0_u64;
    let mut zstd_bytes = 0_u64;
    let mut receipts = ReceiptLog::new(&harbor, apply, chunk_bytes);
    let archives = harbor.join("live-archive");
    if archives.is_dir() {
        for entry in WalkDir::new(&archives).follow_links(false).into_iter() {
            let entry = entry.map_err(error)?;
            if entry.file_type().is_file() && entry.path().extension().is_some_and(|x| x == "gz") {
                let record = convert(entry.path(), apply, retire, &mut receipts)?;
                object_files += 1;
                source_bytes += record["source_bytes"].as_u64().unwrap_or(0);
                zstd_bytes += record["zstd_bytes"].as_u64().unwrap_or(0);
            }
        }
    }
    let journals = harbor.join("run-journals");
    if journals.is_dir() {
        for entry in fs::read_dir(&journals).map_err(error)? {
            let chain = entry.map_err(error)?.path();
            if !chain.is_dir() {
                continue;
            }
            let lock = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(chain.join("writer.lock"))
                .map_err(error)?;
            if lock.try_lock_exclusive().is_err() {
                skipped.push(json!({"chain":chain.file_name().unwrap().to_string_lossy(),"reason":"active writer"}));
                continue;
            }
            let journal = chain.join("journal.jsonl");
            if journal.is_file() {
                // Only finalized segments are cold history. An orphan remains
                // available for investigation/continuation without migration.
                let mut last_lifecycle = String::new();
                for line in BufReader::new(File::open(&journal).map_err(error)?).lines() {
                    let line = line.map_err(error)?;
                    let row: Value = serde_json::from_str(&line).map_err(error)?;
                    if row.get("source").and_then(Value::as_str) == Some("runtime") {
                        last_lifecycle = row
                            .get("type")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .into();
                    }
                }
                if last_lifecycle != "segment_finished" {
                    skipped.push(json!({"chain":chain.file_name().unwrap().to_string_lossy(),"reason":"unsealed segment"}));
                    continue;
                }
                records.push(archive_journal(&chain, apply, retire, chunk_bytes)?);
            }
            let objects = chain.join("objects");
            if objects.is_dir() {
                for object in fs::read_dir(objects).map_err(error)? {
                    let object = object.map_err(error)?.path();
                    if object.is_file() && object.extension().is_some_and(|x| x == "gz") {
                        let record = convert(&object, apply, retire, &mut receipts)?;
                        object_files += 1;
                        source_bytes += record["source_bytes"].as_u64().unwrap_or(0);
                        zstd_bytes += record["zstd_bytes"].as_u64().unwrap_or(0);
                    }
                }
            }
        }
    }
    receipts.flush()?;
    let result = json!({"schema":"benchmark-archive-migration-v1","applied":apply,"retired_sources":retire,"chunk_bytes":chunk_bytes,"files":records,"object_files":object_files,"object_source_bytes":source_bytes,"object_zstd_bytes":zstd_bytes,"receipt_directory":if apply{Some(&receipts.directory)}else{None},"skipped":skipped});
    if apply {
        storage::atomic_write(
            &harbor.join("archive-migration.json"),
            &serde_json::to_vec_pretty(&result).map_err(error)?,
        )?;
    }
    Ok(result)
}

struct CountWriter {
    target: Box<dyn Write>,
    bytes: u64,
}
impl Write for CountWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let count = self.target.write(bytes)?;
        self.bytes += count as u64;
        Ok(count)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.target.flush()
    }
}
struct ChunkWriter {
    encoder: zstd::stream::write::Encoder<'static, CountWriter>,
    temporary: std::path::PathBuf,
    hash: Sha256,
    first: u64,
    last: u64,
    records: u64,
    bytes: u64,
}
impl ChunkWriter {
    fn new(chain: &Path, first: u64, apply: bool) -> Result<Self, String> {
        let temporary = chain.join(format!("history/.chunk-{first}-{}.tmp", std::process::id()));
        let target: Box<dyn Write> = if apply {
            fs::create_dir_all(temporary.parent().unwrap()).map_err(error)?;
            Box::new(File::create(&temporary).map_err(error)?)
        } else {
            Box::new(std::io::sink())
        };
        let mut encoder = zstd::stream::write::Encoder::new(CountWriter { target, bytes: 0 }, 9)
            .map_err(error)?;
        encoder.include_checksum(true).map_err(error)?;
        encoder.window_log(23).map_err(error)?;
        Ok(Self {
            encoder,
            temporary,
            hash: Sha256::new(),
            first,
            last: first,
            records: 0,
            bytes: 0,
        })
    }
    fn write(&mut self, line: &[u8], sequence: u64) -> Result<(), String> {
        self.encoder.write_all(line).map_err(error)?;
        self.hash.update(line);
        self.last = sequence;
        self.records += 1;
        self.bytes += line.len() as u64;
        Ok(())
    }
    fn finish(self, chain: &Path, apply: bool) -> Result<storage::JournalChunk, String> {
        let encoded = self.encoder.finish().map_err(error)?;
        let hash = self
            .hash
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let file = format!(
            "history/{:016}-{:016}-{}.jsonl.zst",
            self.first,
            self.last,
            &hash[..16]
        );
        if apply {
            File::open(&self.temporary)
                .and_then(|file| file.sync_all())
                .map_err(error)?;
            let mut reader =
                zstd::stream::read::Decoder::new(File::open(&self.temporary).map_err(error)?)
                    .map_err(error)?;
            let mut verified = Sha256::new();
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                let count = reader.read(&mut buffer).map_err(error)?;
                if count == 0 {
                    break;
                }
                verified.update(&buffer[..count]);
            }
            let actual = verified
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            if actual != hash {
                return Err("journal chunk checksum mismatch".into());
            }
            fs::rename(&self.temporary, chain.join(&file)).map_err(error)?;
            File::open(chain.join("history"))
                .and_then(|file| file.sync_all())
                .map_err(error)?;
        }
        Ok(storage::JournalChunk {
            file,
            first_sequence: self.first,
            last_sequence: self.last,
            records: self.records,
            uncompressed_bytes: self.bytes,
            compressed_bytes: encoded.bytes,
            content_sha256: hash,
        })
    }
}

/// Caller owns writer.lock and has finalized the segment.
pub fn chunk_size(chain: &Path) -> Result<usize, String> {
    let index = storage::journal_index(chain)?;
    let chunk_bytes = if index.chunks.is_empty() {
        let mib = std::env::var("BENCHMARK_HISTORY_CHUNK_MIB")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(16);
        if !matches!(mib, 16 | 32) {
            return Err("BENCHMARK_HISTORY_CHUNK_MIB must be 16 or 32".into());
        }
        mib * 1024 * 1024
    } else {
        index.chunk_bytes
    };
    Ok(chunk_bytes)
}

pub fn seal_history(chain: &Path) -> Result<Value, String> {
    if !chain.join("journal.jsonl").is_file() {
        return Ok(json!({"already_archived":true}));
    }
    archive_journal(chain, true, true, chunk_size(chain)?)
}

pub fn rotate_history(chain: &Path, chunk_bytes: usize) -> Result<Value, String> {
    archive_journal(chain, true, true, chunk_bytes)
}

fn archive_journal(
    chain: &Path,
    apply: bool,
    retire: bool,
    chunk_bytes: usize,
) -> Result<Value, String> {
    let source = chain.join("journal.jsonl");
    let mut index = storage::journal_index(chain)?;
    if !index.chunks.is_empty() && index.chunk_bytes != chunk_bytes {
        return Err("existing journal uses a different segment size".into());
    }
    index.chunk_bytes = chunk_bytes;
    let archived_through = index
        .chunks
        .last()
        .map(|chunk| chunk.last_sequence)
        .unwrap_or(0);
    let mut previous = 0;
    let mut chunk: Option<ChunkWriter> = None;
    let mut reader = BufReader::new(File::open(&source).map_err(error)?);
    let mut line = Vec::new();
    loop {
        line.clear();
        let count = reader
            .by_ref()
            .take(chunk_bytes as u64 + 1)
            .read_until(b'\n', &mut line)
            .map_err(error)?;
        if count == 0 {
            break;
        }
        if count > chunk_bytes {
            return Err(format!(
                "record exceeds {} MiB chunk limit",
                chunk_bytes / 1024 / 1024
            ));
        }
        if !line.ends_with(b"\n") {
            return Err("journal has an incomplete tail".into());
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let row: Value = serde_json::from_slice(&line).map_err(error)?;
        if row.get("schema").and_then(Value::as_str) != Some(crate::RUN_EVENT_SCHEMA) {
            return Err("unsupported journal schema".into());
        }
        let sequence = row
            .get("sequence")
            .and_then(Value::as_u64)
            .ok_or("missing journal sequence")?;
        if sequence <= previous {
            return Err("journal sequence is not increasing".into());
        }
        previous = sequence;
        if sequence <= archived_through {
            continue;
        }
        if chunk
            .as_ref()
            .is_some_and(|chunk| chunk.bytes + count as u64 > chunk_bytes as u64)
        {
            index
                .chunks
                .push(chunk.take().unwrap().finish(chain, apply)?);
        }
        if chunk.is_none() {
            chunk = Some(ChunkWriter::new(chain, sequence, apply)?);
        }
        chunk.as_mut().unwrap().write(&line, sequence)?;
    }
    if let Some(chunk) = chunk {
        index.chunks.push(chunk.finish(chain, apply)?);
    }
    if apply {
        storage::atomic_write(
            &chain.join(storage::JOURNAL_INDEX),
            &serde_json::to_vec_pretty(&index).map_err(error)?,
        )?;
        if retire {
            fs::remove_file(&source).map_err(error)?;
            File::open(chain)
                .and_then(|file| file.sync_all())
                .map_err(error)?;
        }
    }
    Ok(
        json!({"source":source,"index":chain.join(storage::JOURNAL_INDEX),"segments":index.chunks.len(),"uncompressed_bytes":index.chunks.iter().map(|c|c.uncompressed_bytes).sum::<u64>(),"zstd_bytes":index.chunks.iter().map(|c|c.compressed_bytes).sum::<u64>()}),
    )
}

struct ReceiptLog {
    directory: std::path::PathBuf,
    apply: bool,
    limit: usize,
    number: usize,
    decoded: usize,
    file: Option<File>,
    pending_deletes: Vec<std::path::PathBuf>,
}
impl ReceiptLog {
    fn new(harbor: &Path, apply: bool, limit: usize) -> Self {
        Self {
            directory: harbor.join("archive-receipts").join(format!(
                "{}-{}",
                crate::now_ms(),
                std::process::id()
            )),
            apply,
            limit,
            number: 0,
            decoded: 0,
            file: None,
            pending_deletes: Vec::new(),
        }
    }
    fn append(&mut self, value: &Value) -> Result<(), String> {
        if !self.apply {
            return Ok(());
        }
        let mut line = serde_json::to_vec(value).map_err(error)?;
        line.push(b'\n');
        if self.file.is_none() || self.decoded + line.len() > self.limit {
            self.flush()?;
            self.file = None;
            self.decoded = 0;
            self.number += 1;
            fs::create_dir_all(&self.directory).map_err(error)?;
            self.file = Some(
                OpenOptions::new()
                    .create_new(true)
                    .append(true)
                    .open(self.directory.join(format!("{:06}.jsonl.zst", self.number)))
                    .map_err(error)?,
            );
            File::open(&self.directory)
                .and_then(|directory| directory.sync_all())
                .map_err(error)?;
        }
        // Each receipt is a complete zstd frame. A crash cannot invalidate
        // previously committed receipts; several frames share one 16 MiB shard.
        let bytes = zstd::stream::encode_all(line.as_slice(), 3).map_err(error)?;
        let file = self.file.as_mut().unwrap();
        file.write_all(&bytes).map_err(error)?;
        self.decoded += line.len();
        Ok(())
    }
    fn retire(&mut self, path: &Path) -> Result<(), String> {
        self.pending_deletes.push(path.to_path_buf());
        if self.pending_deletes.len() >= 256 {
            self.flush()?;
        }
        Ok(())
    }
    fn flush(&mut self) -> Result<(), String> {
        if let Some(file) = &self.file {
            file.sync_data().map_err(error)?;
        }
        let mut directories = std::collections::BTreeSet::new();
        for path in std::mem::take(&mut self.pending_deletes) {
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.to_string()),
            }
            directories.insert(path.parent().unwrap().to_path_buf());
        }
        for directory in directories {
            File::open(directory)
                .and_then(|file| file.sync_all())
                .map_err(error)?;
        }
        Ok(())
    }
}

fn convert(
    source: &Path,
    apply: bool,
    retire: bool,
    receipts: &mut ReceiptLog,
) -> Result<Value, String> {
    // Read the original gzip, even if an earlier staging pass left a zstd copy.
    let reader = flate2::read::GzDecoder::new(File::open(source).map_err(error)?);
    let mut decoded = Vec::new();
    reader
        .take(32 * 1024 * 1024 + 1)
        .read_to_end(&mut decoded)
        .map_err(error)?;
    if decoded.len() > 32 * 1024 * 1024 {
        return Err(format!(
            "legacy object exceeds 32 MiB: {}",
            source.display()
        ));
    }
    serde_json::from_slice::<Value>(&decoded).map_err(error)?;
    let destination = source.with_extension("zst");
    let encoded = if apply && destination.is_file() {
        None
    } else {
        Some(zstd::stream::encode_all(decoded.as_slice(), 9).map_err(error)?)
    };
    let compressed_bytes = encoded
        .as_ref()
        .map(Vec::len)
        .map(|bytes| bytes as u64)
        .unwrap_or(
            fs::metadata(&destination)
                .map(|value| value.len())
                .unwrap_or(0),
        );
    let hash = storage::digest(&decoded);
    let record = json!({"schema":"benchmark-archive-receipt-v1","source":source,"destination":destination,"content_sha256":hash,"uncompressed_bytes":decoded.len(),"source_bytes":fs::metadata(source).map_err(error)?.len(),"zstd_bytes":compressed_bytes});
    if apply {
        if let Some(encoded) = encoded {
            storage::atomic_write(&destination, &encoded)?;
        }
        if storage::digest(&storage::read(&destination)?) != hash {
            return Err(format!(
                "archive verification failed: {}",
                destination.display()
            ));
        }
        receipts.append(&record)?;
        if retire {
            receipts.retire(source)?;
        }
    }
    Ok(record)
}

fn error(value: impl std::fmt::Display) -> String {
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn receipt_frames_are_durable_and_grouped_in_bounded_shards() {
        let root = std::env::temp_dir().join(format!("observer-receipts-{}", std::process::id()));
        let mut receipts = ReceiptLog::new(&root, true, 512);
        for sequence in 0..5 {
            receipts
                .append(&json!({"sequence":sequence,"text":"x".repeat(200)}))
                .unwrap();
        }
        let directory = receipts.directory.clone();
        drop(receipts);
        let files = fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(files.len(), 3);
        let mut count = 0;
        for file in files {
            let bytes = storage::read(&file).unwrap();
            assert!(bytes.len() <= 512);
            count += String::from_utf8(bytes).unwrap().lines().count();
        }
        assert_eq!(count, 5);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn sources_are_retired_only_after_the_receipt_batch_is_durable() {
        let root=std::env::temp_dir().join(format!("observer-retire-batch-{}",std::process::id()));fs::create_dir_all(&root).unwrap();
        let source=root.join("object.json.gz");let mut encoder=flate2::write::GzEncoder::new(Vec::new(),flate2::Compression::default());encoder.write_all(br#"{"score":7}"#).unwrap();fs::write(&source,encoder.finish().unwrap()).unwrap();
        let mut receipts=ReceiptLog::new(&root,true,storage::DEFAULT_CHUNK_BYTES);
        convert(&source,true,true,&mut receipts).unwrap();assert!(source.exists());
        receipts.flush().unwrap();assert!(!source.exists());assert_eq!(storage::read(&source.with_extension("zst")).unwrap(),br#"{"score":7}"#);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn chunks_preserve_records_and_resume_without_expanding_history() {
        let root = std::env::temp_dir().join(format!("observer-chunks-{}", std::process::id()));
        let chain = root.join("chain");
        fs::create_dir_all(&chain).unwrap();
        let bytes=(1..25).map(|sequence|format!("{}\n",json!({"schema":crate::RUN_EVENT_SCHEMA,"sequence":sequence,"source":"game","payload":{"score":sequence}}))).collect::<String>().into_bytes();
        fs::write(chain.join("journal.jsonl"), &bytes).unwrap();
        archive_journal(&chain, true, false, 512).unwrap();
        let index = storage::journal_index(&chain).unwrap();
        assert!(index.chunks.len() > 1);
        assert!(
            index
                .chunks
                .iter()
                .all(|chunk| chunk.uncompressed_bytes <= 512)
        );
        let mut decoded = Vec::new();
        storage::journal_reader(&chain)
            .unwrap()
            .read_to_end(&mut decoded)
            .unwrap();
        assert_eq!(decoded, bytes);
        archive_journal(&chain, true, true, 512).unwrap();
        assert!(!chain.join("journal.jsonl").exists());
        let next = format!(
            "{}\n",
            json!({"schema":crate::RUN_EVENT_SCHEMA,"sequence":25,"source":"runtime","type":"segment_finished"})
        );
        fs::write(chain.join("journal.jsonl"), &next).unwrap();
        let mut resumed = Vec::new();
        storage::journal_reader(&chain)
            .unwrap()
            .read_to_end(&mut resumed)
            .unwrap();
        assert_eq!(resumed, [bytes, next.into_bytes()].concat());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn archive_can_resume_without_changing_authority() {
        let root = std::env::temp_dir().join(format!("observer-archive-{}", std::process::id()));
        let chain = root.join(".harbor/run-journals/test");
        fs::create_dir_all(&chain).unwrap();
        let bytes=format!("{}\n",json!({"schema":crate::RUN_EVENT_SCHEMA,"sequence":1,"source":"runtime","type":"segment_finished","effective_elapsed_ms":12})).into_bytes();
        fs::write(chain.join("journal.jsonl"), &bytes).unwrap();
        let lock = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(chain.join("writer.lock"))
            .unwrap();
        lock.lock_exclusive().unwrap();
        assert_eq!(
            migrate(&root, true, true).unwrap()["skipped"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        drop(lock);
        assert_eq!(
            migrate(&root, true, true).unwrap()["files"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(!chain.join("journal.jsonl").exists());
        storage::restore_journal(&chain).unwrap();
        let mut restored = Vec::new();
        storage::journal_reader(&chain)
            .unwrap()
            .read_to_end(&mut restored)
            .unwrap();
        assert_eq!(restored, bytes);
        assert!(!chain.join("journal.jsonl").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
