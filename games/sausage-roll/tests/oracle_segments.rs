use flate2::read::GzDecoder;
use sausage_terminal::{
    Campaign, Direction, GameState, GuidedReplay, OracleCampaign, Replay, data_root,
};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use tar::Archive;

#[derive(Default)]
struct SegmentSources {
    state: Option<String>,
    replay: Option<String>,
}

fn load_sources() -> BTreeMap<String, SegmentSources> {
    let path = data_root().join("oracle").join("segments.tar.gz");
    let decoder = GzDecoder::new(File::open(path).expect("segment archive should exist"));
    let mut archive = Archive::new(decoder);
    let mut sources: BTreeMap<String, SegmentSources> = BTreeMap::new();
    for entry in archive.entries().expect("valid tar archive") {
        let mut entry = entry.expect("valid tar entry");
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path().expect("UTF-8-independent entry path");
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("UTF-8 oracle file name")
            .to_owned();
        if file_name.starts_with("._") {
            continue;
        }
        if !(file_name.ends_with(".state") || file_name.ends_with(".dem")) {
            continue;
        }
        let stem = file_name
            .strip_suffix(".state")
            .or_else(|| file_name.strip_suffix(".dem"))
            .expect("known suffix")
            .to_owned();
        let mut text = String::new();
        entry
            .read_to_string(&mut text)
            .unwrap_or_else(|error| panic!("{file_name} is not UTF-8: {error}"));
        let pair = sources.entry(stem).or_default();
        if file_name.ends_with(".state") {
            pair.state = Some(text);
        } else {
            pair.replay = Some(text);
        }
    }
    sources
}

#[test]
fn complete_tas_is_split_into_86_replayable_puzzle_segments() {
    let sources = load_sources();
    assert_eq!(sources.len(), 86);
    let mut total_actions = 0;
    let mut puzzle_ids = Vec::new();
    for (stem, pair) in sources {
        let state = GameState::parse(
            pair.state
                .as_deref()
                .unwrap_or_else(|| panic!("missing entry state for {stem}")),
        )
        .unwrap_or_else(|error| panic!("invalid entry state for {stem}: {error}"));
        let replay = Replay::parse(
            pair.replay
                .as_deref()
                .unwrap_or_else(|| panic!("missing replay for {stem}")),
        )
        .unwrap_or_else(|error| panic!("invalid replay for {stem}: {error}"));
        assert!(!state.overworld, "{stem} must start inside a puzzle");
        assert_eq!(state.push_target_level, stem[3..]);
        total_actions += replay.directions.len();
        puzzle_ids.push(state.push_target_level);
    }
    assert_eq!(total_actions, 11_769);
    assert!(puzzle_ids.iter().any(|name| name == "level11"));
    assert!(puzzle_ids.iter().any(|name| name == "modular8a__island1"));
}

#[test]
fn typed_walkthrough_replays_all_original_checkpoints() {
    let root = data_root();
    let campaign = Campaign::load_gzip(root.join("campaign").join("merged_binary.gz"))
        .expect("campaign should parse");
    let oracle = OracleCampaign::load(root.join("oracle").join("segments.tar.gz"))
        .expect("walkthrough archive should parse");
    oracle
        .validate_against(&campaign)
        .expect("complete walkthrough should validate");
}

#[test]
fn rust_engine_replays_the_complete_walkthrough() {
    let root = data_root();
    let campaign = Campaign::load_gzip(root.join("campaign").join("merged_binary.gz"))
        .expect("campaign should parse");
    let oracle = OracleCampaign::load(root.join("oracle").join("segments.tar.gz"))
        .expect("walkthrough archive should parse");
    let verified = oracle
        .verify_with_engine(&campaign)
        .expect("Rust engine should match every original-engine checkpoint");
    assert_eq!(verified.puzzles, 86);
    assert_eq!(verified.actions, 11_769);
    assert_eq!(verified.checkpoints, 11_683);
}

#[test]
fn typed_walkthrough_rejects_a_changed_direction() {
    let root = data_root();
    let oracle = OracleCampaign::load(root.join("oracle").join("segments.tar.gz"))
        .expect("walkthrough archive should parse");
    let segment = &oracle.segments[0];
    let expected = segment.replay.directions[0];
    let changed = match expected {
        Direction::North => Direction::South,
        _ => Direction::North,
    };
    let error = GuidedReplay::new(segment)
        .step(changed)
        .expect_err("changed action must be rejected");
    assert!(error.contains("expected"));
}
