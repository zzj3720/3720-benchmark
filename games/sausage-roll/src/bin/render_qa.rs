use serde::Serialize;

use sausage_terminal::{
    Campaign, CampaignEntries, EntityType, GameSnapshot, OracleCampaign, data_root, replay_snapshot,
};

const SAMPLE_COUNT: usize = 64;

#[derive(Serialize)]
struct Features {
    tile_set: i32,
    tiles: usize,
    height_span: i32,
    sausages: usize,
    cooked_faces: usize,
    grills: usize,
    ladders: usize,
    detached_fork: bool,
    exit_ready: bool,
}

#[derive(Serialize)]
struct Sample {
    reference: String,
    title: String,
    area: &'static str,
    step: usize,
    total_steps: usize,
    features: Features,
    state: GameSnapshot,
}

#[derive(Serialize)]
struct Gallery {
    schema: &'static str,
    source_levels: usize,
    source_actions: usize,
    samples: Vec<Sample>,
}

fn main() {
    let root = data_root();
    let campaign = Campaign::load_gzip(root.join("campaign").join("merged_binary.gz"))
        .expect("campaign should load");
    let entries = CampaignEntries::load(root.join("campaign").join("entries.tar.gz"))
        .expect("entries should load");
    let oracle = OracleCampaign::load(root.join("oracle").join("segments.tar.gz"))
        .expect("walkthrough should load");
    entries
        .validate_against(&campaign)
        .expect("entries should cover the campaign");
    oracle
        .validate_against(&campaign)
        .expect("walkthrough should cover the campaign");

    let total_actions = oracle
        .segments
        .iter()
        .map(|segment| segment.replay.directions.len())
        .sum();
    let samples = (0..SAMPLE_COUNT)
        .map(|sample_index| {
            let level_index = sample_index * (entries.levels.len() - 1) / (SAMPLE_COUNT - 1);
            let entry = &entries.levels[level_index];
            let segment = &oracle.segments[level_index];
            assert_eq!(entry.id, segment.id);
            let total_steps = segment.replay.directions.len();
            let phase = sample_index % 5;
            let step = if phase == 4 {
                total_steps
            } else {
                total_steps.saturating_sub(1) * phase / 4
            };
            let state = replay_snapshot(
                &campaign,
                entry,
                entries.levels.len(),
                &segment.replay.directions[..step],
            )
            .unwrap_or_else(|error| panic!("{} step {step}: {error}", entry.id));
            let features = features(&state);
            Sample {
                reference: format!("{:02}-{}", entry.ordinal, entry.id),
                title: state
                    .level
                    .as_ref()
                    .expect("sample should have an active level")
                    .title
                    .clone(),
                area: area_name(features.tile_set),
                step,
                total_steps,
                features,
                state,
            }
        })
        .collect();
    let gallery = Gallery {
        schema: "sausage-render-qa-v1",
        source_levels: entries.levels.len(),
        source_actions: total_actions,
        samples,
    };
    println!(
        "{}",
        serde_json::to_string(&gallery).expect("gallery should serialize")
    );
}

fn features(state: &GameSnapshot) -> Features {
    let min_z = state
        .tiles
        .iter()
        .map(|tile| tile.pos.z)
        .min()
        .expect("sample should have visible terrain");
    let max_z = state
        .tiles
        .iter()
        .map(|tile| tile.pos.z)
        .max()
        .expect("sample should have visible terrain");
    Features {
        tile_set: state
            .level
            .as_ref()
            .map(|level| level.tile_set)
            .expect("sample should have an active level"),
        tiles: state.tiles.len(),
        height_span: max_z - min_z + 1,
        sausages: state
            .entities
            .iter()
            .filter(|entity| entity.kind == EntityType::Sausage)
            .count(),
        cooked_faces: state
            .entities
            .iter()
            .flat_map(|entity| entity.cooked_faces.into_iter().flatten())
            .filter(|face| *face != 0)
            .count(),
        grills: state
            .tiles
            .iter()
            .filter(|tile| tile.kind == "grill")
            .count(),
        ladders: state
            .tiles
            .iter()
            .filter(|tile| tile.kind == "ladder")
            .count(),
        detached_fork: state
            .entities
            .iter()
            .any(|entity| entity.kind == EntityType::Fork),
        exit_ready: state.exit_ready,
    }
}

fn area_name(tile_set: i32) -> &'static str {
    match tile_set {
        1 => "sand",
        2 => "snow",
        3 => "swamp",
        4 => "temple",
        _ => "green",
    }
}
