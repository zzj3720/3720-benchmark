use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use kitchen_terminal::{API_VERSION, Command, GameData, Session, SessionConfig, execute};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
struct AuditHeader {
    schema: String,
    api_version: String,
    content_sha256: String,
    level: u8,
    scene: String,
    time_scale: u32,
    seed: u64,
}

#[derive(Deserialize)]
struct AuditRecord {
    sequence: u64,
    elapsed_ms: u64,
    command: Command,
    response: Value,
}

fn main() {
    if let Err(error) = verify() {
        eprintln!("verification failed: {error}");
        std::process::exit(1);
    }
}

fn verify() -> Result<(), String> {
    let mut args = env::args_os();
    let program = args
        .next()
        .and_then(|value| value.into_string().ok())
        .unwrap_or_else(|| "kitchen-verifier".to_owned());
    let data_root = args.next().ok_or_else(|| usage(&program))?;
    let audit_path = args.next().ok_or_else(|| usage(&program))?;
    if args.next().is_some() {
        return Err(usage(&program));
    }
    let data_root = PathBuf::from(data_root);
    let file = File::open(&audit_path)
        .map_err(|error| format!("could not open {}: {error}", audit_path.to_string_lossy()))?;
    let mut lines = BufReader::new(file).lines();
    let header_line = lines
        .next()
        .ok_or_else(|| "audit is empty".to_owned())?
        .map_err(|error| format!("could not read audit header: {error}"))?;
    let header: AuditHeader = serde_json::from_str(&header_line)
        .map_err(|error| format!("invalid audit header: {error}"))?;
    if header.schema != "overcooked-audit-v1" || header.api_version != API_VERSION {
        return Err("audit uses an unsupported schema or API".to_owned());
    }
    let data = GameData::load(&data_root, header.level)?;
    if data.layout.scene != header.scene {
        return Err("audit scene does not match the imported level".to_owned());
    }
    let config = SessionConfig {
        time_scale: header.time_scale,
        seed: header.seed,
    };
    if content_hash(&data_root, &data, config)? != header.content_sha256 {
        return Err("audit does not describe this frozen content/configuration".to_owned());
    }
    let mut session = Session::new(&data, config)?;
    let mut previous_sequence = 0;
    let mut previous_elapsed = 0;
    let mut command_count = 0;
    for (index, line) in lines.enumerate() {
        let line = line.map_err(|error| format!("could not read audit: {error}"))?;
        let record: AuditRecord = serde_json::from_str(&line)
            .map_err(|error| format!("invalid audit line {}: {error}", index + 2))?;
        if record.sequence <= previous_sequence {
            return Err(format!(
                "audit sequence {} is not greater than {previous_sequence}",
                record.sequence
            ));
        }
        if record.elapsed_ms < previous_elapsed || record.elapsed_ms > session.duration_ms() {
            return Err(format!(
                "invalid elapsed time {} at sequence {}",
                record.elapsed_ms, record.sequence
            ));
        }
        if session.started() {
            session.advance_to(record.elapsed_ms)?;
        } else if record.elapsed_ms != 0 {
            return Err("audit advances time before the shift starts".to_owned());
        }
        let actual = execute(&mut session, &record.command);
        if actual != record.response {
            return Err(format!(
                "audit response mismatch at sequence {}",
                record.sequence
            ));
        }
        previous_sequence = record.sequence;
        previous_elapsed = record.elapsed_ms;
        command_count += 1;
    }
    let score = session.final_score();
    let snapshot = session.snapshot();
    println!("score: {score}");
    println!("stars: {}", snapshot.campaign.stars);
    println!("verified: {command_count} commands through a deterministic real-time replay");
    Ok(())
}

fn content_hash(root: &Path, data: &GameData, config: SessionConfig) -> Result<String, String> {
    let mut digest = Sha256::new();
    for path in [
        root.join("campaign.json"),
        root.join("levels")
            .join(format!("{}.json", data.layout.scene)),
    ] {
        digest.update(
            fs::read(&path)
                .map_err(|error| format!("could not read {}: {error}", path.display()))?,
        );
    }
    digest.update(data.level.number.to_le_bytes());
    digest.update(config.time_scale.to_le_bytes());
    digest.update(config.seed.to_le_bytes());
    Ok(format!("{:x}", digest.finalize()))
}

fn usage(program: &str) -> String {
    format!("usage: {program} OVERCOOKED_DATA_ROOT AUDIT_JSONL")
}
