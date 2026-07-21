use std::env;
use std::path::PathBuf;

use sausage_terminal::{Campaign, OracleCampaign, data_root};

fn main() {
    let mut args = env::args_os().skip(1).map(PathBuf::from);
    let campaign_path = args
        .next()
        .unwrap_or_else(|| data_root().join("campaign").join("merged_binary.gz"));
    let oracle_path = args
        .next()
        .unwrap_or_else(|| data_root().join("oracle").join("segments.tar.gz"));
    let campaign = Campaign::load_gzip(&campaign_path)
        .unwrap_or_else(|error| panic!("{}: {error}", campaign_path.display()));
    let oracle = OracleCampaign::load(&oracle_path)
        .unwrap_or_else(|error| panic!("{}: {error}", oracle_path.display()));
    oracle
        .validate_against(&campaign)
        .unwrap_or_else(|error| panic!("walkthrough validation failed: {error}"));

    let verified = oracle
        .verify_with_engine(&campaign)
        .unwrap_or_else(|error| panic!("engine replay failed: {error}"));
    println!(
        "verified: {} puzzles, {} actions, {} original-engine checkpoints",
        verified.puzzles, verified.actions, verified.checkpoints
    );
}
