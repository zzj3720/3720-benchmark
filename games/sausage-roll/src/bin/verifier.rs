use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader};

use sausage_terminal::{
    API_VERSION, CAMPAIGN_ID, Campaign, CampaignEntries, Command, Session, execute,
};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct AuditHeader {
    schema: String,
    api_version: String,
    campaign: String,
}

#[derive(Deserialize)]
struct AuditRecord {
    sequence: u64,
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
        .unwrap_or_else(|| "sausage-verifier".to_owned());
    let campaign_path = args.next().ok_or_else(|| usage(&program))?;
    let entries_path = args.next().ok_or_else(|| usage(&program))?;
    let audit_path = args.next().ok_or_else(|| usage(&program))?;
    if args.next().is_some() {
        return Err(usage(&program));
    }

    let campaign = Campaign::load_gzip(&campaign_path)?;
    let entries = CampaignEntries::load(&entries_path)?;
    entries.validate_against(&campaign)?;
    let mut session = Session::new(&campaign, &entries)?;
    let file = File::open(&audit_path)
        .map_err(|error| format!("could not open {}: {error}", audit_path.to_string_lossy()))?;
    let mut lines = BufReader::new(file).lines();
    let header_line = lines
        .next()
        .ok_or_else(|| "audit is empty".to_owned())?
        .map_err(|error| format!("could not read audit header: {error}"))?;
    let header: AuditHeader = serde_json::from_str(&header_line)
        .map_err(|error| format!("invalid audit header: {error}"))?;
    if header.schema != "sausage-audit-v1"
        || header.api_version != API_VERSION
        || header.campaign != CAMPAIGN_ID
    {
        return Err("audit header does not describe this frozen campaign".to_owned());
    }

    let mut previous_sequence = 0;
    let mut command_count = 0;
    for (index, line) in lines.enumerate() {
        let line = line.map_err(|error| format!("could not read audit: {error}"))?;
        let record: AuditRecord = serde_json::from_str(&line)
            .map_err(|error| format!("invalid audit line {}: {error}", index + 2))?;
        if record.sequence <= previous_sequence {
            return Err(format!(
                "audit sequence {} is not greater than {}",
                record.sequence, previous_sequence
            ));
        }
        let actual = execute(&mut session, &record.command);
        if actual != record.response {
            return Err(format!(
                "audit response mismatch at sequence {}",
                record.sequence
            ));
        }
        previous_sequence = record.sequence;
        command_count += 1;
    }
    let snapshot = session.snapshot()?;
    println!("score: {}", snapshot.campaign.score);
    println!(
        "verified: {} commands, {}/{} levels",
        command_count, snapshot.campaign.solved, snapshot.campaign.total
    );
    Ok(())
}

fn usage(program: &str) -> String {
    format!("usage: {program} CAMPAIGN_GZIP ENTRIES_TAR_GZ AUDIT_JSONL")
}
