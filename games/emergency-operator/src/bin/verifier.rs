use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};

use operator_terminal::{API_VERSION, Campaign, Command, Session, execute};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
struct AuditHeader {
    schema: String,
    api_version: String,
    campaign: String,
    campaign_sha256: String,
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
        .unwrap_or_else(|| "operator-verifier".to_owned());
    let campaign_path = args.next().ok_or_else(|| usage(&program))?;
    let audit_path = args.next().ok_or_else(|| usage(&program))?;
    if args.next().is_some() {
        return Err(usage(&program));
    }

    let campaign_bytes = fs::read(&campaign_path).map_err(|error| {
        format!(
            "could not read {}: {error}",
            campaign_path.to_string_lossy()
        )
    })?;
    let campaign_hash = format!("{:x}", Sha256::digest(&campaign_bytes));
    let campaign = Campaign::load(&campaign_path)?;
    let file = File::open(&audit_path)
        .map_err(|error| format!("could not open {}: {error}", audit_path.to_string_lossy()))?;
    let mut lines = BufReader::new(file).lines();
    let header_line = lines
        .next()
        .ok_or_else(|| "audit is empty".to_owned())?
        .map_err(|error| format!("could not read audit header: {error}"))?;
    let header: AuditHeader = serde_json::from_str(&header_line)
        .map_err(|error| format!("invalid audit header: {error}"))?;
    if header.schema != "emergency-operator-audit-v1"
        || header.api_version != API_VERSION
        || header.campaign != campaign.id
        || header.campaign_sha256 != campaign_hash
    {
        return Err("audit header does not describe this frozen campaign".to_owned());
    }

    let mut session = Session::new(&campaign);
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
    println!("score: {score}");
    println!("max_score: {}", campaign.max_score);
    println!("verified: {command_count} commands through a deterministic real-time replay");
    Ok(())
}

fn usage(program: &str) -> String {
    format!("usage: {program} CAMPAIGN_JSON AUDIT_JSONL")
}
